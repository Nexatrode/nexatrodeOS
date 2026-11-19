#![forbid(unsafe_code)]
#![allow(dead_code)]

#[path = "../rust/src/qcom_tftp.rs"]
mod qcom_tftp;
#[path = "../rust/src/sunfish_lpd.rs"]
mod sunfish_lpd;
#[path = "../rust/src/sunfish_vendor_ext4.rs"]
mod sunfish_vendor_ext4;
#[path = "../rust/src/sunfish_vendor_wlan_source.rs"]
mod sunfish_vendor_wlan_source;

use qcom_tftp::{ReadSource, ReadSourceCell, ReadSourceIo};
use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::rc::Rc;
use sunfish_lpd::{Error, VendorPartition};
use sunfish_vendor_wlan_source::{
    Error as SourceError, SunfishVendorBoardSource, SunfishVendorWlanSource,
};

const SECTOR: usize = 512;
const DISK_SECTORS: usize = 8192;
const DISK_BYTES: usize = SECTOR * DISK_SECTORS;
const META_MAX: usize = 65536;
const PRIMARY_META: usize = 4096 + 8192;
const BACKUP_META: usize = PRIMARY_META + META_MAX * 3;
const TABLES_BYTES: usize = 2 * 52 + 4 * 24 + 2 * 48 + 64;
const LIVE_METADATA_PREFIX_BYTES: u64 = 512 * 1024;

#[derive(Clone)]
struct MemoryIo {
    bytes: Rc<RefCell<Vec<u8>>>,
    visible_len: Rc<Cell<u64>>,
    reads: Rc<RefCell<Vec<(u64, usize)>>>,
}

impl ReadSourceIo for MemoryIo {
    fn len(&mut self) -> Result<u64, ()> {
        Ok(self.visible_len.get())
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        let start = usize::try_from(offset).map_err(|_| ())?;
        let end = start.checked_add(out.len()).ok_or(())?;
        out.copy_from_slice(self.bytes.borrow().get(start..end).ok_or(())?);
        self.reads.borrow_mut().push((offset, out.len()));
        Ok(())
    }
}

fn source(bytes: Vec<u8>) -> MemoryIo {
    let len = bytes.len() as u64;
    MemoryIo {
        bytes: Rc::new(RefCell::new(bytes)),
        visible_len: Rc::new(Cell::new(len)),
        reads: Rc::new(RefCell::new(Vec::new())),
    }
}

struct PrefixFileIo {
    file: File,
    source_bytes: u64,
    captured_bytes: u64,
}

impl ReadSourceIo for PrefixFileIo {
    fn len(&mut self) -> Result<u64, ()> {
        Ok(self.source_bytes)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        let end = offset.checked_add(out.len() as u64).ok_or(())?;
        if end > self.captured_bytes {
            return Err(());
        }
        self.file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
        self.file.read_exact(out).map_err(|_| ())
    }
}

fn image() -> Vec<u8> {
    let mut disk = vec![0u8; DISK_BYTES];
    write_geometry(&mut disk);

    let mut tables = vec![0u8; TABLES_BYTES];
    let partition = &mut tables[..52];
    partition[..9].copy_from_slice(b"vendor_a\0");
    put32(partition, 36, 1 | 4); // readonly | updated
    put32(partition, 40, 0);
    put32(partition, 44, 3);
    put32(partition, 48, 0);
    let cow_partition = &mut tables[52..104];
    cow_partition[..13].copy_from_slice(b"vendor_a-cow\0");
    put32(cow_partition, 40, 3);
    put32(cow_partition, 44, 1);
    put32(cow_partition, 48, 1);

    let extents = &mut tables[104..104 + 4 * 24];
    write_extent(extents, 0, 24, 0, 1504, 0);
    write_extent(extents, 24, 16, 1, 0, 0);
    write_extent(extents, 48, 24, 0, 2504, 0);
    write_extent(extents, 72, 8, 0, 3000, 0);

    let group_at = 104 + 4 * 24;
    tables[group_at..group_at + 7].copy_from_slice(b"default");
    tables[group_at + 48..group_at + 51].copy_from_slice(b"cow");
    let device_at = group_at + 48;
    let device_at = device_at + 48;
    put64(&mut tables, device_at, 1024);
    put32(&mut tables, device_at + 8, 4096);
    put64(&mut tables, device_at + 16, DISK_BYTES as u64);
    tables[device_at + 24..device_at + 30].copy_from_slice(b"super\0");

    write_metadata(&mut disk, PRIMARY_META, &tables);
    write_metadata(&mut disk, BACKUP_META, &tables);
    disk[1504 * SECTOR..1528 * SECTOR].fill(b'A');
    disk[2504 * SECTOR..2528 * SECTOR].fill(b'C');
    disk
}

fn super_image_with_vendor(ext4: &[u8]) -> Vec<u8> {
    let mut disk = image();
    let vendor_first_sector = 1024u64;
    let vendor_sectors = u64::try_from(ext4.len()).unwrap() / SECTOR as u64;
    assert_eq!(ext4.len() as u64 % SECTOR as u64, 0);
    let vendor_at = usize::try_from(vendor_first_sector * SECTOR as u64).unwrap();
    disk[vendor_at..vendor_at + ext4.len()].copy_from_slice(ext4);

    for metadata_at in [PRIMARY_META, BACKUP_META] {
        let tables_at = metadata_at + 128;
        put32(&mut disk[tables_at..], 44, 1); // vendor_a has one extent.
        write_extent(
            &mut disk[tables_at + 104..],
            0,
            vendor_sectors,
            0,
            vendor_first_sector,
            0,
        );
        rewrite_table_and_header_hash(&mut disk, metadata_at);
    }
    disk
}

mod ext4_fixture {
    const BLOCK: usize = 4096;
    const BLOCKS: usize = 64;
    pub const FILE_BYTES: usize = BLOCK * 2 + 17;
    pub const BOARD_DATA: &[u8] =
        b"sunfish-board-data-fixture\0sunfish-board-data-fixture\0sunfish-board-data-fixture\0";

    pub fn image() -> Vec<u8> {
        let mut disk = vec![0u8; BLOCK * BLOCKS];
        let sb = &mut disk[1024..2048];
        put32(sb, 0, 32);
        put32(sb, 4, BLOCKS as u32);
        put32(sb, 20, 0);
        put32(sb, 24, 2);
        put32(sb, 28, 2);
        put32(sb, 32, 32);
        put32(sb, 36, 32);
        put32(sb, 40, 16);
        put16(sb, 56, 0xef53);
        put32(sb, 76, 1);
        put16(sb, 88, 256);
        put32(sb, 92, 0x28);
        put32(sb, 96, 0x42);
        put32(sb, 100, 0x7b);
        sb[104..120].fill(0x11);
        put16(sb, 254, 0);

        let group0 = &mut disk[BLOCK..BLOCK + 32];
        put32(group0, 8, 2);
        let group1 = &mut disk[BLOCK + 32..BLOCK + 64];
        put32(group1, 8, 32);
        for group in 0..2u32 {
            let at = BLOCK + group as usize * 32;
            let mut desc = [0u8; 32];
            desc.copy_from_slice(&disk[at..at + 32]);
            let checksum = group_checksum(&[0x11; 16], group, &desc);
            put16(&mut disk[at..at + 32], 30, checksum);
        }

        write_inode(&mut disk, 2, 0x4000, 4096, 1, 18);
        write_inode(&mut disk, 3, 0x4000, 4096, 1, 19);
        write_inode(&mut disk, 4, 0x8000, FILE_BYTES as u32, 2, 20);
        write_inode(&mut disk, 5, 0x8000, BOARD_DATA.len() as u32, 1, 24);
        let file_inode_at = BLOCK * 2 + 3 * 256;
        let file_inode = &mut disk[file_inode_at..file_inode_at + 256];
        put16(file_inode, 40, 0xf30a);
        put16(file_inode, 42, 1);
        put16(file_inode, 44, 4);
        put16(file_inode, 46, 1);
        put32(file_inode, 52, 0);
        put32(file_inode, 56, 20);
        put16(file_inode, 60, 0);

        write_directory(
            &mut disk[18 * BLOCK..19 * BLOCK],
            &[(2, b".", 2), (2, b"..", 2), (3, b"firmware", 2)],
        );
        write_directory(
            &mut disk[19 * BLOCK..20 * BLOCK],
            &[
                (3, b".", 2),
                (2, b"..", 2),
                (4, b"wlanmdsp.mbn", 1),
                (5, b"bdwlan-sunfish.bin", 1),
            ],
        );
        let node = &mut disk[20 * BLOCK..21 * BLOCK];
        put16(node, 0, 0xf30a);
        put16(node, 2, 2);
        put16(node, 4, 340);
        put16(node, 6, 0);
        write_extent(node, 12, 0, 1, 21);
        write_extent(node, 24, 2, 1, 23);
        disk[21 * BLOCK..22 * BLOCK].fill(0x41);
        disk[23 * BLOCK..24 * BLOCK].fill(0x43);
        disk[24 * BLOCK..24 * BLOCK + BOARD_DATA.len()].copy_from_slice(BOARD_DATA);
        disk
    }

    fn write_inode(
        disk: &mut [u8],
        ino: usize,
        mode: u16,
        size: u32,
        extent_len: u16,
        physical: u32,
    ) {
        let at = BLOCK * 2 + (ino - 1) * 256;
        let inode = &mut disk[at..at + 256];
        put16(inode, 0, mode | 0o644);
        put16(inode, 26, 1);
        put32(inode, 4, size);
        put32(inode, 32, 0x0008_0000);
        put16(inode, 40, 0xf30a);
        put16(inode, 42, 1);
        put16(inode, 44, 4);
        put16(inode, 46, 0);
        write_extent(inode, 52, 0, extent_len, physical);
    }

    fn write_extent(bytes: &mut [u8], at: usize, logical: u32, blocks: u16, physical: u32) {
        put32(bytes, at, logical);
        put16(bytes, at + 4, blocks);
        put16(bytes, at + 6, 0);
        put32(bytes, at + 8, physical);
    }

    fn write_directory(block: &mut [u8], entries: &[(u32, &[u8], u8)]) {
        let mut at = 0;
        for (index, (ino, name, file_type)) in entries.iter().enumerate() {
            let rec_len = if index + 1 == entries.len() {
                block.len() - at
            } else {
                (8 + name.len() + 3) & !3
            };
            put32(block, at, *ino);
            put16(block, at + 4, rec_len as u16);
            block[at + 6] = name.len() as u8;
            block[at + 7] = *file_type;
            block[at + 8..at + 8 + name.len()].copy_from_slice(name);
            at += rec_len;
        }
        assert_eq!(at, block.len());
    }

    fn group_checksum(uuid: &[u8; 16], group: u32, descriptor: &[u8; 32]) -> u16 {
        let mut crc = crc16(!0, uuid);
        crc = crc16(crc, &group.to_le_bytes());
        crc = crc16(crc, &descriptor[..30]);
        crc16(crc, &descriptor[32..])
    }

    fn crc16(mut crc: u16, bytes: &[u8]) -> u16 {
        for byte in bytes {
            crc ^= u16::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xa001
                } else {
                    crc >> 1
                };
            }
        }
        crc
    }

    fn put16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn write_geometry(disk: &mut [u8]) {
    let mut geometry = [0u8; 52];
    put32(&mut geometry, 0, 0x616c_4467);
    put32(&mut geometry, 4, 52);
    put32(&mut geometry, 40, META_MAX as u32);
    put32(&mut geometry, 44, 3);
    put32(&mut geometry, 48, 4096);
    let checksum = sunfish_lpd::sha256(&geometry);
    geometry[8..40].copy_from_slice(&checksum);
    disk[4096..4096 + 52].copy_from_slice(&geometry);
    disk[8192..8192 + 52].copy_from_slice(&geometry);
}

fn write_metadata(disk: &mut [u8], at: usize, tables: &[u8]) {
    let mut header = [0u8; 128];
    put32(&mut header, 0, 0x414c_5030);
    put16(&mut header, 4, 10);
    put16(&mut header, 6, 1);
    put32(&mut header, 8, 128);
    put32(&mut header, 44, tables.len() as u32);
    let table_hash = sunfish_lpd::sha256(tables);
    header[48..80].copy_from_slice(&table_hash);
    write_table(&mut header, 80, 0, 2, 52);
    write_table(&mut header, 92, 104, 4, 24);
    write_table(&mut header, 104, 200, 2, 48);
    write_table(&mut header, 116, 296, 1, 64);
    let header_hash = sunfish_lpd::sha256(&header);
    header[12..44].copy_from_slice(&header_hash);
    disk[at..at + 128].copy_from_slice(&header);
    disk[at + 128..at + 128 + tables.len()].copy_from_slice(tables);
}

fn rewrite_table_and_header_hash(disk: &mut [u8], at: usize) {
    let table_hash = sunfish_lpd::sha256(&disk[at + 128..at + 128 + TABLES_BYTES]);
    disk[at + 48..at + 80].copy_from_slice(&table_hash);
    disk[at + 12..at + 44].fill(0);
    let header_hash = sunfish_lpd::sha256(&disk[at..at + 128]);
    disk[at + 12..at + 44].copy_from_slice(&header_hash);
}

fn write_table(header: &mut [u8], at: usize, offset: u32, count: u32, entry_size: u32) {
    put32(header, at, offset);
    put32(header, at + 4, count);
    put32(header, at + 8, entry_size);
}

fn write_extent(bytes: &mut [u8], at: usize, sectors: u64, kind: u32, target: u64, source: u32) {
    put64(bytes, at, sectors);
    put32(bytes, at + 8, kind);
    put64(bytes, at + 12, target);
    put32(bytes, at + 20, source);
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn sha256_matches_standard_vectors_and_streaming_chunks() {
    assert_eq!(
        sunfish_lpd::sha256(b"abc"),
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ]
    );
    let all_a = [b'a'; 1000];
    assert_eq!(
        sunfish_lpd::sha256(&all_a),
        [
            0x41, 0xed, 0xec, 0xe4, 0x2d, 0x63, 0xe8, 0xd9, 0xbf, 0x51, 0x5a, 0x9b, 0xa6, 0x93,
            0x2e, 0x1c, 0x20, 0xcb, 0xc9, 0xf5, 0xa5, 0xd1, 0x34, 0x64, 0x5a, 0xdb, 0x5d, 0xb1,
            0xb9, 0x73, 0x7e, 0xa3,
        ]
    );
    let mut streamed = sunfish_lpd::Sha256::new();
    for chunk in all_a.chunks(37) {
        streamed.update(chunk);
    }
    assert_eq!(streamed.finish(), sunfish_lpd::sha256(&all_a));
}

#[test]
fn opens_and_maps_active_vendor_partition_read_only() {
    let vendor = VendorPartition::open(source(image()), 0).unwrap();
    assert_eq!(vendor.partition_len(), 64 * SECTOR as u64);
    let source = ReadSourceCell::new(vendor).unwrap();
    let mut bytes = vec![0xff; 64 * SECTOR];
    source.read_exact_at(0, &mut bytes).unwrap();
    assert!(bytes[..24 * SECTOR].iter().all(|byte| *byte == b'A'));
    assert!(
        bytes[24 * SECTOR..40 * SECTOR]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert!(bytes[40 * SECTOR..].iter().all(|byte| *byte == b'C'));
    assert_eq!(
        source.read_exact_at((64 * SECTOR) as u64, &mut [0; 1]),
        Err(())
    );
}

#[test]
fn resolves_slot_suffixed_vendor_name_for_active_slot() {
    let mut disk = image();
    for at in [PRIMARY_META, BACKUP_META] {
        let partition = at + 128;
        disk[partition..partition + 36].fill(0);
        disk[partition..partition + 7].copy_from_slice(b"vendor\0");
        put32(&mut disk, partition + 36, 1 | 2 | 4);
        rewrite_table_and_header_hash(&mut disk, at);
    }
    let vendor = VendorPartition::open(source(disk), 0).unwrap();
    assert_eq!(vendor.partition_len(), 64 * SECTOR as u64);
}

#[test]
fn rejects_bad_geometry_checksum_even_if_backup_is_bad() {
    let mut disk = image();
    disk[4096 + 8] ^= 1;
    disk[8192 + 8] ^= 1;
    assert_eq!(
        VendorPartition::open(source(disk), 0).err(),
        Some(Error::GeometryChecksum)
    );
}

#[test]
fn uses_backup_metadata_when_primary_copy_is_corrupt() {
    let mut disk = image();
    disk[PRIMARY_META + 128 + 10] ^= 0x80;
    let vendor = VendorPartition::open(source(disk), 0).unwrap();
    assert_eq!(vendor.partition_len(), 64 * SECTOR as u64);
}

#[test]
fn rejects_table_checksum_mismatch_in_both_metadata_copies() {
    let mut disk = image();
    disk[PRIMARY_META + 128 + 52] ^= 1;
    disk[BACKUP_META + 128 + 52] ^= 1;
    assert_eq!(
        VendorPartition::open(source(disk), 0).err(),
        Some(Error::MetadataChecksum)
    );
}

#[test]
fn rejects_extent_past_super_end_after_valid_rehash() {
    let mut disk = image();
    for at in [PRIMARY_META, BACKUP_META] {
        let extent_target = at + 128 + 104 + 48 + 12;
        put64(&mut disk, extent_target, DISK_SECTORS as u64);
        rewrite_table_and_header_hash(&mut disk, at);
    }
    assert_eq!(
        VendorPartition::open(source(disk), 0).err(),
        Some(Error::Extent)
    );
}

#[test]
fn rejects_wrong_slot_and_fails_closed_on_length_drift() {
    assert_eq!(
        VendorPartition::open(source(image()), 2).err(),
        Some(Error::Slot)
    );
    let source = source(image());
    let visible_len = source.visible_len.clone();
    let mut vendor = VendorPartition::open(source, 0).unwrap();
    visible_len.set((DISK_BYTES - 1) as u64);
    assert_eq!(vendor.len(), Err(()));
    assert_eq!(vendor.read_exact_at(0, &mut [0; 1]), Err(()));
}

#[test]
fn composes_super_slot_and_vendor_ext4_into_an_exact_lazy_wlan_file_source() {
    let ext4 = ext4_fixture::image();
    let super_source = source(super_image_with_vendor(&ext4));
    let reads = super_source.reads.clone();

    let file = SunfishVendorWlanSource::open(super_source, 0).unwrap();
    assert_eq!(file.file_len(), ext4_fixture::FILE_BYTES as u64);
    let metadata_reads = reads.borrow().clone();
    let vendor_base = 1024 * SECTOR as u64;
    let payload_ranges = [
        (vendor_base + 21 * 4096, vendor_base + 22 * 4096),
        (vendor_base + 23 * 4096, vendor_base + 24 * 4096),
    ];
    assert!(metadata_reads.iter().all(|(offset, len)| {
        let end = *offset + *len as u64;
        payload_ranges
            .iter()
            .all(|(start, stop)| end <= *start || *offset >= *stop)
    }));

    let verified = file
        .verify_identity(
            ext4_fixture::FILE_BYTES as u64,
            [
                0x89, 0x7a, 0x7d, 0x30, 0x28, 0x69, 0x17, 0xcf, 0xbd, 0x1b, 0xda, 0xd5, 0x6c, 0x3c,
                0xdf, 0xc5, 0xdf, 0x1b, 0x57, 0x7c, 0xc6, 0x1b, 0xaf, 0x6c, 0x31, 0x05, 0x15, 0xf8,
                0xc6, 0x5b, 0x82, 0x56,
            ],
        )
        .unwrap();
    assert_eq!(verified.file_len(), ext4_fixture::FILE_BYTES as u64);
    assert_eq!(verified.sha256()[..4], [0x89, 0x7a, 0x7d, 0x30]);

    let file = ReadSourceCell::new(verified).unwrap();
    let mut payload = vec![0xff; ext4_fixture::FILE_BYTES];
    file.read_exact_at(0, &mut payload).unwrap();
    assert!(payload[..4096].iter().all(|byte| *byte == 0x41));
    assert!(payload[4096..8192].iter().all(|byte| *byte == 0));
    assert!(payload[8192..].iter().all(|byte| *byte == 0x43));
    assert_eq!(
        file.read_exact_at(ext4_fixture::FILE_BYTES as u64, &mut [0; 1]),
        Err(())
    );

    assert!(matches!(
        SunfishVendorWlanSource::open(source(image()), 4),
        Err(SourceError::LogicalPartition(sunfish_lpd::Error::Slot))
    ));
}

#[test]
fn vendor_image_identity_verification_rejects_length_and_digest_mismatches() {
    let file =
        SunfishVendorWlanSource::open(source(super_image_with_vendor(&ext4_fixture::image())), 0)
            .unwrap();
    assert_eq!(
        file.verify_identity(1, [0; 32]).err(),
        Some(SourceError::IdentityLength)
    );

    let file =
        SunfishVendorWlanSource::open(source(super_image_with_vendor(&ext4_fixture::image())), 0)
            .unwrap();
    assert_eq!(
        file.verify_phone_observed_identity().err(),
        Some(SourceError::IdentityLength)
    );

    let file =
        SunfishVendorWlanSource::open(source(super_image_with_vendor(&ext4_fixture::image())), 0)
            .unwrap();
    assert_eq!(
        file.verify_identity(ext4_fixture::FILE_BYTES as u64, [0; 32])
            .err(),
        Some(SourceError::IdentityDigest)
    );
}

#[test]
fn composes_super_slot_and_vendor_ext4_into_lazy_exact_board_data_source() {
    let ext4 = ext4_fixture::image();
    let super_source = source(super_image_with_vendor(&ext4));
    let reads = super_source.reads.clone();

    let file = SunfishVendorBoardSource::open(super_source, 0).unwrap();
    assert_eq!(file.file_len(), ext4_fixture::BOARD_DATA.len() as u64);
    let vendor_base = 1024 * SECTOR as u64;
    let board_payload = (vendor_base + 24 * 4096, vendor_base + 25 * 4096);
    assert!(reads.borrow().iter().all(|(offset, len)| {
        let end = *offset + *len as u64;
        end <= board_payload.0 || *offset >= board_payload.1
    }));

    let verified = file
        .verify_identity(
            ext4_fixture::BOARD_DATA.len() as u64,
            [
                0x1c, 0x0d, 0x6b, 0x89, 0xad, 0x83, 0xa6, 0x1f, 0x57, 0x08, 0x5c, 0xa3, 0x82, 0x73,
                0x6b, 0x1f, 0x81, 0x37, 0x20, 0x85, 0x53, 0x3d, 0x0e, 0xe0, 0x98, 0x39, 0x5b, 0xbe,
                0x07, 0x33, 0xda, 0x9a,
            ],
        )
        .unwrap();
    assert_eq!(verified.file_len(), ext4_fixture::BOARD_DATA.len() as u64);
    assert_eq!(verified.sha256()[..4], [0x1c, 0x0d, 0x6b, 0x89]);

    let file = ReadSourceCell::new(verified).unwrap();
    let mut payload = vec![0; ext4_fixture::BOARD_DATA.len()];
    file.read_exact_at(0, &mut payload).unwrap();
    assert_eq!(payload, ext4_fixture::BOARD_DATA);
    assert_eq!(
        file.read_exact_at(ext4_fixture::BOARD_DATA.len() as u64, &mut [0; 1]),
        Err(())
    );
}

#[test]
#[ignore = "requires a bounded metadata-only prefix captured from a live Sunfish super partition"]
fn parses_live_super_metadata_prefix() {
    let path = std::env::var("SUNFISH_SUPER_METADATA_PREFIX").unwrap();
    let source_bytes = std::env::var("SUNFISH_SUPER_BYTES")
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let file = File::open(path).unwrap();
    let captured_bytes = file.metadata().unwrap().len();
    assert!(captured_bytes >= (BACKUP_META + META_MAX) as u64);
    assert_eq!(captured_bytes, LIVE_METADATA_PREFIX_BYTES);
    assert!(captured_bytes < source_bytes);
    let source = PrefixFileIo {
        file,
        source_bytes,
        captured_bytes,
    };
    let vendor = VendorPartition::open(source, 0).unwrap();
    assert_eq!(vendor.partition_len(), 1_344_728 * SECTOR as u64);
}
