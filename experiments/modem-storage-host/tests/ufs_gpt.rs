use modem_storage_host::qcom_ufs_gpt as gpt;
use std::{env, fs};

const LUN_SECTORS: u64 = 200_000;
const ENTRY_COUNT: usize = 128;
const ENTRY_BYTES: usize = 128;

fn fixture() -> ([u8; gpt::GPT_HEADER_BYTES], Vec<u8>) {
    let mut entries = vec![0; ENTRY_COUNT * ENTRY_BYTES];
    add_entry(&mut entries, 0, b"modemst1", 100, 199, 1);
    add_entry(&mut entries, 1, b"modemst2", 200, 299, 2);
    add_entry(&mut entries, 2, b"fsg", 300, 399, 3);
    add_entry(&mut entries, 3, b"fsc", 400, 499, 4);
    add_entry(&mut entries, 4, b"userdata", 500, 1499, 5);
    add_entry(&mut entries, 5, b"super", 2000, 12000, 6);

    let mut header = [0; gpt::GPT_HEADER_BYTES];
    header[0..8].copy_from_slice(b"EFI PART");
    put32(&mut header, 8, 0x0001_0000);
    put32(&mut header, 12, 92);
    put64(&mut header, 24, 1);
    put64(&mut header, 32, LUN_SECTORS - 1);
    put64(&mut header, 40, 34);
    put64(&mut header, 48, LUN_SECTORS - 34);
    for (index, byte) in header[56..72].iter_mut().enumerate() {
        *byte = index as u8 + 1;
    }
    put64(&mut header, 72, 2);
    put32(&mut header, 80, ENTRY_COUNT as u32);
    put32(&mut header, 84, ENTRY_BYTES as u32);
    refresh_crcs(&mut header, &entries);
    (header, entries)
}

fn pair_fixture() -> (
    [u8; gpt::GPT_HEADER_BYTES],
    Vec<u8>,
    [u8; gpt::GPT_HEADER_BYTES],
    Vec<u8>,
) {
    let (primary_header, primary_entries) = fixture();
    let mut backup_header = primary_header;
    let backup_entries = primary_entries.clone();
    let array_sectors = (backup_entries.len() / gpt::LOGICAL_BLOCK_BYTES) as u64;
    put64(&mut backup_header, 24, LUN_SECTORS - 1);
    put64(&mut backup_header, 32, 1);
    put64(&mut backup_header, 72, LUN_SECTORS - 1 - array_sectors);
    refresh_crcs(&mut backup_header, &backup_entries);
    (
        primary_header,
        primary_entries,
        backup_header,
        backup_entries,
    )
}

fn add_entry(entries: &mut [u8], index: usize, name: &[u8], first: u64, last: u64, id: u8) {
    let at = index * ENTRY_BYTES;
    entries[at..at + 16].fill(0x42); // Nonzero partition type GUID.
    entries[at + 16] = id;
    put64(entries, at + 32, first);
    put64(entries, at + 40, last);
    for (offset, byte) in name.iter().enumerate() {
        let code_unit = u16::from(*byte).to_le_bytes();
        entries[at + 56 + offset * 2..at + 58 + offset * 2].copy_from_slice(&code_unit);
    }
}

fn set_name(entries: &mut [u8], index: usize, name: &[u8]) {
    let at = index * ENTRY_BYTES + 56;
    entries[at..at + 72].fill(0);
    for (offset, byte) in name.iter().enumerate() {
        entries[at + offset * 2..at + offset * 2 + 2]
            .copy_from_slice(&u16::from(*byte).to_le_bytes());
    }
}

fn refresh_crcs(header: &mut [u8; gpt::GPT_HEADER_BYTES], entries: &[u8]) {
    put32(header, 88, crc32(entries));
    put32(header, 16, 0);
    let header_crc = crc32(&header[..92]);
    put32(header, 16, header_crc);
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn parses_primary_gpt_and_returns_exact_modem_partition_extents() {
    let (header, entries) = fixture();
    let found = gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS).unwrap();
    assert_eq!(found.modemst1.first_lba, 100);
    assert_eq!(found.modemst1.last_lba, 199);
    assert_eq!(found.modemst1.sectors, 100);
    assert_eq!(found.modemst2.first_lba, 200);
    assert_eq!(found.fsg.first_lba, 300);
    assert_eq!(found.fsc.last_lba, 499);
    assert_eq!(found.super_partition.first_lba, 2000);
    assert_eq!(found.super_partition.first_lba, 2000);
    assert_eq!(found.super_partition.last_lba, 12000);
    assert_ne!(found.disk_guid, [0; 16]);
}

#[test]
fn parses_matching_primary_and_backup_gpt_copies() {
    let (primary_header, primary_entries, backup_header, backup_entries) = pair_fixture();
    let found = gpt::parse_sunfish_pair(
        &primary_header,
        &primary_entries,
        &backup_header,
        &backup_entries,
        LUN_SECTORS,
    )
    .unwrap();
    assert_eq!(found.modemst1.first_lba, 100);
    assert_eq!(found.modemst2.first_lba, 200);
    assert_eq!(found.fsg.first_lba, 300);
    assert_eq!(found.fsc.last_lba, 499);
}

#[test]
fn parses_native_4k_gpt_lbas_without_reinterpreting_them_as_512_byte_sectors() {
    const NATIVE_BLOCKS: u64 = LUN_SECTORS / 8;
    let (mut primary, entries, mut backup, backup_entries) = pair_fixture();
    put64(&mut primary, 32, NATIVE_BLOCKS - 1);
    put64(&mut primary, 48, NATIVE_BLOCKS - 6);
    refresh_crcs(&mut primary, &entries);
    put64(&mut backup, 24, NATIVE_BLOCKS - 1);
    put64(&mut backup, 40, 34);
    put64(&mut backup, 48, NATIVE_BLOCKS - 6);
    put64(&mut backup, 72, NATIVE_BLOCKS - 5);
    refresh_crcs(&mut backup, &backup_entries);

    let found = gpt::parse_sunfish_pair_verified_with_block_size(
        &primary,
        &entries,
        &backup,
        &backup_entries,
        NATIVE_BLOCKS,
        4096,
    )
    .unwrap()
    .partitions();
    assert_eq!(found.logical_block_bytes, 4096);
    assert_eq!(found.modemst1.first_lba, 100);
    assert_eq!(found.modemst1.sectors, 100);
    assert_eq!(found.super_partition.first_lba, 2000);
    assert_eq!(found.super_partition.sectors, 10_001);
}

#[test]
fn super_only_gpt_proof_does_not_require_absent_modem_nv_partitions() {
    let (mut primary, mut entries, mut backup, mut backup_entries) = pair_fixture();
    set_name(&mut entries, 0, b"modem_a");
    set_name(&mut entries, 1, b"modem_b");
    set_name(&mut entries, 2, b"boot_a");
    set_name(&mut entries, 3, b"boot_b");
    backup_entries.copy_from_slice(&entries);
    refresh_crcs(&mut primary, &entries);
    refresh_crcs(&mut backup, &backup_entries);

    assert_eq!(
        gpt::parse_sunfish_pair(&primary, &entries, &backup, &backup_entries, LUN_SECTORS),
        Err(gpt::Error::MissingPartition)
    );
    let found = gpt::parse_sunfish_super_pair_verified_with_block_size(
        &primary,
        &entries,
        &backup,
        &backup_entries,
        LUN_SECTORS,
        512,
    )
    .unwrap()
    .partition();
    assert_eq!(found.extent.first_lba, 2000);
    assert_eq!(found.extent.last_lba, 12000);
    assert_eq!(found.logical_block_bytes, 512);
}

#[test]
fn parses_4k_super_pair_with_short_entry_array_and_zero_disk_guid() {
    const NATIVE_BLOCKS: u64 = LUN_SECTORS / 8;
    const ENTRY_COUNT: usize = 6;
    let (mut primary, mut entries, mut backup, mut backup_entries) = pair_fixture();
    entries.truncate(ENTRY_COUNT * ENTRY_BYTES);
    backup_entries.clone_from(&entries);

    put64(&mut primary, 32, NATIVE_BLOCKS - 1);
    put64(&mut primary, 40, 6);
    put64(&mut primary, 48, NATIVE_BLOCKS - 6);
    put32(&mut primary, 80, ENTRY_COUNT as u32);
    primary[56..72].fill(0);
    refresh_crcs(&mut primary, &entries);

    put64(&mut backup, 24, NATIVE_BLOCKS - 1);
    put64(&mut backup, 32, 1);
    put64(&mut backup, 40, 6);
    put64(&mut backup, 48, NATIVE_BLOCKS - 6);
    put64(&mut backup, 72, NATIVE_BLOCKS - 2);
    put32(&mut backup, 80, ENTRY_COUNT as u32);
    backup[56..72].fill(0);
    refresh_crcs(&mut backup, &backup_entries);

    let found = gpt::parse_sunfish_super_pair_verified_with_block_size(
        &primary,
        &entries,
        &backup,
        &backup_entries,
        NATIVE_BLOCKS,
        4096,
    )
    .unwrap()
    .partition();
    assert_eq!(found.disk_guid, [0; 16]);
    assert_eq!(found.extent.first_lba, 2000);
    assert_eq!(found.extent.last_lba, 12000);
    assert_eq!(found.logical_block_bytes, 4096);
}

#[test]
#[ignore = "requires a read-only Sunfish GPT-prefix capture"]
fn parses_live_sunfish_4k_user_gpt_and_locates_super() {
    const HEADER_OFFSET: usize = 4096;
    const ENTRIES_OFFSET: usize = 8192;
    let capture = env::var("SUNFISH_GPT_PREFIX").expect("set SUNFISH_GPT_PREFIX");
    let native_blocks = env::var("SUNFISH_GPT_NATIVE_BLOCKS")
        .expect("set SUNFISH_GPT_NATIVE_BLOCKS")
        .parse::<u64>()
        .unwrap();
    let bytes = fs::read(capture).unwrap();
    let header: &[u8; gpt::GPT_HEADER_BYTES] = bytes
        [HEADER_OFFSET..HEADER_OFFSET + gpt::GPT_HEADER_BYTES]
        .try_into()
        .unwrap();
    let entry_count = u32::from_le_bytes(header[80..84].try_into().unwrap()) as usize;
    let entry_bytes = u32::from_le_bytes(header[84..88].try_into().unwrap()) as usize;
    let array_len = entry_count.checked_mul(entry_bytes).unwrap();
    assert_eq!(entry_count, 15);
    assert_eq!(entry_bytes, ENTRY_BYTES);
    assert!(bytes.len() >= ENTRIES_OFFSET + array_len);
    let entries = &bytes[ENTRIES_OFFSET..ENTRIES_OFFSET + array_len];

    assert_eq!(
        gpt::parse_sunfish_primary_with_block_size(header, entries, native_blocks, 4096),
        Err(gpt::Error::DiskGuid)
    );
    let super_part =
        gpt::parse_sunfish_super_primary_with_block_size(header, entries, native_blocks, 4096)
            .unwrap();
    assert_eq!(super_part.extent.first_lba, 98_304);
    assert_eq!(super_part.extent.last_lba, 2_480_127);
    assert_eq!(super_part.extent.sectors, 2_381_824);
    assert_eq!(super_part.logical_block_bytes, 4096);
    assert_eq!(super_part.disk_guid, [0; 16]);
}

#[test]
fn rejects_bad_backup_location_header_crc_and_array_geometry() {
    let (primary_header, primary_entries, mut backup_header, backup_entries) = pair_fixture();
    put64(&mut backup_header, 24, LUN_SECTORS - 2);
    refresh_crcs(&mut backup_header, &backup_entries);
    assert_eq!(
        gpt::parse_sunfish_pair(
            &primary_header,
            &primary_entries,
            &backup_header,
            &backup_entries,
            LUN_SECTORS,
        ),
        Err(gpt::Error::BackupLocation)
    );

    let (primary_header, primary_entries, mut backup_header, backup_entries) = pair_fixture();
    backup_header[40] ^= 1;
    assert_eq!(
        gpt::parse_sunfish_pair(
            &primary_header,
            &primary_entries,
            &backup_header,
            &backup_entries,
            LUN_SECTORS,
        ),
        Err(gpt::Error::BackupHeaderCrc)
    );

    let (primary_header, primary_entries, mut backup_header, backup_entries) = pair_fixture();
    put64(&mut backup_header, 72, LUN_SECTORS - 1);
    refresh_crcs(&mut backup_header, &backup_entries);
    assert_eq!(
        gpt::parse_sunfish_pair(
            &primary_header,
            &primary_entries,
            &backup_header,
            &backup_entries,
            LUN_SECTORS,
        ),
        Err(gpt::Error::BackupGeometry)
    );
}

#[test]
fn rejects_bad_backup_array_length_and_crc() {
    let (primary_header, primary_entries, backup_header, backup_entries) = pair_fixture();
    assert_eq!(
        gpt::parse_sunfish_pair(
            &primary_header,
            &primary_entries,
            &backup_header,
            &backup_entries[..backup_entries.len() - 1],
            LUN_SECTORS,
        ),
        Err(gpt::Error::BackupEntryArrayLength)
    );

    let (primary_header, primary_entries, backup_header, mut backup_entries) = pair_fixture();
    backup_entries[0] ^= 1;
    assert_eq!(
        gpt::parse_sunfish_pair(
            &primary_header,
            &primary_entries,
            &backup_header,
            &backup_entries,
            LUN_SECTORS,
        ),
        Err(gpt::Error::BackupEntryArrayCrc)
    );
}

#[test]
fn rejects_backup_table_content_or_disk_guid_mismatch() {
    let (primary_header, primary_entries, mut backup_header, mut backup_entries) = pair_fixture();
    backup_entries[4 * ENTRY_BYTES + 56] ^= 1;
    refresh_crcs(&mut backup_header, &backup_entries);
    assert_eq!(
        gpt::parse_sunfish_pair(
            &primary_header,
            &primary_entries,
            &backup_header,
            &backup_entries,
            LUN_SECTORS,
        ),
        Err(gpt::Error::CopiesMismatch)
    );

    let (primary_header, primary_entries, mut backup_header, backup_entries) = pair_fixture();
    backup_header[56] ^= 1;
    refresh_crcs(&mut backup_header, &backup_entries);
    assert_eq!(
        gpt::parse_sunfish_pair(
            &primary_header,
            &primary_entries,
            &backup_header,
            &backup_entries,
            LUN_SECTORS,
        ),
        Err(gpt::Error::CopiesMismatch)
    );
}

#[test]
fn rejects_header_signature_revision_crc_reserved_and_primary_location_errors() {
    let (mut header, entries) = fixture();
    header[0] ^= 1;
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::Signature)
    );

    let (mut header, entries) = fixture();
    put32(&mut header, 8, 0x0001_0001);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::Revision)
    );

    let (mut header, entries) = fixture();
    header[40] ^= 1;
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::HeaderCrc)
    );

    let (mut header, entries) = fixture();
    header[100] = 1;
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::Reserved)
    );

    let (mut header, entries) = fixture();
    put64(&mut header, 24, 2);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::PrimaryLocation)
    );
}

#[test]
fn rejects_invalid_header_and_entry_array_geometry() {
    let (mut header, entries) = fixture();
    put32(&mut header, 12, 91);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::HeaderSize)
    );

    let (mut header, entries) = fixture();
    put64(&mut header, 48, LUN_SECTORS);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::Geometry)
    );

    let (mut header, entries) = fixture();
    put32(&mut header, 80, (gpt::MAX_PARTITION_ENTRIES + 1) as u32);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::EntryGeometry)
    );

    let (mut header, entries) = fixture();
    put32(&mut header, 84, 192);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::EntryGeometry)
    );

    let (mut header, entries) = fixture();
    put64(&mut header, 72, u64::MAX);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::Geometry)
    );
}

#[test]
fn rejects_wrong_entry_array_length_and_crc() {
    let (header, entries) = fixture();
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries[..entries.len() - 1], LUN_SECTORS),
        Err(gpt::Error::EntryArrayLength)
    );
    let mut corrupt_entries = entries;
    corrupt_entries[0] ^= 1;
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &corrupt_entries, LUN_SECTORS),
        Err(gpt::Error::EntryArrayCrc)
    );
}

#[test]
fn rejects_empty_or_duplicate_guid_and_partition_entries() {
    let (mut header, mut entries) = fixture();
    entries[16] = 0;
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::EntryInvalid)
    );

    let (mut header, mut entries) = fixture();
    entries[ENTRY_BYTES + 16] = entries[16];
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::DuplicateGuid)
    );

    let (mut header, mut entries) = fixture();
    entries[ENTRY_BYTES + 16..ENTRY_BYTES + 32].fill(0);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::EntryInvalid)
    );
}

#[test]
fn rejects_out_of_capacity_and_overlapping_partition_extents() {
    let (mut header, mut entries) = fixture();
    put64(&mut entries, 32, 10);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::PartitionBounds)
    );

    let (mut header, mut entries) = fixture();
    put64(&mut entries, ENTRY_BYTES + 32, 150);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::PartitionOverlap)
    );

    let (mut header, mut entries) = fixture();
    put64(&mut entries, 32, LUN_SECTORS - 1);
    put64(&mut entries, 40, LUN_SECTORS);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::PartitionBounds)
    );
}

#[test]
fn requires_each_exact_modem_and_super_partition_name_once() {
    let (mut header, mut entries) = fixture();
    entries[3 * ENTRY_BYTES + 56..3 * ENTRY_BYTES + 128].fill(0);
    for (offset, byte) in b"modemst1".iter().enumerate() {
        let code_unit = u16::from(*byte).to_le_bytes();
        entries[3 * ENTRY_BYTES + 56 + offset * 2..3 * ENTRY_BYTES + 58 + offset * 2]
            .copy_from_slice(&code_unit);
    }
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::DuplicateName)
    );

    let (mut header, mut entries) = fixture();
    entries[0 * ENTRY_BYTES + 56..1 * ENTRY_BYTES].fill(0);
    for (offset, byte) in b"MODEMST1".iter().enumerate() {
        let code_unit = u16::from(*byte).to_le_bytes();
        entries[56 + offset * 2..58 + offset * 2].copy_from_slice(&code_unit);
    }
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::MissingPartition)
    );

    let (mut header, mut entries) = fixture();
    entries[5 * ENTRY_BYTES + 56..5 * ENTRY_BYTES + 128].fill(0);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::MissingPartition)
    );
}

#[test]
fn refuses_unusable_capacity_and_zero_disk_guid() {
    let (header, entries) = fixture();
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, 1),
        Err(gpt::Error::InvalidCapacity)
    );

    let (mut header, entries) = fixture();
    header[56..72].fill(0);
    refresh_crcs(&mut header, &entries);
    assert_eq!(
        gpt::parse_sunfish_primary(&header, &entries, LUN_SECTORS),
        Err(gpt::Error::DiskGuid)
    );
}
