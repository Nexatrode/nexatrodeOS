//! Bounded, read-only access to the observed Sunfish WLAN vendor files.
//!
//! This parser supports only the ext4 feature profile observed on the
//! LineageOS vendor partition. It verifies the superblock geometry, group
//! descriptor CRC16 values, directory records, inode types and extent trees.
//! Sparse and unwritten file blocks are returned as zeroes. It does not read
//! any file data while opening the source, authorize firmware use, identify a
//! trusted partition, or provide a storage/DMA backend.
#![forbid(unsafe_code)]

use crate::qcom_tftp::ReadSourceIo;

pub const BLOCK_BYTES: usize = 4096;
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_EXTENTS: usize = 128;

const EXT4_MAGIC: u16 = 0xef53;
const FEATURE_COMPAT: u32 = 0x0000_0028;
const FEATURE_INCOMPAT: u32 = 0x0000_0042;
const FEATURE_RO_COMPAT: u32 = 0x0000_007b;
const EXTENTS_FL: u32 = 0x0008_0000;
const EXT4_FT_REG_FILE: u8 = 1;
const EXT4_FT_DIR: u8 = 2;
const EXTENT_MAGIC: u16 = 0xf30a;
const EXTENT_HEADER_BYTES: usize = 12;
const EXTENT_BYTES: usize = 12;
const ROOT_EXTENT_CAPACITY: usize = (60 - EXTENT_HEADER_BYTES) / EXTENT_BYTES;
const GROUP_DESC_BYTES: usize = 32;
const GROUP_DESC_CHECKSUM_OFFSET: usize = 30;
const MAX_GROUP_DESCRIPTORS: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Source,
    Geometry,
    UnsupportedFeatures,
    Superblock,
    GroupDescriptor,
    GroupChecksum,
    Inode,
    InodeType,
    Directory,
    DuplicateName,
    ExtentTree,
    FileSize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Extent {
    logical: u32,
    blocks: u32,
    physical: u32,
    initialized: bool,
}

#[derive(Clone, Copy)]
struct Geometry {
    partition_bytes: u64,
    filesystem_bytes: u64,
    blocks: u32,
    inodes: u32,
    blocks_per_group: u32,
    inodes_per_group: u32,
    groups: u32,
    uuid: [u8; 16],
}

/// An open exact-path ext4 file backed by caller-provided read-only I/O.
///
/// The constructor reads metadata only. Reads later expose only the file data
/// extents discovered for one of the two exact WLAN file paths.
pub struct VendorWlanFirmware<S: ReadSourceIo> {
    source: S,
    geometry: Geometry,
    file_bytes: u64,
    extents: [Extent; MAX_EXTENTS],
    extent_count: usize,
}

impl<S: ReadSourceIo> VendorWlanFirmware<S> {
    /// Open `/vendor/firmware/wlanmdsp.mbn`.
    pub fn open(source: S) -> Result<Self, Error> {
        Self::open_named(source, b"wlanmdsp.mbn")
    }

    /// Open `/vendor/firmware/bdwlan-sunfish.bin` board data.
    pub fn open_board_data(source: S) -> Result<Self, Error> {
        Self::open_named(source, b"bdwlan-sunfish.bin")
    }

    fn open_named(mut source: S, file_name: &[u8]) -> Result<Self, Error> {
        let partition_bytes = source.len().map_err(|_| Error::Source)?;
        if partition_bytes < (BLOCK_BYTES * 2) as u64 {
            return Err(Error::Geometry);
        }

        let mut superblock = [0u8; 1024];
        checked_read(&mut source, partition_bytes, 1024, &mut superblock)?;
        let geometry = parse_superblock(&superblock, partition_bytes)?;

        let root_inode = read_inode(&mut source, geometry, 2)?;
        parse_inode(&root_inode, InodeKind::Directory)?;
        let firmware_ino =
            find_directory_entry(&mut source, geometry, &root_inode, b"firmware", EXT4_FT_DIR)?;

        let firmware_inode = read_inode(&mut source, geometry, firmware_ino)?;
        parse_inode(&firmware_inode, InodeKind::Directory)?;
        let file_ino = find_directory_entry(
            &mut source,
            geometry,
            &firmware_inode,
            file_name,
            EXT4_FT_REG_FILE,
        )?;

        let file_inode = read_inode(&mut source, geometry, file_ino)?;
        let file = parse_inode(&file_inode, InodeKind::RegularFile)?;
        if file.size == 0 || file.size > MAX_FILE_BYTES {
            return Err(Error::FileSize);
        }
        let (extents, extent_count) =
            parse_extent_tree(&mut source, geometry, &file_inode, file.size)?;

        Ok(Self {
            source,
            geometry,
            file_bytes: file.size,
            extents,
            extent_count,
        })
    }

    pub const fn file_len(&self) -> u64 {
        self.file_bytes
    }

    fn read_file(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        if self.source.len().map_err(|_| ())? != self.geometry.partition_bytes {
            return Err(());
        }
        let end = offset.checked_add(out.len() as u64).ok_or(())?;
        if end > self.file_bytes {
            return Err(());
        }

        let mut file_at = offset;
        let mut written = 0usize;
        while written < out.len() {
            let logical = u32::try_from(file_at / BLOCK_BYTES as u64).map_err(|_| ())?;
            let within = (file_at % BLOCK_BYTES as u64) as usize;
            let mut available = BLOCK_BYTES - within;
            let mapping = self.extent_for(logical);
            if let Some(extent) = mapping {
                let extent_end = extent.logical.checked_add(extent.blocks).ok_or(())?;
                let logical_left = usize::try_from(extent_end - logical).map_err(|_| ())?;
                available = available.min(
                    logical_left
                        .checked_mul(BLOCK_BYTES)
                        .and_then(|n| n.checked_sub(within))
                        .ok_or(())?,
                );
            } else if let Some(next) = self.next_extent_start(logical) {
                let blocks_until = usize::try_from(next - logical).map_err(|_| ())?;
                available = available.min(
                    blocks_until
                        .checked_mul(BLOCK_BYTES)
                        .and_then(|n| n.checked_sub(within))
                        .ok_or(())?,
                );
            }
            let take = available.min(out.len() - written);
            if take == 0 {
                return Err(());
            }

            if let Some(extent) = mapping.filter(|extent| extent.initialized) {
                let physical_block = extent
                    .physical
                    .checked_add(logical - extent.logical)
                    .ok_or(())?;
                let physical_offset = u64::from(physical_block)
                    .checked_mul(BLOCK_BYTES as u64)
                    .and_then(|n| n.checked_add(within as u64))
                    .ok_or(())?;
                checked_read(
                    &mut self.source,
                    self.geometry.partition_bytes,
                    physical_offset,
                    &mut out[written..written + take],
                )
                .map_err(|_| ())?;
            } else {
                out[written..written + take].fill(0);
            }
            written += take;
            file_at = file_at.checked_add(take as u64).ok_or(())?;
        }
        if self.source.len().map_err(|_| ())? != self.geometry.partition_bytes {
            return Err(());
        }
        Ok(())
    }

    fn extent_for(&self, logical: u32) -> Option<Extent> {
        self.extents[..self.extent_count]
            .iter()
            .copied()
            .find(|extent| logical >= extent.logical && logical < extent.logical + extent.blocks)
    }

    fn next_extent_start(&self, logical: u32) -> Option<u32> {
        self.extents[..self.extent_count]
            .iter()
            .map(|extent| extent.logical)
            .filter(|start| *start > logical)
            .min()
    }
}

impl<S: ReadSourceIo> ReadSourceIo for VendorWlanFirmware<S> {
    fn len(&mut self) -> Result<u64, ()> {
        if self.source.len().map_err(|_| ())? != self.geometry.partition_bytes {
            return Err(());
        }
        Ok(self.file_bytes)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        self.read_file(offset, out)
    }
}

#[derive(Clone, Copy)]
enum InodeKind {
    Directory,
    RegularFile,
}

#[derive(Clone, Copy)]
struct InodeInfo {
    size: u64,
}

fn parse_superblock(superblock: &[u8; 1024], partition_bytes: u64) -> Result<Geometry, Error> {
    if le16(superblock, 56) != EXT4_MAGIC {
        return Err(Error::Superblock);
    }
    if le32(superblock, 24) != 2 || le32(superblock, 28) != 2 {
        return Err(Error::Geometry);
    }
    if le32(superblock, 20) != 0 || le16(superblock, 88) != 256 || le32(superblock, 0x150) != 0 {
        return Err(Error::Geometry);
    }
    if le32(superblock, 92) != FEATURE_COMPAT
        || le32(superblock, 96) != FEATURE_INCOMPAT
        || le32(superblock, 100) != FEATURE_RO_COMPAT
    {
        return Err(Error::UnsupportedFeatures);
    }
    let desc_size = le16(superblock, 254);
    if desc_size != 0 && usize::from(desc_size) != GROUP_DESC_BYTES {
        return Err(Error::Geometry);
    }
    let blocks = le32(superblock, 4);
    let inodes = le32(superblock, 0);
    let blocks_per_group = le32(superblock, 32);
    let inodes_per_group = le32(superblock, 40);
    if blocks == 0
        || inodes < 2
        || blocks_per_group == 0
        || blocks_per_group > (BLOCK_BYTES * 8) as u32
        || inodes_per_group == 0
        || le32(superblock, 36) != blocks_per_group
    {
        return Err(Error::Geometry);
    }
    let filesystem_bytes = u64::from(blocks)
        .checked_mul(BLOCK_BYTES as u64)
        .ok_or(Error::Geometry)?;
    if filesystem_bytes > partition_bytes {
        return Err(Error::Geometry);
    }
    let groups = blocks
        .checked_add(blocks_per_group - 1)
        .ok_or(Error::Geometry)?
        / blocks_per_group;
    let inode_groups = inodes
        .checked_add(inodes_per_group - 1)
        .ok_or(Error::Geometry)?
        / inodes_per_group;
    if groups == 0 || u64::from(groups) > MAX_GROUP_DESCRIPTORS || inode_groups > groups {
        return Err(Error::Geometry);
    }
    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(&superblock[104..120]);
    Ok(Geometry {
        partition_bytes,
        filesystem_bytes,
        blocks,
        inodes,
        blocks_per_group,
        inodes_per_group,
        groups,
        uuid,
    })
}

fn read_inode<S: ReadSourceIo>(
    source: &mut S,
    geometry: Geometry,
    ino: u32,
) -> Result<[u8; 256], Error> {
    if ino == 0 || ino > geometry.inodes {
        return Err(Error::Inode);
    }
    let zero_based = ino - 1;
    let group = zero_based / geometry.inodes_per_group;
    let index = zero_based % geometry.inodes_per_group;
    if group >= geometry.groups {
        return Err(Error::Inode);
    }
    let descriptor = read_group_descriptor(source, geometry, group)?;
    let table_block = le32(&descriptor, 8);
    let table_blocks_bytes = u64::from(geometry.inodes_per_group)
        .checked_mul(256)
        .ok_or(Error::Geometry)?;
    let table_blocks = table_blocks_bytes
        .checked_add(BLOCK_BYTES as u64 - 1)
        .ok_or(Error::Geometry)?
        / BLOCK_BYTES as u64;
    let group_start = u64::from(group)
        .checked_mul(u64::from(geometry.blocks_per_group))
        .ok_or(Error::Geometry)?;
    let group_end = group_start
        .checked_add(u64::from(geometry.blocks_per_group))
        .ok_or(Error::Geometry)?
        .min(u64::from(geometry.blocks));
    let table_end = u64::from(table_block)
        .checked_add(table_blocks)
        .ok_or(Error::Geometry)?;
    if u64::from(table_block) < group_start || table_end > group_end {
        return Err(Error::GroupDescriptor);
    }
    let table_offset = u64::from(table_block)
        .checked_mul(BLOCK_BYTES as u64)
        .ok_or(Error::Geometry)?;
    let inode_offset = table_offset
        .checked_add(u64::from(index).checked_mul(256).ok_or(Error::Geometry)?)
        .ok_or(Error::Geometry)?;
    if inode_offset.checked_add(256).ok_or(Error::Geometry)? > geometry.filesystem_bytes {
        return Err(Error::Geometry);
    }
    let mut inode = [0u8; 256];
    checked_read(source, geometry.partition_bytes, inode_offset, &mut inode)?;
    Ok(inode)
}

fn read_group_descriptor<S: ReadSourceIo>(
    source: &mut S,
    geometry: Geometry,
    group: u32,
) -> Result<[u8; GROUP_DESC_BYTES], Error> {
    if group >= geometry.groups {
        return Err(Error::GroupDescriptor);
    }
    let gdt_start_block = 1u64; // 4 KiB profile: superblock is in block 0.
    let offset = gdt_start_block
        .checked_mul(BLOCK_BYTES as u64)
        .and_then(|start| start.checked_add(u64::from(group) * GROUP_DESC_BYTES as u64))
        .ok_or(Error::Geometry)?;
    if offset
        .checked_add(GROUP_DESC_BYTES as u64)
        .ok_or(Error::Geometry)?
        > geometry.filesystem_bytes
    {
        return Err(Error::Geometry);
    }
    let mut descriptor = [0u8; GROUP_DESC_BYTES];
    checked_read(source, geometry.partition_bytes, offset, &mut descriptor)?;
    let stored = le16(&descriptor, GROUP_DESC_CHECKSUM_OFFSET);
    let calculated = group_checksum(&geometry.uuid, group, &descriptor);
    if stored != calculated {
        return Err(Error::GroupChecksum);
    }
    Ok(descriptor)
}

fn group_checksum(uuid: &[u8; 16], group: u32, descriptor: &[u8; GROUP_DESC_BYTES]) -> u16 {
    let mut crc = crc16(!0, uuid);
    crc = crc16(crc, &group.to_le_bytes());
    crc = crc16(crc, &descriptor[..GROUP_DESC_CHECKSUM_OFFSET]);
    crc16(
        crc,
        &descriptor[GROUP_DESC_CHECKSUM_OFFSET + 2..GROUP_DESC_BYTES],
    )
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

fn parse_inode(inode: &[u8; 256], kind: InodeKind) -> Result<InodeInfo, Error> {
    let mode = le16(inode, 0) & 0xf000;
    let expected_mode = match kind {
        InodeKind::Directory => 0x4000,
        InodeKind::RegularFile => 0x8000,
    };
    if mode != expected_mode {
        return Err(Error::InodeType);
    }
    if le32(inode, 32) != EXTENTS_FL {
        return Err(Error::UnsupportedFeatures);
    }
    if le16(inode, 26) == 0 || le32(inode, 20) != 0 {
        return Err(Error::Inode);
    }
    let mut size = u64::from(le32(inode, 4));
    if matches!(kind, InodeKind::RegularFile) {
        size |= u64::from(le32(inode, 108)) << 32;
    }
    if matches!(kind, InodeKind::Directory) && size != BLOCK_BYTES as u64 {
        return Err(Error::Directory);
    }
    Ok(InodeInfo { size })
}

fn find_directory_entry<S: ReadSourceIo>(
    source: &mut S,
    geometry: Geometry,
    directory_inode: &[u8; 256],
    name: &[u8],
    expected_type: u8,
) -> Result<u32, Error> {
    let (extents, count) =
        parse_extent_tree(source, geometry, directory_inode, BLOCK_BYTES as u64)?;
    if count != 1 || extents[0].logical != 0 || extents[0].blocks != 1 || !extents[0].initialized {
        return Err(Error::Directory);
    }
    let offset = u64::from(extents[0].physical)
        .checked_mul(BLOCK_BYTES as u64)
        .ok_or(Error::Geometry)?;
    let mut block = [0u8; BLOCK_BYTES];
    checked_read(source, geometry.partition_bytes, offset, &mut block)?;

    let mut at = 0usize;
    let mut found = None;
    while at < BLOCK_BYTES {
        let remaining = BLOCK_BYTES - at;
        if remaining < 8 {
            return Err(Error::Directory);
        }
        let ino = le32(&block, at);
        let rec_len = usize::from(le16(&block, at + 4));
        let name_len = usize::from(block[at + 6]);
        let file_type = block[at + 7];
        if rec_len < 8 || rec_len & 3 != 0 || rec_len > remaining || name_len > rec_len - 8 {
            return Err(Error::Directory);
        }
        if ino != 0 && &block[at + 8..at + 8 + name_len] == name {
            if found.is_some() {
                return Err(Error::DuplicateName);
            }
            if file_type != expected_type || ino > geometry.inodes {
                return Err(Error::Directory);
            }
            found = Some(ino);
        }
        at += rec_len;
    }
    found.ok_or(Error::Directory)
}

fn parse_extent_tree<S: ReadSourceIo>(
    source: &mut S,
    geometry: Geometry,
    inode: &[u8; 256],
    file_size: u64,
) -> Result<([Extent; MAX_EXTENTS], usize), Error> {
    let block_count_u64 = file_size
        .checked_add(BLOCK_BYTES as u64 - 1)
        .ok_or(Error::FileSize)?
        / BLOCK_BYTES as u64;
    let block_count = u32::try_from(block_count_u64).map_err(|_| Error::FileSize)?;
    let root = &inode[40..100];
    let (entries, depth) = validate_extent_header(root, ROOT_EXTENT_CAPACITY)?;
    let mut output = [Extent {
        logical: 0,
        blocks: 0,
        physical: 0,
        initialized: false,
    }; MAX_EXTENTS];
    let mut count = 0usize;
    match depth {
        0 => parse_extent_entries(
            root,
            entries,
            block_count,
            geometry,
            &mut output,
            &mut count,
        )?,
        1 => {
            if entries != 1 {
                return Err(Error::ExtentTree);
            }
            let index = &root[EXTENT_HEADER_BYTES..EXTENT_HEADER_BYTES + EXTENT_BYTES];
            if le32(index, 0) != 0 {
                return Err(Error::ExtentTree);
            }
            let node_block = le32(index, 4);
            if le16(index, 8) != 0 || node_block == 0 || node_block >= geometry.blocks {
                return Err(Error::ExtentTree);
            }
            let mut node = [0u8; BLOCK_BYTES];
            let offset = u64::from(node_block)
                .checked_mul(BLOCK_BYTES as u64)
                .ok_or(Error::Geometry)?;
            checked_read(source, geometry.partition_bytes, offset, &mut node)?;
            let (node_entries, node_depth) =
                validate_extent_header(&node, (BLOCK_BYTES - EXTENT_HEADER_BYTES) / EXTENT_BYTES)?;
            if node_depth != 0 {
                return Err(Error::ExtentTree);
            }
            parse_extent_entries(
                &node,
                node_entries,
                block_count,
                geometry,
                &mut output,
                &mut count,
            )?;
        }
        _ => return Err(Error::ExtentTree),
    }
    Ok((output, count))
}

fn validate_extent_header(bytes: &[u8], capacity: usize) -> Result<(usize, u16), Error> {
    if bytes.len() < EXTENT_HEADER_BYTES || le16(bytes, 0) != EXTENT_MAGIC {
        return Err(Error::ExtentTree);
    }
    let entries = usize::from(le16(bytes, 2));
    let max = usize::from(le16(bytes, 4));
    let depth = le16(bytes, 6);
    if max == 0 || max > capacity || entries > max || depth > 1 {
        return Err(Error::ExtentTree);
    }
    Ok((entries, depth))
}

fn parse_extent_entries(
    bytes: &[u8],
    entries: usize,
    file_blocks: u32,
    geometry: Geometry,
    output: &mut [Extent; MAX_EXTENTS],
    count: &mut usize,
) -> Result<(), Error> {
    if entries > MAX_EXTENTS {
        return Err(Error::ExtentTree);
    }
    let mut previous_end = 0u32;
    for index in 0..entries {
        let at = EXTENT_HEADER_BYTES + index * EXTENT_BYTES;
        let raw = bytes.get(at..at + EXTENT_BYTES).ok_or(Error::ExtentTree)?;
        let logical = le32(raw, 0);
        let encoded_len = le16(raw, 4);
        let (blocks, initialized) = if encoded_len == 0x8000 {
            (0x8000u32, true)
        } else if encoded_len > 0x8000 {
            (u32::from(encoded_len - 0x8000), false)
        } else {
            (u32::from(encoded_len), true)
        };
        let physical_hi = le16(raw, 6);
        let physical = le32(raw, 8);
        let logical_end = logical.checked_add(blocks).ok_or(Error::ExtentTree)?;
        let physical_end = physical.checked_add(blocks).ok_or(Error::ExtentTree)?;
        if blocks == 0
            || physical_hi != 0
            || logical < previous_end
            || logical_end > file_blocks
            || physical == 0
            || physical_end > geometry.blocks
        {
            return Err(Error::ExtentTree);
        }
        if output[..*count].iter().any(|prior| {
            let prior_end = prior.physical.saturating_add(prior.blocks);
            physical < prior_end && prior.physical < physical_end
        }) {
            return Err(Error::ExtentTree);
        }
        let slot = output.get_mut(*count).ok_or(Error::ExtentTree)?;
        *slot = Extent {
            logical,
            blocks,
            physical,
            initialized,
        };
        *count += 1;
        previous_end = logical_end;
    }
    Ok(())
}

fn checked_read<S: ReadSourceIo>(
    source: &mut S,
    partition_bytes: u64,
    offset: u64,
    out: &mut [u8],
) -> Result<(), Error> {
    if source.len().map_err(|_| Error::Source)? != partition_bytes {
        return Err(Error::Source);
    }
    let end = offset
        .checked_add(out.len() as u64)
        .ok_or(Error::Geometry)?;
    if end > partition_bytes {
        return Err(Error::Geometry);
    }
    source
        .read_exact_at(offset, out)
        .map_err(|_| Error::Source)?;
    if source.len().map_err(|_| Error::Source)? != partition_bytes {
        return Err(Error::Source);
    }
    Ok(())
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
