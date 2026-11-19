//! Bounded, read-only GPT validation for the Sunfish UFS storage provider.
//!
//! These parsers consume GPT headers and entry-array bytes supplied by a
//! caller. They do not issue SCSI reads or access a device. The supported
//! Sunfish profile is capped at a 16 KiB entry array so callers can reserve a
//! fixed scratch region.
#![forbid(unsafe_code)]

pub const LOGICAL_BLOCK_BYTES: usize = 512;
pub const MAX_LOGICAL_BLOCK_BYTES: u32 = 4096;
pub const GPT_HEADER_BYTES: usize = LOGICAL_BLOCK_BYTES;
pub const GPT_MIN_HEADER_SIZE: usize = 92;
pub const GPT_PARTITION_ENTRY_BYTES: usize = 128;
pub const MAX_ENTRY_ARRAY_BYTES: usize = 16 * 1024;
pub const MAX_PARTITION_ENTRIES: usize = 128;

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_REVISION_1_0: u32 = 0x0001_0000;
const ENTRY_ARRAY_CRC_OFFSET: usize = 88;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidCapacity,
    Signature,
    Revision,
    HeaderSize,
    HeaderCrc,
    Reserved,
    DiskGuid,
    PrimaryLocation,
    Geometry,
    EntryGeometry,
    EntryArrayLength,
    EntryArrayCrc,
    BackupLocation,
    BackupGeometry,
    BackupHeaderCrc,
    BackupEntryArrayLength,
    BackupEntryArrayCrc,
    CopiesMismatch,
    EntryInvalid,
    PartitionBounds,
    PartitionOverlap,
    DuplicateGuid,
    DuplicateName,
    MissingPartition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartitionExtent {
    /// Native GPT logical-block address (not the modem API's 512-byte sector).
    pub first_lba: u64,
    pub last_lba: u64,
    /// Count of native GPT logical blocks.
    pub sectors: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModemPartitions {
    /// Raw on-disk GUID bytes; callers must bind this to trusted device state.
    pub disk_guid: [u8; 16],
    pub modemst1: PartitionExtent,
    pub modemst2: PartitionExtent,
    pub fsg: PartitionExtent,
    pub fsc: PartitionExtent,
    /// The dynamic-partition container used by Android's `vendor_a`/`vendor_b`.
    pub super_partition: PartitionExtent,
    /// Native GPT logical block size (512 or 4096 bytes).
    pub logical_block_bytes: u32,
}

/// The `super` extent needed by Android's dynamic partitions. The Sunfish user
/// LUN may have a zero GPT disk GUID; callers must bind it to a trusted UFS LUN
/// identity rather than treating `disk_guid` as a device identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SuperPartition {
    pub disk_guid: [u8; 16],
    pub extent: PartitionExtent,
    pub logical_block_bytes: u32,
}

/// Sunfish GPT extents that have passed reciprocal primary/backup GPT, header
/// and entry-array CRC, geometry, GUID, overlap, and exact-name checks for the
/// modem state partitions and `super`. Its private field prevents callers from
/// manufacturing the proof from a hand-built `ModemPartitions` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedModemPartitions(ModemPartitions);

/// A super extent extracted only after the primary and backup GPT copies agree.
/// This proves table consistency, not disk identity; callers must bind the
/// extent to the expected UFS LUN using a trusted device identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedSuperPartition(SuperPartition);

impl VerifiedModemPartitions {
    pub const fn partitions(self) -> ModemPartitions {
        self.0
    }
}

impl VerifiedSuperPartition {
    pub const fn partition(self) -> SuperPartition {
        self.0
    }
}

/// Validate the primary GPT and locate the four modem partitions plus `super`
/// by exact ASCII GPT name. The entry array is bounded to 16 KiB and its CRC
/// must match the primary header. Every active entry is checked for a valid
/// in-range extent, unique partition GUID, and overlap with earlier entries.
pub fn parse_sunfish_primary(
    header: &[u8; GPT_HEADER_BYTES],
    entries: &[u8],
    lun_sectors: u64,
) -> Result<ModemPartitions, Error> {
    parse_sunfish_primary_with_block_size(header, entries, lun_sectors, LOGICAL_BLOCK_BYTES as u32)
}

/// Validate a primary GPT using native SCSI LBAs. The GPT header structure
/// itself remains 512 bytes even when native blocks are 4096 bytes.
pub fn parse_sunfish_primary_with_block_size(
    header: &[u8; GPT_HEADER_BYTES],
    entries: &[u8],
    lun_blocks: u64,
    logical_block_bytes: u32,
) -> Result<ModemPartitions, Error> {
    parse_sunfish_primary_impl(header, entries, lun_blocks, logical_block_bytes, true)
}

/// Validate the complete primary GPT and require only the `super` partition.
/// This matches the actual Sunfish user LUN, which contains `modem_a` and
/// `modem_b`, not the modem NV names used by the separate QRTR storage API.
pub fn parse_sunfish_super_primary_with_block_size(
    header: &[u8; GPT_HEADER_BYTES],
    entries: &[u8],
    lun_blocks: u64,
    logical_block_bytes: u32,
) -> Result<SuperPartition, Error> {
    let parsed =
        parse_sunfish_primary_impl(header, entries, lun_blocks, logical_block_bytes, false)?;
    Ok(SuperPartition {
        disk_guid: parsed.disk_guid,
        extent: parsed.super_partition,
        logical_block_bytes: parsed.logical_block_bytes,
    })
}

fn parse_sunfish_primary_impl(
    header: &[u8; GPT_HEADER_BYTES],
    entries: &[u8],
    lun_blocks: u64,
    logical_block_bytes: u32,
    require_modem_partitions: bool,
) -> Result<ModemPartitions, Error> {
    if lun_blocks < 2 {
        return Err(Error::InvalidCapacity);
    }
    if !supported_logical_block_bytes(logical_block_bytes) {
        return Err(Error::Geometry);
    }
    if &header[0..8] != GPT_SIGNATURE {
        return Err(Error::Signature);
    }
    if le32(header, 8) != GPT_REVISION_1_0 {
        return Err(Error::Revision);
    }
    let header_size = usize::try_from(le32(header, 12)).map_err(|_| Error::HeaderSize)?;
    if !(GPT_MIN_HEADER_SIZE..=GPT_HEADER_BYTES).contains(&header_size) {
        return Err(Error::HeaderSize);
    }
    if le32(header, 20) != 0 || header[GPT_MIN_HEADER_SIZE..].iter().any(|byte| *byte != 0) {
        return Err(Error::Reserved);
    }
    if crc32_with_zeroed_field(&header[..header_size], 16..20) != le32(header, 16) {
        return Err(Error::HeaderCrc);
    }

    let disk_guid: [u8; 16] = header[56..72].try_into().map_err(|_| Error::DiskGuid)?;
    if require_modem_partitions && disk_guid == [0; 16] {
        return Err(Error::DiskGuid);
    }
    let alternate_lba = le64(header, 32);
    let first_usable = le64(header, 40);
    let last_usable = le64(header, 48);
    let array_lba = le64(header, 72);
    let entry_count = usize::try_from(le32(header, 80)).map_err(|_| Error::EntryGeometry)?;
    let entry_bytes = usize::try_from(le32(header, 84)).map_err(|_| Error::EntryGeometry)?;
    let expected_array_crc = le32(header, ENTRY_ARRAY_CRC_OFFSET);
    let lun_last_lba = lun_blocks - 1;

    if le64(header, 24) != 1 || alternate_lba != lun_last_lba {
        return Err(Error::PrimaryLocation);
    }
    if first_usable > last_usable || last_usable >= alternate_lba {
        return Err(Error::Geometry);
    }
    if entry_count == 0
        || entry_count > MAX_PARTITION_ENTRIES
        || entry_bytes < GPT_PARTITION_ENTRY_BYTES
        || entry_bytes > MAX_ENTRY_ARRAY_BYTES
        || entry_bytes % GPT_PARTITION_ENTRY_BYTES != 0
        || !(entry_bytes / GPT_PARTITION_ENTRY_BYTES).is_power_of_two()
    {
        return Err(Error::EntryGeometry);
    }
    let array_len = entry_count
        .checked_mul(entry_bytes)
        .ok_or(Error::EntryGeometry)?;
    if array_len > MAX_ENTRY_ARRAY_BYTES {
        return Err(Error::EntryGeometry);
    }
    if entries.len() != array_len {
        return Err(Error::EntryArrayLength);
    }
    if crc32(entries) != expected_array_crc {
        return Err(Error::EntryArrayCrc);
    }
    let array_sectors = array_len
        .checked_add(logical_block_bytes as usize - 1)
        .ok_or(Error::Geometry)?
        / logical_block_bytes as usize;
    let array_end = array_lba
        .checked_add(u64::try_from(array_sectors).map_err(|_| Error::Geometry)?)
        .ok_or(Error::Geometry)?;
    if array_lba < 2 || array_end > first_usable || array_end > alternate_lba {
        return Err(Error::Geometry);
    }

    let mut result = ModemPartitions {
        disk_guid,
        modemst1: empty_extent(),
        modemst2: empty_extent(),
        fsg: empty_extent(),
        fsc: empty_extent(),
        super_partition: empty_extent(),
        logical_block_bytes,
    };
    let mut found = [false; 5];

    for index in 0..entry_count {
        let start = index.checked_mul(entry_bytes).ok_or(Error::EntryGeometry)?;
        let entry = &entries[start..start + entry_bytes];
        let type_guid = &entry[..16];
        if type_guid.iter().all(|byte| *byte == 0) {
            if entry.iter().any(|byte| *byte != 0) {
                return Err(Error::EntryInvalid);
            }
            continue;
        }
        if entry[16..32].iter().all(|byte| *byte == 0) || entry[128..].iter().any(|byte| *byte != 0)
        {
            return Err(Error::EntryInvalid);
        }
        let first = le64(entry, 32);
        let last = le64(entry, 40);
        if first > last || first < first_usable || last > last_usable || last >= lun_blocks {
            return Err(Error::PartitionBounds);
        }

        for previous_index in 0..index {
            let previous_start = previous_index
                .checked_mul(entry_bytes)
                .ok_or(Error::EntryGeometry)?;
            let previous = &entries[previous_start..previous_start + entry_bytes];
            if previous[..16].iter().all(|byte| *byte == 0) {
                continue;
            }
            if entry[16..32] == previous[16..32] {
                return Err(Error::DuplicateGuid);
            }
            let previous_first = le64(previous, 32);
            let previous_last = le64(previous, 40);
            if first <= previous_last && previous_first <= last {
                return Err(Error::PartitionOverlap);
            }
        }

        let extent = PartitionExtent {
            first_lba: first,
            last_lba: last,
            sectors: last - first + 1,
        };
        for (partition_index, name) in REQUIRED_NAMES.iter().enumerate() {
            if name_matches(&entry[56..128], name) {
                if found[partition_index] {
                    return Err(Error::DuplicateName);
                }
                found[partition_index] = true;
                match partition_index {
                    0 => result.modemst1 = extent,
                    1 => result.modemst2 = extent,
                    2 => result.fsg = extent,
                    3 => result.fsc = extent,
                    4 => result.super_partition = extent,
                    _ => return Err(Error::EntryInvalid),
                }
            }
        }
    }

    if !found[4] || (require_modem_partitions && found[..4].iter().any(|value| !value)) {
        return Err(Error::MissingPartition);
    }
    Ok(result)
}

/// Validate both GPT copies and return Sunfish extents only when the backup
/// header and entry array agree with the primary copy. The caller supplies
/// already-read sectors; this function performs no device I/O.
pub fn parse_sunfish_pair(
    primary_header: &[u8; GPT_HEADER_BYTES],
    primary_entries: &[u8],
    backup_header: &[u8; GPT_HEADER_BYTES],
    backup_entries: &[u8],
    lun_sectors: u64,
) -> Result<ModemPartitions, Error> {
    parse_sunfish_pair_with_block_size(
        primary_header,
        primary_entries,
        backup_header,
        backup_entries,
        lun_sectors,
        LOGICAL_BLOCK_BYTES as u32,
    )
}

/// Validate primary and backup GPT copies using native SCSI logical blocks.
pub fn parse_sunfish_pair_with_block_size(
    primary_header: &[u8; GPT_HEADER_BYTES],
    primary_entries: &[u8],
    backup_header: &[u8; GPT_HEADER_BYTES],
    backup_entries: &[u8],
    lun_blocks: u64,
    logical_block_bytes: u32,
) -> Result<ModemPartitions, Error> {
    parse_sunfish_pair_impl(
        primary_header,
        primary_entries,
        backup_header,
        backup_entries,
        lun_blocks,
        logical_block_bytes,
        true,
    )
}

fn parse_sunfish_pair_impl(
    primary_header: &[u8; GPT_HEADER_BYTES],
    primary_entries: &[u8],
    backup_header: &[u8; GPT_HEADER_BYTES],
    backup_entries: &[u8],
    lun_blocks: u64,
    logical_block_bytes: u32,
    require_modem_partitions: bool,
) -> Result<ModemPartitions, Error> {
    let primary = parse_sunfish_primary_impl(
        primary_header,
        primary_entries,
        lun_blocks,
        logical_block_bytes,
        require_modem_partitions,
    )?;
    let lun_last_lba = lun_blocks.checked_sub(1).ok_or(Error::InvalidCapacity)?;

    if &backup_header[0..8] != GPT_SIGNATURE {
        return Err(Error::Signature);
    }
    if le32(backup_header, 8) != GPT_REVISION_1_0 {
        return Err(Error::Revision);
    }
    let backup_header_size =
        usize::try_from(le32(backup_header, 12)).map_err(|_| Error::HeaderSize)?;
    if !(GPT_MIN_HEADER_SIZE..=GPT_HEADER_BYTES).contains(&backup_header_size) {
        return Err(Error::HeaderSize);
    }
    if le32(backup_header, 20) != 0
        || backup_header[GPT_MIN_HEADER_SIZE..]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err(Error::Reserved);
    }
    if crc32_with_zeroed_field(&backup_header[..backup_header_size], 16..20)
        != le32(backup_header, 16)
    {
        return Err(Error::BackupHeaderCrc);
    }

    let backup_my_lba = le64(backup_header, 24);
    let backup_alternate_lba = le64(backup_header, 32);
    if backup_my_lba != lun_last_lba || backup_alternate_lba != 1 {
        return Err(Error::BackupLocation);
    }

    let first_usable = le64(backup_header, 40);
    let last_usable = le64(backup_header, 48);
    let backup_array_lba = le64(backup_header, 72);
    let backup_entry_count =
        usize::try_from(le32(backup_header, 80)).map_err(|_| Error::BackupGeometry)?;
    let backup_entry_bytes =
        usize::try_from(le32(backup_header, 84)).map_err(|_| Error::BackupGeometry)?;
    if first_usable > last_usable || last_usable >= backup_my_lba {
        return Err(Error::BackupGeometry);
    }
    if backup_entry_count == 0
        || backup_entry_count > MAX_PARTITION_ENTRIES
        || backup_entry_bytes < GPT_PARTITION_ENTRY_BYTES
        || backup_entry_bytes > MAX_ENTRY_ARRAY_BYTES
        || backup_entry_bytes % GPT_PARTITION_ENTRY_BYTES != 0
        || !(backup_entry_bytes / GPT_PARTITION_ENTRY_BYTES).is_power_of_two()
    {
        return Err(Error::BackupGeometry);
    }
    let backup_array_len = backup_entry_count
        .checked_mul(backup_entry_bytes)
        .ok_or(Error::BackupGeometry)?;
    if backup_array_len > MAX_ENTRY_ARRAY_BYTES {
        return Err(Error::BackupGeometry);
    }
    if backup_entries.len() != backup_array_len {
        return Err(Error::BackupEntryArrayLength);
    }
    let backup_array_sectors = backup_array_len
        .checked_add(logical_block_bytes as usize - 1)
        .ok_or(Error::BackupGeometry)?
        / logical_block_bytes as usize;
    let backup_array_end = backup_array_lba
        .checked_add(u64::try_from(backup_array_sectors).map_err(|_| Error::BackupGeometry)?)
        .ok_or(Error::BackupGeometry)?;
    if backup_array_lba <= last_usable || backup_array_end > backup_my_lba {
        return Err(Error::BackupGeometry);
    }
    if crc32(backup_entries) != le32(backup_header, ENTRY_ARRAY_CRC_OFFSET) {
        return Err(Error::BackupEntryArrayCrc);
    }

    if le32(backup_header, 12) != le32(primary_header, 12)
        || first_usable != le64(primary_header, 40)
        || last_usable != le64(primary_header, 48)
        || backup_header[56..72] != primary_header[56..72]
        || le32(backup_header, 80) != le32(primary_header, 80)
        || le32(backup_header, 84) != le32(primary_header, 84)
        || le32(backup_header, ENTRY_ARRAY_CRC_OFFSET)
            != le32(primary_header, ENTRY_ARRAY_CRC_OFFSET)
        || backup_entries != primary_entries
    {
        return Err(Error::CopiesMismatch);
    }

    Ok(primary)
}

/// Validate both GPT copies and retain a type-level proof that the returned
/// Sunfish extents came from the paired-table validator.
pub fn parse_sunfish_pair_verified(
    primary_header: &[u8; GPT_HEADER_BYTES],
    primary_entries: &[u8],
    backup_header: &[u8; GPT_HEADER_BYTES],
    backup_entries: &[u8],
    lun_sectors: u64,
) -> Result<VerifiedModemPartitions, Error> {
    parse_sunfish_pair_with_block_size(
        primary_header,
        primary_entries,
        backup_header,
        backup_entries,
        lun_sectors,
        LOGICAL_BLOCK_BYTES as u32,
    )
    .map(VerifiedModemPartitions)
}

/// Validate a paired GPT using native block geometry and retain its proof.
pub fn parse_sunfish_pair_verified_with_block_size(
    primary_header: &[u8; GPT_HEADER_BYTES],
    primary_entries: &[u8],
    backup_header: &[u8; GPT_HEADER_BYTES],
    backup_entries: &[u8],
    lun_blocks: u64,
    logical_block_bytes: u32,
) -> Result<VerifiedModemPartitions, Error> {
    parse_sunfish_pair_with_block_size(
        primary_header,
        primary_entries,
        backup_header,
        backup_entries,
        lun_blocks,
        logical_block_bytes,
    )
    .map(VerifiedModemPartitions)
}

/// Validate both copies of the Sunfish user-LUN GPT and retain only its
/// `super` partition proof, without requiring modem NV partition names.
pub fn parse_sunfish_super_pair_verified_with_block_size(
    primary_header: &[u8; GPT_HEADER_BYTES],
    primary_entries: &[u8],
    backup_header: &[u8; GPT_HEADER_BYTES],
    backup_entries: &[u8],
    lun_blocks: u64,
    logical_block_bytes: u32,
) -> Result<VerifiedSuperPartition, Error> {
    let parsed = parse_sunfish_pair_impl(
        primary_header,
        primary_entries,
        backup_header,
        backup_entries,
        lun_blocks,
        logical_block_bytes,
        false,
    )?;
    Ok(VerifiedSuperPartition(SuperPartition {
        disk_guid: parsed.disk_guid,
        extent: parsed.super_partition,
        logical_block_bytes: parsed.logical_block_bytes,
    }))
}

pub const fn supported_logical_block_bytes(block_bytes: u32) -> bool {
    block_bytes == LOGICAL_BLOCK_BYTES as u32 || block_bytes == MAX_LOGICAL_BLOCK_BYTES
}

const REQUIRED_NAMES: [&[u8]; 5] = [b"modemst1", b"modemst2", b"fsg", b"fsc", b"super"];

const fn empty_extent() -> PartitionExtent {
    PartitionExtent {
        first_lba: 0,
        last_lba: 0,
        sectors: 0,
    }
}

fn name_matches(utf16_le: &[u8], expected: &[u8]) -> bool {
    if utf16_le.len() != 72 || expected.len() > 36 {
        return false;
    }
    for index in 0..36 {
        let code_unit = u16::from_le_bytes([utf16_le[index * 2], utf16_le[index * 2 + 1]]);
        if index < expected.len() {
            if code_unit != u16::from(expected[index]) {
                return false;
            }
        } else if code_unit != 0 {
            return false;
        }
    }
    true
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

fn crc32_with_zeroed_field(bytes: &[u8], zero: core::ops::Range<usize>) -> u32 {
    let mut crc = !0u32;
    for (index, byte) in bytes.iter().enumerate() {
        crc = crc32_byte(crc, if zero.contains(&index) { 0 } else { *byte });
    }
    !crc
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc = crc32_byte(crc, *byte);
    }
    !crc
}

fn crc32_byte(mut crc: u32, byte: u8) -> u32 {
    crc ^= u32::from(byte);
    for _ in 0..8 {
        crc = if crc & 1 == 1 {
            (crc >> 1) ^ 0xedb8_8320
        } else {
            crc >> 1
        };
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
