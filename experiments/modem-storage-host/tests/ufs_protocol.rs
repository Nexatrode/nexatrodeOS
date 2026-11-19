use modem_storage_host::qcom_ufs_protocol as ufs;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);

struct DisposableImage {
    path: PathBuf,
    file: File,
}

impl Drop for DisposableImage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn read_command() -> ufs::ScsiCommand {
    ufs::ScsiCommand::read10(0x1234_5678, 2).unwrap()
}

fn response(lun: u8, tag: u8) -> [u8; ufs::UPIU_BYTES] {
    let mut response = [0; ufs::UPIU_BYTES];
    response[0] = 0x21;
    response[2] = lun;
    response[3] = tag;
    response
}

fn disposable_image() -> DisposableImage {
    let id = NEXT_IMAGE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sunfish-ufs-packet-{}-{id}.img",
        std::process::id()
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.set_len(16 * 512).unwrap();
    DisposableImage { path, file }
}

fn execute_on_image(
    image: &File,
    packet: &mut ufs::TransferPacket,
    data: &mut [u8],
) -> io::Result<()> {
    let cdb = &packet.command_upiu[16..26];
    let lun = packet.command_upiu[2];
    let task_tag = packet.command_upiu[3];
    match cdb[0] {
        0x28 | 0x2a => {
            let lba = u64::from(u32::from_be_bytes(cdb[2..6].try_into().unwrap()));
            let blocks = usize::from(u16::from_be_bytes(cdb[7..9].try_into().unwrap()));
            let bytes = blocks * ufs::SECTOR_BYTES as usize;
            if bytes != data.len() || packet.transfer_bytes != bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "transfer length mismatch",
                ));
            }
            if u16::from_le_bytes(packet.utrd[28..30].try_into().unwrap()) != 1
                || u32::from_le_bytes(packet.prdt[12..16].try_into().unwrap()) != bytes as u32 - 1
            {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "bad PRDT"));
            }
            let offset = lba * ufs::SECTOR_BYTES;
            let end = offset + bytes as u64;
            if end > image.metadata()?.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "outside image",
                ));
            }
            let mut done = 0;
            while done < bytes {
                let at = offset + done as u64;
                let count = if cdb[0] == 0x28 {
                    image.read_at(&mut data[done..], at)?
                } else {
                    image.write_at(&data[done..], at)?
                };
                if count == 0 {
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "short image IO"));
                }
                done += count;
            }
        }
        0x35 => {
            if !data.is_empty()
                || packet.transfer_bytes != 0
                || u16::from_le_bytes(packet.utrd[28..30].try_into().unwrap()) != 0
            {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "bad cache sync"));
            }
            image.sync_all()?;
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported CDB",
            ));
        }
    }
    packet.utrd[8..12].copy_from_slice(&0u32.to_le_bytes());
    let completion = response(lun, task_tag);
    ufs::validate_completion(&packet.utrd, &completion, lun, task_tag)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid completion"))
}

#[test]
fn read10_builds_bounded_big_endian_cdb_upiu_and_little_endian_dma_descriptors() {
    let command = read_command();
    let packet = ufs::encode_transfer(
        command,
        0,
        7,
        ufs::HciVersion::V2_0,
        true,
        0x0000_0001_2345_0000,
        Some(0x0000_0002_0000_1000),
    )
    .unwrap();

    assert_eq!(command.transfer_bytes(), 1024);
    assert_eq!(
        &command.cdb()[..10],
        &[0x28, 0, 0x12, 0x34, 0x56, 0x78, 0, 0, 2, 0]
    );
    assert_eq!(&packet.command_upiu[..8], &[0x01, 0x40, 0, 7, 0, 0, 0, 0]);
    assert_eq!(&packet.command_upiu[12..16], &1024u32.to_be_bytes());
    assert_eq!(&packet.command_upiu[16..26], &command.cdb()[..10]);

    // UFSHCI 2.0 uses the native-storage transfer command type.
    assert_eq!(
        u32::from_le_bytes(packet.utrd[0..4].try_into().unwrap()),
        0x1400_0000
    );
    assert_eq!(
        u32::from_le_bytes(packet.utrd[8..12].try_into().unwrap()),
        0x0f
    );
    assert_eq!(
        &packet.utrd[16..24],
        &0x0000_0001_2345_0000u64.to_le_bytes()
    );
    assert_eq!(
        u16::from_le_bytes(packet.utrd[24..26].try_into().unwrap()),
        128
    );
    assert_eq!(
        u16::from_le_bytes(packet.utrd[26..28].try_into().unwrap()),
        128
    );
    assert_eq!(
        u16::from_le_bytes(packet.utrd[28..30].try_into().unwrap()),
        1
    );
    assert_eq!(
        u16::from_le_bytes(packet.utrd[30..32].try_into().unwrap()),
        256
    );

    assert_eq!(&packet.prdt[..8], &0x0000_0002_0000_1000u64.to_le_bytes());
    assert_eq!(
        u32::from_le_bytes(packet.prdt[12..16].try_into().unwrap()),
        1023
    );
}

#[test]
fn write_and_flush_use_correct_direction_and_sync_has_no_prdt() {
    let write = ufs::ScsiCommand::write10(0x10, 1).unwrap();
    let packet = ufs::encode_transfer(
        write,
        0,
        1,
        ufs::HciVersion::V2_1,
        false,
        0x1000,
        Some(0x4000),
    )
    .unwrap();
    assert_eq!(&write.cdb()[..10], &[0x2a, 0, 0, 0, 0, 0x10, 0, 0, 1, 0]);
    assert_eq!(packet.command_upiu[1], 0x20);
    assert_eq!(
        u32::from_le_bytes(packet.utrd[0..4].try_into().unwrap()),
        0x0200_0000
    );
    assert_eq!(
        u16::from_le_bytes(packet.utrd[28..30].try_into().unwrap()),
        1
    );

    let sync = ufs::ScsiCommand::synchronize_cache10();
    let packet =
        ufs::encode_transfer(sync, 0, 2, ufs::HciVersion::V2_1, false, 0x2000, None).unwrap();
    assert_eq!(sync.cdb()[0], 0x35);
    assert_eq!(packet.command_upiu[1], 0);
    assert_eq!(&packet.command_upiu[12..16], &[0; 4]);
    assert_eq!(packet.prdt, [0; ufs::PRDT_BYTES]);
    assert_eq!(
        u16::from_le_bytes(packet.utrd[28..30].try_into().unwrap()),
        0
    );
}

#[test]
fn discovery_commands_encode_exact_cdbs_and_bounded_data_lengths() {
    let tur = ufs::ScsiCommand::test_unit_ready();
    assert_eq!(&tur.cdb()[..6], &[0x00, 0, 0, 0, 0, 0]);
    assert_eq!(tur.transfer_bytes(), 0);
    let tur_packet =
        ufs::encode_transfer(tur, 0, 1, ufs::HciVersion::V2_1, false, 0x1000, None).unwrap();
    assert_eq!(tur_packet.command_upiu[1], 0);
    assert_eq!(tur_packet.prdt, [0; ufs::PRDT_BYTES]);

    let inquiry = ufs::ScsiCommand::inquiry();
    assert_eq!(&inquiry.cdb()[..6], &[0x12, 0, 0, 0, 36, 0]);
    assert_eq!(inquiry.transfer_bytes(), 36);
    let inquiry_packet = ufs::encode_transfer(
        inquiry,
        0,
        2,
        ufs::HciVersion::V2_1,
        false,
        0x2000,
        Some(0x4000),
    )
    .unwrap();
    assert_eq!(inquiry_packet.command_upiu[1], 0x40);
    assert_eq!(
        u32::from_le_bytes(inquiry_packet.prdt[12..16].try_into().unwrap()),
        35
    );

    let capacity = ufs::ScsiCommand::read_capacity10();
    assert_eq!(&capacity.cdb()[..10], &[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(capacity.transfer_bytes(), 8);
}

#[test]
fn discovery_parsers_return_direct_lun_identity_and_512_byte_capacity() {
    let mut inquiry = [0; ufs::STANDARD_INQUIRY_BYTES];
    inquiry[0] = 0;
    inquiry[1] = 0x80;
    inquiry[4] = 31;
    inquiry[8..16].copy_from_slice(b"GOOGLE  ");
    inquiry[16..32].copy_from_slice(b"SUNFISH UFS     ");
    inquiry[32..36].copy_from_slice(b"0001");
    let identity = ufs::parse_standard_inquiry(&inquiry).unwrap();
    assert!(identity.removable);
    assert_eq!(&identity.vendor_id, b"GOOGLE  ");
    assert_eq!(&identity.product_id, b"SUNFISH UFS     ");
    assert_eq!(&identity.product_revision, b"0001");

    let capacity = ufs::parse_read_capacity10(&[
        0x00, 0x00, 0x3f, 0xff, // last LBA 16383
        0x00, 0x00, 0x02, 0x00, // 512-byte blocks
    ])
    .unwrap();
    assert_eq!(capacity.sectors, 16_384);
    assert_eq!(capacity.block_bytes, 512);

    let native_4k = ufs::parse_read_capacity10(&[
        0x00, 0x00, 0x3f, 0xff, // 16384 native 4 KiB blocks
        0x00, 0x00, 0x10, 0x00,
    ])
    .unwrap();
    assert_eq!(native_4k.sectors, 131_072);
    assert_eq!(native_4k.block_bytes, 4096);
}

#[test]
fn discovery_parsers_reject_malformed_or_unsupported_media() {
    assert_eq!(
        ufs::parse_standard_inquiry(&[0; ufs::STANDARD_INQUIRY_BYTES - 1]),
        Err(ufs::Error::DiscoveryResponse)
    );
    let mut non_disk = [0; ufs::STANDARD_INQUIRY_BYTES];
    non_disk[4] = 31;
    non_disk[0] = 5; // CD/DVD peripheral type, not a direct-access disk.
    assert_eq!(
        ufs::parse_standard_inquiry(&non_disk),
        Err(ufs::Error::UnsupportedPeripheral)
    );
    let mut short_standard = [0; ufs::STANDARD_INQUIRY_BYTES];
    short_standard[4] = 30;
    assert_eq!(
        ufs::parse_standard_inquiry(&short_standard),
        Err(ufs::Error::DiscoveryResponse)
    );
    assert_eq!(
        ufs::parse_read_capacity10(&[0; ufs::READ_CAPACITY10_BYTES - 1]),
        Err(ufs::Error::DiscoveryResponse)
    );
    assert_eq!(
        ufs::parse_read_capacity10(&[0xff, 0xff, 0xff, 0xff, 0, 0, 2, 0,]),
        Err(ufs::Error::CapacityNeeds16)
    );
    assert_eq!(
        ufs::parse_read_capacity10(&[0, 0, 0, 1, 0, 0, 4, 0]),
        Err(ufs::Error::UnsupportedBlockSize)
    );
}

#[test]
fn native_4k_commands_keep_lba_and_transfer_size_in_native_units() {
    let command = ufs::ScsiCommand::read10_with_block_size(0x1234, 1, 4096).unwrap();
    assert_eq!(command.transfer_bytes(), 4096);
    assert_eq!(&command.cdb()[2..6], &0x1234u32.to_be_bytes());
    assert_eq!(
        ufs::ScsiCommand::read10_with_block_size(0, 2, 4096),
        Err(ufs::Error::TooLarge)
    );
    assert_eq!(
        ufs::ScsiCommand::read10_with_block_size(0, 1, 2048),
        Err(ufs::Error::UnsupportedBlockSize)
    );
}

#[test]
fn invalid_ranges_addresses_and_unbounded_transfers_are_rejected() {
    assert_eq!(ufs::ScsiCommand::read10(0, 0), Err(ufs::Error::ZeroBlocks));
    assert_eq!(ufs::ScsiCommand::read10(0, 9), Err(ufs::Error::TooLarge));
    assert_eq!(
        ufs::ScsiCommand::read10(u64::from(u32::MAX), 2),
        Err(ufs::Error::Range)
    );

    let cases = [
        (8, 0, 0x1000, Some(0x2000), ufs::Error::InvalidLun),
        (0, 32, 0x1000, Some(0x2000), ufs::Error::InvalidTag),
        (0, 1, 0x1001, Some(0x2000), ufs::Error::MisalignedAddress),
        (0, 1, 0x1000, None, ufs::Error::MissingDataAddress),
        (0, 1, 0x1000, Some(0x2001), ufs::Error::MisalignedAddress),
        (0, 1, 0x1000, Some(0x1000), ufs::Error::AliasedBuffers),
        (0, 1, 0x1000, Some(0x1_0000_0000), ufs::Error::AddressWidth),
    ];
    for (lun, tag, command_desc, data, expected) in cases {
        assert_eq!(
            ufs::encode_transfer(
                read_command(),
                lun,
                tag,
                ufs::HciVersion::V2_1,
                false,
                command_desc,
                data,
            ),
            Err(expected)
        );
    }
    assert_eq!(
        ufs::encode_transfer(
            ufs::ScsiCommand::synchronize_cache10(),
            0,
            0,
            ufs::HciVersion::V2_1,
            false,
            0x1000,
            Some(0x2000),
        ),
        Err(ufs::Error::UnexpectedDataAddress)
    );
}

#[test]
fn completion_requires_successful_ocs_matching_response_and_no_residual() {
    let mut utrd = [0; ufs::UTRD_BYTES];
    let mut response = response(3, 11);
    assert_eq!(ufs::validate_completion(&utrd, &response, 3, 11), Ok(()));

    utrd[8] = 1;
    assert_eq!(
        ufs::validate_completion(&utrd, &response, 3, 11),
        Err(ufs::Error::Ocs)
    );
    utrd[8] = 0;

    response[0] = 0;
    assert_eq!(
        ufs::validate_completion(&utrd, &response, 3, 11),
        Err(ufs::Error::Response)
    );
    response[0] = 0x21;
    response[3] = 12;
    assert_eq!(
        ufs::validate_completion(&utrd, &response, 3, 11),
        Err(ufs::Error::Response)
    );
    response[3] = 11;
    response[7] = 2;
    assert_eq!(
        ufs::validate_completion(&utrd, &response, 3, 11),
        Err(ufs::Error::ScsiStatus)
    );
    response[7] = 0;
    response[15] = 1;
    assert_eq!(
        ufs::validate_completion(&utrd, &response, 3, 11),
        Err(ufs::Error::Residual)
    );
}

#[test]
fn encoded_write_sync_and_read_round_trip_a_disposable_image() {
    let image = disposable_image();
    let source = vec![0xa5; 1024];
    let mut write_data = source.clone();
    let mut write_packet = ufs::encode_transfer(
        ufs::ScsiCommand::write10(2, 2).unwrap(),
        0,
        3,
        ufs::HciVersion::V2_1,
        false,
        0x1000,
        Some(0x3000),
    )
    .unwrap();
    execute_on_image(&image.file, &mut write_packet, &mut write_data).unwrap();

    let mut sync_packet = ufs::encode_transfer(
        ufs::ScsiCommand::synchronize_cache10(),
        0,
        4,
        ufs::HciVersion::V2_1,
        false,
        0x2000,
        None,
    )
    .unwrap();
    execute_on_image(&image.file, &mut sync_packet, &mut []).unwrap();

    let mut read_data = vec![0; source.len()];
    let mut read_packet = ufs::encode_transfer(
        ufs::ScsiCommand::read10(2, 2).unwrap(),
        0,
        5,
        ufs::HciVersion::V2_1,
        false,
        0x4000,
        Some(0x5000),
    )
    .unwrap();
    execute_on_image(&image.file, &mut read_packet, &mut read_data).unwrap();
    assert_eq!(read_data, source);
}
