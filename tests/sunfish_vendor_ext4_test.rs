#![forbid(unsafe_code)]
#![allow(dead_code)]

#[path = "../rust/src/qcom_tftp.rs"]
mod qcom_tftp;
#[path = "../rust/src/sunfish_vendor_ext4.rs"]
mod sunfish_vendor_ext4;

use qcom_tftp::{ReadSource, ReadSourceCell, ReadSourceIo};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use sunfish_vendor_ext4::{Error, VendorWlanFirmware};

const BLOCK: usize = 4096;
const BLOCKS: usize = 64;
const FILE_BYTES: usize = BLOCK * 2 + 17;

#[derive(Clone)]
struct MemoryIo {
    bytes: Rc<RefCell<Vec<u8>>>,
    reads: Rc<RefCell<Vec<(u64, usize)>>>,
    visible_len: Rc<Cell<u64>>,
}

impl ReadSourceIo for MemoryIo {
    fn len(&mut self) -> Result<u64, ()> {
        Ok(self.visible_len.get())
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        let start = usize::try_from(offset).map_err(|_| ())?;
        let end = start.checked_add(out.len()).ok_or(())?;
        let bytes = self.bytes.borrow();
        out.copy_from_slice(bytes.get(start..end).ok_or(())?);
        self.reads.borrow_mut().push((offset, out.len()));
        Ok(())
    }
}

fn image() -> Vec<u8> {
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
    // File inode 4 has a single depth-1 extent index pointing at block 20.
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
        &[(3, b".", 2), (2, b"..", 2), (4, b"wlanmdsp.mbn", 1)],
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
    disk
}

fn source(bytes: Vec<u8>) -> MemoryIo {
    let len = bytes.len() as u64;
    MemoryIo {
        bytes: Rc::new(RefCell::new(bytes)),
        reads: Rc::new(RefCell::new(Vec::new())),
        visible_len: Rc::new(Cell::new(len)),
    }
}

fn write_inode(disk: &mut [u8], ino: usize, mode: u16, size: u32, extent_len: u16, physical: u32) {
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

#[test]
fn opens_exact_file_without_reading_its_payload_and_maps_sparse_blocks() {
    let source = source(image());
    let reads = source.reads.clone();
    let file = VendorWlanFirmware::open(source).unwrap();
    assert_eq!(file.file_len(), FILE_BYTES as u64);
    let source = ReadSourceCell::new(file).unwrap();
    assert_eq!(source.len(), Ok(FILE_BYTES as u64));
    assert!(
        reads
            .borrow()
            .iter()
            .all(|(offset, _)| *offset < 21 * BLOCK as u64 || *offset >= 22 * BLOCK as u64)
    );

    let mut bytes = vec![0xff; FILE_BYTES];
    source.read_exact_at(0, &mut bytes).unwrap();
    assert!(bytes[..BLOCK].iter().all(|byte| *byte == 0x41));
    assert!(bytes[BLOCK..BLOCK * 2].iter().all(|byte| *byte == 0));
    assert!(bytes[BLOCK * 2..].iter().all(|byte| *byte == 0x43));
    assert_eq!(
        source.read_exact_at(FILE_BYTES as u64, &mut [0u8; 1]),
        Err(())
    );
}

#[test]
fn rejects_bad_group_descriptor_checksum() {
    let mut disk = image();
    disk[BLOCK + 8] ^= 1;
    assert_eq!(
        VendorWlanFirmware::open(source(disk)).err(),
        Some(Error::GroupChecksum)
    );
}

#[test]
fn rejects_unsupported_feature_profile() {
    let mut disk = image();
    put32(&mut disk[1024..2048], 96, 0x42 | 0x80);
    assert_eq!(
        VendorWlanFirmware::open(source(disk)).err(),
        Some(Error::UnsupportedFeatures)
    );
}

#[test]
fn rejects_malformed_directory_record() {
    let mut disk = image();
    put16(&mut disk[18 * BLOCK..19 * BLOCK], 4, 0);
    assert_eq!(
        VendorWlanFirmware::open(source(disk)).err(),
        Some(Error::Directory)
    );
}

#[test]
fn rejects_extent_outside_filesystem() {
    let mut disk = image();
    put32(&mut disk[20 * BLOCK..21 * BLOCK], 20, BLOCKS as u32);
    assert_eq!(
        VendorWlanFirmware::open(source(disk)).err(),
        Some(Error::ExtentTree)
    );
}

#[test]
fn fails_closed_if_partition_length_changes_after_open() {
    let source = source(image());
    let visible_len = source.visible_len.clone();
    let mut file = VendorWlanFirmware::open(source).unwrap();
    visible_len.set((BLOCK * BLOCKS - 1) as u64);
    assert_eq!(file.len(), Err(()));
    assert_eq!(file.read_exact_at(0, &mut [0u8; 1]), Err(()));
}
