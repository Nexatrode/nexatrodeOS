//! Bounded, read-only Android logical-partition metadata parsing for Sunfish.
//!
//! This accepts the observed LP metadata v10.0/v10.1 format, verifies the
//! geometry, header, and table SHA-256 digests, and exposes only the active
//! `vendor_a`/`vendor_b` partition as a read-only byte source. The parser does
//! not open block devices, activate device-mapper targets, read partition
//! payloads while opening, or authorize firmware use.
#![forbid(unsafe_code)]

use crate::qcom_tftp::ReadSourceIo;

pub const SECTOR_BYTES: u64 = 512;
pub const MAX_METADATA_BYTES: usize = 64 * 1024;
pub const MAX_PARTITION_EXTENTS: usize = 64;

const GEOMETRY_MAGIC: u32 = 0x616c_4467;
const GEOMETRY_BYTES: usize = 52;
const GEOMETRY_RESERVED_BYTES: u64 = 4096;
const GEOMETRY_COPY_BYTES: u64 = 4096;
const GEOMETRY_PRIMARY_OFFSET: u64 = GEOMETRY_RESERVED_BYTES;
const GEOMETRY_BACKUP_OFFSET: u64 = GEOMETRY_PRIMARY_OFFSET + GEOMETRY_COPY_BYTES;
const METADATA_MAGIC: u32 = 0x414c_5030;
const METADATA_MAJOR: u16 = 10;
const METADATA_MAX_MINOR: u16 = 1;
const METADATA_HEADER_BYTES: usize = 128;
const PARTITION_ENTRY_BYTES: usize = 52;
const EXTENT_ENTRY_BYTES: usize = 24;
const GROUP_ENTRY_BYTES: usize = 48;
const BLOCK_DEVICE_ENTRY_BYTES: usize = 64;
const MAX_PARTITIONS: u32 = 512;
const MAX_EXTENTS: u32 = 4096;
const MAX_GROUPS: u32 = 64;
const MAX_BLOCK_DEVICES: u32 = 8;
const ATTR_READONLY: u32 = 1;
const ATTR_SLOT_SUFFIXED: u32 = 2;
const ATTR_UPDATED: u32 = 4;
const ATTR_DISABLED: u32 = 8;
const ATTR_V0_MASK: u32 = ATTR_READONLY | ATTR_SLOT_SUFFIXED;
const ATTR_V1_MASK: u32 = ATTR_UPDATED | ATTR_DISABLED;
const TARGET_LINEAR: u32 = 0;
const TARGET_ZERO: u32 = 1;
const BLOCK_DEVICE_SLOT_SUFFIXED: u32 = 1;
const GROUP_SLOT_SUFFIXED: u32 = 1;
const TABLES_HASH_CHUNK: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Source,
    Geometry,
    GeometryChecksum,
    MetadataHeader,
    UnsupportedVersion,
    MetadataChecksum,
    TableBounds,
    UnsupportedTable,
    BlockDevice,
    Partition,
    Extent,
    Slot,
    VendorMissing,
    VendorDuplicate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Geometry {
    metadata_max_size: u32,
    metadata_slot_count: u32,
    logical_block_size: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Table {
    offset: u32,
    count: u32,
    entry_size: u32,
}

impl Table {
    fn byte_len(self) -> Result<u32, Error> {
        self.count
            .checked_mul(self.entry_size)
            .ok_or(Error::TableBounds)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockDevice {
    first_logical_sector: u64,
    size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Extent {
    logical_sector: u64,
    sector_count: u64,
    target_sector: u64,
    zero: bool,
}

/// A verified read-only view of the active Sunfish vendor logical partition.
pub struct VendorPartition<S: ReadSourceIo> {
    source: S,
    source_bytes: u64,
    logical_bytes: u64,
    extents: [Extent; MAX_PARTITION_EXTENTS],
    extent_count: usize,
}

impl<S: ReadSourceIo> VendorPartition<S> {
    /// Parse the active slot's `vendor_a` or `vendor_b` mapping from `super`.
    ///
    /// `slot_number` is 0 for `_a` and 1 for `_b`, matching AOSP liblp.
    pub fn open(mut source: S, slot_number: u32) -> Result<Self, Error> {
        let target_name = match slot_number {
            0 => b"vendor_a".as_slice(),
            1 => b"vendor_b".as_slice(),
            _ => return Err(Error::Slot),
        };
        let source_bytes = source.len().map_err(|_| Error::Source)?;
        if source_bytes < GEOMETRY_BACKUP_OFFSET + GEOMETRY_BYTES as u64 {
            return Err(Error::Geometry);
        }

        let geometry = read_geometry(&mut source, source_bytes)?;
        if slot_number >= geometry.metadata_slot_count {
            return Err(Error::Slot);
        }

        let primary_offset = metadata_offset(&geometry, slot_number, false)?;
        match parse_metadata(
            &mut source,
            source_bytes,
            geometry,
            slot_number,
            target_name,
            primary_offset,
        ) {
            Ok((extents, extent_count, logical_bytes)) => Ok(Self {
                source,
                source_bytes,
                logical_bytes,
                extents,
                extent_count,
            }),
            Err(Error::Source) => Err(Error::Source),
            Err(primary_error) => {
                let backup_offset = metadata_offset(&geometry, slot_number, true)?;
                match parse_metadata(
                    &mut source,
                    source_bytes,
                    geometry,
                    slot_number,
                    target_name,
                    backup_offset,
                ) {
                    Ok((extents, extent_count, logical_bytes)) => Ok(Self {
                        source,
                        source_bytes,
                        logical_bytes,
                        extents,
                        extent_count,
                    }),
                    Err(Error::Source) => Err(Error::Source),
                    Err(_) => Err(primary_error),
                }
            }
        }
    }

    pub const fn partition_len(&self) -> u64 {
        self.logical_bytes
    }

    fn read_partition(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        if self.source.len().map_err(|_| ())? != self.source_bytes {
            return Err(());
        }
        let end = offset.checked_add(out.len() as u64).ok_or(())?;
        if end > self.logical_bytes {
            return Err(());
        }

        let mut logical_at = offset;
        let mut written = 0usize;
        while written < out.len() {
            let extent = self.extents[..self.extent_count]
                .iter()
                .copied()
                .find(|extent| {
                    let start = extent.logical_sector * SECTOR_BYTES;
                    let length = extent.sector_count * SECTOR_BYTES;
                    logical_at >= start && logical_at < start + length
                })
                .ok_or(())?;
            let extent_start = extent.logical_sector.checked_mul(SECTOR_BYTES).ok_or(())?;
            let extent_bytes = extent.sector_count.checked_mul(SECTOR_BYTES).ok_or(())?;
            let within = logical_at.checked_sub(extent_start).ok_or(())?;
            let take = usize::try_from((extent_bytes - within).min((out.len() - written) as u64))
                .map_err(|_| ())?;
            if take == 0 {
                return Err(());
            }
            if extent.zero {
                out[written..written + take].fill(0);
            } else {
                let physical_offset = extent
                    .target_sector
                    .checked_mul(SECTOR_BYTES)
                    .and_then(|value| value.checked_add(within))
                    .ok_or(())?;
                checked_read(
                    &mut self.source,
                    self.source_bytes,
                    physical_offset,
                    &mut out[written..written + take],
                )
                .map_err(|_| ())?;
            }
            logical_at = logical_at.checked_add(take as u64).ok_or(())?;
            written += take;
        }
        if self.source.len().map_err(|_| ())? != self.source_bytes {
            return Err(());
        }
        Ok(())
    }
}

impl<S: ReadSourceIo> ReadSourceIo for VendorPartition<S> {
    fn len(&mut self) -> Result<u64, ()> {
        if self.source.len().map_err(|_| ())? != self.source_bytes {
            return Err(());
        }
        Ok(self.logical_bytes)
    }

    fn read_exact_at(&mut self, offset: u64, out: &mut [u8]) -> Result<(), ()> {
        self.read_partition(offset, out)
    }
}

fn read_geometry<S: ReadSourceIo>(source: &mut S, source_bytes: u64) -> Result<Geometry, Error> {
    let mut primary = [0u8; GEOMETRY_BYTES];
    checked_read(source, source_bytes, GEOMETRY_PRIMARY_OFFSET, &mut primary)?;
    match parse_geometry(&primary) {
        Ok(geometry) => return Ok(geometry),
        Err(primary_error) => {
            let mut backup = [0u8; GEOMETRY_BYTES];
            checked_read(source, source_bytes, GEOMETRY_BACKUP_OFFSET, &mut backup)?;
            parse_geometry(&backup).map_err(|_| primary_error)
        }
    }
}

fn parse_geometry(raw: &[u8; GEOMETRY_BYTES]) -> Result<Geometry, Error> {
    if le32(raw, 0) != GEOMETRY_MAGIC || le32(raw, 4) as usize != GEOMETRY_BYTES {
        return Err(Error::Geometry);
    }
    let mut checked = *raw;
    checked[8..40].fill(0);
    if sha256(&checked) != raw[8..40] {
        return Err(Error::GeometryChecksum);
    }
    let metadata_max_size = le32(raw, 40);
    let metadata_slot_count = le32(raw, 44);
    let logical_block_size = le32(raw, 48);
    if metadata_max_size == 0
        || metadata_max_size as usize > MAX_METADATA_BYTES
        || metadata_max_size % SECTOR_BYTES as u32 != 0
        || metadata_slot_count == 0
        || metadata_slot_count > 4
        || logical_block_size < SECTOR_BYTES as u32
        || logical_block_size % SECTOR_BYTES as u32 != 0
        || !logical_block_size.is_power_of_two()
    {
        return Err(Error::Geometry);
    }
    Ok(Geometry {
        metadata_max_size,
        metadata_slot_count,
        logical_block_size,
    })
}

fn metadata_offset(geometry: &Geometry, slot: u32, backup: bool) -> Result<u64, Error> {
    if slot >= geometry.metadata_slot_count {
        return Err(Error::Slot);
    }
    let slots_bytes = u64::from(geometry.metadata_max_size)
        .checked_mul(u64::from(geometry.metadata_slot_count))
        .ok_or(Error::Geometry)?;
    let base = GEOMETRY_RESERVED_BYTES
        .checked_add(GEOMETRY_COPY_BYTES.checked_mul(2).ok_or(Error::Geometry)?)
        .ok_or(Error::Geometry)?;
    let copy_base = if backup {
        base.checked_add(slots_bytes).ok_or(Error::Geometry)?
    } else {
        base
    };
    copy_base
        .checked_add(u64::from(geometry.metadata_max_size) * u64::from(slot))
        .ok_or(Error::Geometry)
}

fn parse_metadata<S: ReadSourceIo>(
    source: &mut S,
    source_bytes: u64,
    geometry: Geometry,
    slot_number: u32,
    target_name: &[u8],
    metadata_at: u64,
) -> Result<([Extent; MAX_PARTITION_EXTENTS], usize, u64), Error> {
    let mut header = [0u8; METADATA_HEADER_BYTES];
    checked_read(source, source_bytes, metadata_at, &mut header)?;
    if le32(&header, 0) != METADATA_MAGIC || le16(&header, 4) != METADATA_MAJOR {
        return Err(Error::MetadataHeader);
    }
    let minor = le16(&header, 6);
    if minor > METADATA_MAX_MINOR || le32(&header, 8) as usize != METADATA_HEADER_BYTES {
        return Err(Error::UnsupportedVersion);
    }
    let mut stored_header_hash = [0u8; 32];
    stored_header_hash.copy_from_slice(&header[12..44]);
    header[12..44].fill(0);
    let computed_header_hash = sha256(&header);
    header[12..44].copy_from_slice(&stored_header_hash);
    if computed_header_hash != stored_header_hash {
        return Err(Error::MetadataChecksum);
    }

    let tables_size = le32(&header, 44);
    let metadata_capacity = geometry.metadata_max_size as usize;
    if tables_size == 0
        || tables_size as usize > MAX_METADATA_BYTES
        || METADATA_HEADER_BYTES
            .checked_add(tables_size as usize)
            .is_none_or(|end| end > metadata_capacity)
    {
        return Err(Error::TableBounds);
    }
    let tables_start = metadata_at
        .checked_add(METADATA_HEADER_BYTES as u64)
        .ok_or(Error::TableBounds)?;
    let mut expected_tables_hash = [0u8; 32];
    expected_tables_hash.copy_from_slice(&header[48..80]);
    let actual_tables_hash =
        hash_source_range(source, source_bytes, tables_start, tables_size as usize)?;
    if actual_tables_hash != expected_tables_hash {
        return Err(Error::MetadataChecksum);
    }

    let partitions = table(&header, 80);
    let extents = table(&header, 92);
    let groups = table(&header, 104);
    let devices = table(&header, 116);
    if partitions.entry_size != PARTITION_ENTRY_BYTES as u32
        || extents.entry_size != EXTENT_ENTRY_BYTES as u32
        || groups.entry_size != GROUP_ENTRY_BYTES as u32
        || devices.entry_size != BLOCK_DEVICE_ENTRY_BYTES as u32
        || partitions.count == 0
        || partitions.count > MAX_PARTITIONS
        || extents.count == 0
        || extents.count > MAX_EXTENTS
        || groups.count == 0
        || groups.count > MAX_GROUPS
        || devices.count == 0
        || devices.count > MAX_BLOCK_DEVICES
    {
        return Err(Error::UnsupportedTable);
    }
    validate_tables_cover_region([partitions, extents, groups, devices], tables_size)?;

    let mut block_devices = [BlockDevice {
        first_logical_sector: 0,
        size_bytes: 0,
    }; MAX_BLOCK_DEVICES as usize];
    for index in 0..devices.count {
        let mut raw = [0u8; BLOCK_DEVICE_ENTRY_BYTES];
        read_entry(source, source_bytes, tables_start, devices, index, &mut raw)?;
        let name = parse_name(&raw[24..60])?;
        let flags = le32(&raw, 60);
        let size_bytes = le64(&raw, 16);
        let first_logical_sector = le64(&raw, 0);
        if flags & !BLOCK_DEVICE_SLOT_SUFFIXED != 0
            || size_bytes == 0
            || size_bytes % SECTOR_BYTES != 0
            || first_logical_sector > size_bytes / SECTOR_BYTES
            || (index == 0 && (name != b"super" || size_bytes != source_bytes))
        {
            return Err(Error::BlockDevice);
        }
        block_devices[index as usize] = BlockDevice {
            first_logical_sector,
            size_bytes,
        };
    }

    for index in 0..groups.count {
        let mut raw = [0u8; GROUP_ENTRY_BYTES];
        read_entry(source, source_bytes, tables_start, groups, index, &mut raw)?;
        let _ = parse_name(&raw[..36])?;
        if le32(&raw, 36) & !GROUP_SLOT_SUFFIXED != 0 {
            return Err(Error::Partition);
        }
    }

    let valid_attributes = ATTR_V0_MASK | if minor >= 1 { ATTR_V1_MASK } else { 0 };
    let mut selected: Option<(u32, u32)> = None;
    for index in 0..partitions.count {
        let mut raw = [0u8; PARTITION_ENTRY_BYTES];
        read_entry(
            source,
            source_bytes,
            tables_start,
            partitions,
            index,
            &mut raw,
        )?;
        let name = parse_name(&raw[..36])?;
        let attributes = le32(&raw, 36);
        let first_extent = le32(&raw, 40);
        let extent_count = le32(&raw, 44);
        let group_index = le32(&raw, 48);
        let extent_end = first_extent
            .checked_add(extent_count)
            .ok_or(Error::Partition)?;
        if attributes & !valid_attributes != 0
            || (extent_count == 0 && attributes & ATTR_DISABLED == 0)
            || extent_end > extents.count
            || group_index >= groups.count
        {
            return Err(Error::Partition);
        }
        if effective_name_matches(name, attributes, slot_number, target_name)? {
            if attributes & ATTR_DISABLED != 0 {
                return Err(Error::Partition);
            }
            if selected.replace((first_extent, extent_count)).is_some() {
                return Err(Error::VendorDuplicate);
            }
        }
    }
    let (first_extent, selected_count) = selected.ok_or(Error::VendorMissing)?;
    if selected_count as usize > MAX_PARTITION_EXTENTS {
        return Err(Error::Extent);
    }

    let metadata_bytes = GEOMETRY_RESERVED_BYTES
        .checked_add(GEOMETRY_COPY_BYTES * 2)
        .and_then(|value| {
            value.checked_add(
                u64::from(geometry.metadata_max_size)
                    .checked_mul(u64::from(geometry.metadata_slot_count))?
                    .checked_mul(2)?,
            )
        })
        .ok_or(Error::Geometry)?;
    let mut super_first_logical_sector = 0;
    let alignment_sectors = u64::from(geometry.logical_block_size / SECTOR_BYTES as u32);
    for index in 0..devices.count {
        if index == 0 {
            super_first_logical_sector = block_devices[0].first_logical_sector;
            if metadata_bytes > super_first_logical_sector * SECTOR_BYTES
                || super_first_logical_sector % alignment_sectors != 0
            {
                return Err(Error::BlockDevice);
            }
        }
    }

    let mut output = [Extent {
        logical_sector: 0,
        sector_count: 0,
        target_sector: 0,
        zero: false,
    }; MAX_PARTITION_EXTENTS];
    let mut logical_sector = 0u64;
    for index in 0..extents.count {
        let mut raw = [0u8; EXTENT_ENTRY_BYTES];
        read_entry(source, source_bytes, tables_start, extents, index, &mut raw)?;
        let sector_count = le64(&raw, 0);
        let target_type = le32(&raw, 8);
        let target_sector = le64(&raw, 12);
        let target_source = le32(&raw, 20);
        if sector_count == 0 {
            return Err(Error::Extent);
        }
        if sector_count % alignment_sectors != 0 {
            return Err(Error::Extent);
        }
        match target_type {
            TARGET_LINEAR => {
                let device = block_devices
                    .get(target_source as usize)
                    .filter(|_| target_source < devices.count)
                    .ok_or(Error::Extent)?;
                let target_end = target_sector
                    .checked_add(sector_count)
                    .ok_or(Error::Extent)?;
                if target_end > device.size_bytes / SECTOR_BYTES
                    || target_sector % alignment_sectors != 0
                    || (target_source == 0 && target_sector < super_first_logical_sector)
                {
                    return Err(Error::Extent);
                }
            }
            TARGET_ZERO if target_sector == 0 && target_source == 0 => {}
            _ => return Err(Error::Extent),
        }
        if index >= first_extent && index < first_extent + selected_count {
            if target_type == TARGET_LINEAR && target_source != 0 {
                return Err(Error::Extent);
            }
            let position = (index - first_extent) as usize;
            output[position] = Extent {
                logical_sector,
                sector_count,
                target_sector,
                zero: target_type == TARGET_ZERO,
            };
            logical_sector = logical_sector
                .checked_add(sector_count)
                .ok_or(Error::Extent)?;
        }
    }
    let logical_bytes = logical_sector
        .checked_mul(SECTOR_BYTES)
        .filter(|bytes| *bytes != 0)
        .ok_or(Error::Extent)?;
    Ok((output, selected_count as usize, logical_bytes))
}

fn effective_name_matches(
    name: &[u8],
    attributes: u32,
    slot_number: u32,
    target_name: &[u8],
) -> Result<bool, Error> {
    if attributes & ATTR_SLOT_SUFFIXED == 0 {
        return Ok(name == target_name);
    }
    let suffix: &[u8] = match slot_number {
        0 => b"_a",
        1 => b"_b",
        _ => return Err(Error::Slot),
    };
    Ok(name
        .len()
        .checked_add(suffix.len())
        .ok_or(Error::Partition)?
        <= 36
        && target_name.len() == name.len() + suffix.len()
        && target_name.starts_with(name)
        && &target_name[name.len()..] == suffix)
}

fn parse_name(raw: &[u8]) -> Result<&[u8], Error> {
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    if end == 0 || raw[end..].iter().any(|byte| *byte != 0) {
        return Err(Error::Partition);
    }
    let name = &raw[..end];
    if !name
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
    {
        return Err(Error::Partition);
    }
    Ok(name)
}

fn table(header: &[u8; METADATA_HEADER_BYTES], at: usize) -> Table {
    Table {
        offset: le32(header, at),
        count: le32(header, at + 4),
        entry_size: le32(header, at + 8),
    }
}

fn validate_tables_cover_region(tables: [Table; 4], tables_size: u32) -> Result<(), Error> {
    let mut total = 0u32;
    for (index, current) in tables.iter().copied().enumerate() {
        let current_len = current.byte_len()?;
        let current_end = current
            .offset
            .checked_add(current_len)
            .ok_or(Error::TableBounds)?;
        if current_end > tables_size {
            return Err(Error::TableBounds);
        }
        total = total.checked_add(current_len).ok_or(Error::TableBounds)?;
        for other in tables.iter().copied().skip(index + 1) {
            let other_len = other.byte_len()?;
            let other_end = other
                .offset
                .checked_add(other_len)
                .ok_or(Error::TableBounds)?;
            if current.offset < other_end && other.offset < current_end {
                return Err(Error::TableBounds);
            }
        }
    }
    if total != tables_size {
        return Err(Error::TableBounds);
    }
    Ok(())
}

fn read_entry<S: ReadSourceIo, const N: usize>(
    source: &mut S,
    source_bytes: u64,
    tables_start: u64,
    table: Table,
    index: u32,
    out: &mut [u8; N],
) -> Result<(), Error> {
    if N != table.entry_size as usize || index >= table.count {
        return Err(Error::TableBounds);
    }
    let relative = u64::from(table.offset)
        .checked_add(u64::from(index) * u64::from(table.entry_size))
        .ok_or(Error::TableBounds)?;
    let at = tables_start
        .checked_add(relative)
        .ok_or(Error::TableBounds)?;
    checked_read(source, source_bytes, at, out)
}

fn hash_source_range<S: ReadSourceIo>(
    source: &mut S,
    source_bytes: u64,
    offset: u64,
    length: usize,
) -> Result<[u8; 32], Error> {
    let mut hash = Sha256::new();
    let mut scratch = [0u8; TABLES_HASH_CHUNK];
    let mut consumed = 0usize;
    while consumed < length {
        let take = (length - consumed).min(scratch.len());
        let at = offset
            .checked_add(consumed as u64)
            .ok_or(Error::TableBounds)?;
        checked_read(source, source_bytes, at, &mut scratch[..take])?;
        hash.update(&scratch[..take]);
        consumed += take;
    }
    Ok(hash.finish())
}

fn checked_read<S: ReadSourceIo>(
    source: &mut S,
    source_bytes: u64,
    offset: u64,
    out: &mut [u8],
) -> Result<(), Error> {
    if source.len().map_err(|_| Error::Source)? != source_bytes {
        return Err(Error::Source);
    }
    let end = offset
        .checked_add(out.len() as u64)
        .ok_or(Error::TableBounds)?;
    if end > source_bytes {
        return Err(Error::Source);
    }
    if !out.is_empty() {
        source
            .read_exact_at(offset, out)
            .map_err(|_| Error::Source)?;
        if source.len().map_err(|_| Error::Source)? != source_bytes {
            return Err(Error::Source);
        }
    }
    Ok(())
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn le64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.finish()
}

pub(crate) struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    used: usize,
    total_bytes: u64,
}

impl Sha256 {
    const INITIAL: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];

    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    pub(crate) const fn new() -> Self {
        Self {
            state: Self::INITIAL,
            block: [0; 64],
            used: 0,
            total_bytes: 0,
        }
    }

    pub(crate) fn update(&mut self, mut bytes: &[u8]) {
        self.total_bytes = self.total_bytes.wrapping_add(bytes.len() as u64);
        if self.used != 0 {
            let take = (64 - self.used).min(bytes.len());
            self.block[self.used..self.used + take].copy_from_slice(&bytes[..take]);
            self.used += take;
            bytes = &bytes[take..];
            if self.used == 64 {
                let block = self.block;
                self.compress(&block);
                self.used = 0;
            }
        }
        while bytes.len() >= 64 {
            let block: &[u8; 64] = bytes[..64].try_into().expect("fixed SHA block length");
            self.compress(block);
            bytes = &bytes[64..];
        }
        if !bytes.is_empty() {
            self.block[..bytes.len()].copy_from_slice(bytes);
            self.used = bytes.len();
        }
    }

    pub(crate) fn finish(mut self) -> [u8; 32] {
        let bit_length = self.total_bytes.wrapping_mul(8);
        self.block[self.used] = 0x80;
        self.used += 1;
        if self.used > 56 {
            self.block[self.used..].fill(0);
            let block = self.block;
            self.compress(&block);
            self.block = [0; 64];
        } else {
            self.block[self.used..56].fill(0);
        }
        self.block[56..64].copy_from_slice(&bit_length.to_be_bytes());
        let block = self.block;
        self.compress(&block);
        let mut output = [0u8; 32];
        for (chunk, word) in output.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        output
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut words = [0u32; 64];
        for (index, bytes) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for index in 16..64 {
            let x = words[index - 15];
            let y = words[index - 2];
            let small0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let small1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            words[index] = words[index - 16]
                .wrapping_add(small0)
                .wrapping_add(words[index - 7])
                .wrapping_add(small1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let big1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(big1)
                .wrapping_add(choose)
                .wrapping_add(Self::K[index])
                .wrapping_add(words[index]);
            let big0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = big0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}
