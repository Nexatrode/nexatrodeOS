//! Bounded parser for Qualcomm MDT ELF32 metadata.
//!
//! This validates headers and segment ranges only. It does not load segment
//! files, map memory, invoke SCM/PAS, or start a remote processor.

#![allow(dead_code)]

pub const MDT_MAX_PROGRAM_HEADERS: usize = 64;
pub const MDT_PAGE_SIZE: u32 = 4096;
pub const MDT_EM_HEXAGON: u16 = 164;
const ELF32_HEADER_SIZE: usize = 52;
const ELF32_PROGRAM_HEADER_SIZE: usize = 32;
const PT_LOAD: u32 = 1;
const QCOM_MDT_TYPE_MASK: u32 = 7 << 24;
const QCOM_MDT_TYPE_HASH: u32 = 2 << 24;
const QCOM_MDT_RELOCATABLE: u32 = 1 << 27;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MdtError {
    TruncatedElfHeader,
    InvalidElfIdentity,
    UnsupportedMachine,
    InvalidProgramHeaderTable,
    TooManyProgramHeaders,
    InvalidSegment,
    NoLoadableSegments,
    SegmentOutsideReservation,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MdtSegment {
    pub index: u16,
    pub linked_address: u32,
    pub file_size: u32,
    pub memory_size: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MdtImage {
    pub segments: [Option<MdtSegment>; MDT_MAX_PROGRAM_HEADERS],
    pub program_header_count: usize,
    pub loadable_segment_count: usize,
    pub linked_min_address: u32,
    /// Page-rounded exclusive end, matching qcom_mdt_get_size().
    pub linked_max_address: u32,
    pub relocatable: bool,
}

impl MdtImage {
    pub fn memory_extent(&self) -> u32 {
        self.linked_max_address - self.linked_min_address
    }

    /// Validate the relocatable image's required span against a fixed pool.
    /// Returns the physical subrange the secure loader would need to own.
    pub fn validate_relocatable_pool(
        &self,
        pool_base: u64,
        pool_size: u64,
    ) -> Result<(u64, u64), MdtError> {
        if !self.relocatable {
            return Err(MdtError::InvalidSegment);
        }
        let extent = self.memory_extent() as u64;
        let pool_end = pool_base
            .checked_add(pool_size)
            .ok_or(MdtError::SegmentOutsideReservation)?;
        let image_end = pool_base
            .checked_add(extent)
            .ok_or(MdtError::SegmentOutsideReservation)?;
        if extent == 0 || image_end > pool_end {
            return Err(MdtError::SegmentOutsideReservation);
        }
        Ok((pool_base, extent))
    }
}

pub fn parse_mdt(data: &[u8]) -> Result<MdtImage, MdtError> {
    if data.len() < ELF32_HEADER_SIZE {
        return Err(MdtError::TruncatedElfHeader);
    }
    if &data[..4] != b"\x7fELF"
        || data[4] != 1 // ELFCLASS32
        || data[5] != 1 // ELFDATA2LSB
        || data[6] != 1
    // EV_CURRENT
    {
        return Err(MdtError::InvalidElfIdentity);
    }
    if read_u16(data, 18)? != MDT_EM_HEXAGON || read_u32(data, 20)? != 1 {
        return Err(MdtError::UnsupportedMachine);
    }
    let phoff = read_u32(data, 28)? as usize;
    let phentsize = read_u16(data, 42)? as usize;
    let phnum = read_u16(data, 44)? as usize;
    if phnum == 0 {
        return Err(MdtError::NoLoadableSegments);
    }
    if phnum > MDT_MAX_PROGRAM_HEADERS {
        return Err(MdtError::TooManyProgramHeaders);
    }
    if phentsize != ELF32_PROGRAM_HEADER_SIZE || phoff < ELF32_HEADER_SIZE {
        return Err(MdtError::InvalidProgramHeaderTable);
    }
    let table_size = phentsize
        .checked_mul(phnum)
        .ok_or(MdtError::InvalidProgramHeaderTable)?;
    let table_end = phoff
        .checked_add(table_size)
        .ok_or(MdtError::InvalidProgramHeaderTable)?;
    if table_end > data.len() {
        return Err(MdtError::InvalidProgramHeaderTable);
    }

    let mut segments = [None; MDT_MAX_PROGRAM_HEADERS];
    let mut count = 0usize;
    let mut min_address = u32::MAX;
    let mut max_address = 0u32;
    let mut relocatable = false;
    for index in 0..phnum {
        let offset = phoff + index * phentsize;
        let kind = read_u32(data, offset)?;
        let linked_address = read_u32(data, offset + 12)?;
        let file_size = read_u32(data, offset + 16)?;
        let memory_size = read_u32(data, offset + 20)?;
        let flags = read_u32(data, offset + 24)?;
        if kind != PT_LOAD || (flags & QCOM_MDT_TYPE_MASK) == QCOM_MDT_TYPE_HASH || memory_size == 0
        {
            continue;
        }
        if file_size > memory_size {
            return Err(MdtError::InvalidSegment);
        }
        let raw_end = linked_address
            .checked_add(memory_size)
            .ok_or(MdtError::InvalidSegment)?;
        let rounded_end = raw_end
            .checked_add(MDT_PAGE_SIZE - 1)
            .ok_or(MdtError::InvalidSegment)?
            & !(MDT_PAGE_SIZE - 1);
        min_address = min_address.min(linked_address);
        max_address = max_address.max(rounded_end);
        relocatable |= flags & QCOM_MDT_RELOCATABLE != 0;
        segments[count] = Some(MdtSegment {
            index: index as u16,
            linked_address,
            file_size,
            memory_size,
            flags,
        });
        count += 1;
    }
    if count == 0 || min_address >= max_address {
        return Err(MdtError::NoLoadableSegments);
    }
    Ok(MdtImage {
        segments,
        program_header_count: phnum,
        loadable_segment_count: count,
        linked_min_address: min_address,
        linked_max_address: max_address,
        relocatable,
    })
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, MdtError> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or(MdtError::TruncatedElfHeader)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, MdtError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(MdtError::TruncatedElfHeader)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf_fixture(headers: &[(u32, u32, u32, u32, u32)]) -> std::vec::Vec<u8> {
        let mut bytes = vec![0u8; ELF32_HEADER_SIZE + headers.len() * ELF32_PROGRAM_HEADER_SIZE];
        bytes[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        bytes[18..20].copy_from_slice(&MDT_EM_HEXAGON.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&(ELF32_HEADER_SIZE as u32).to_le_bytes());
        bytes[40..42].copy_from_slice(&(ELF32_HEADER_SIZE as u16).to_le_bytes());
        bytes[42..44].copy_from_slice(&(ELF32_PROGRAM_HEADER_SIZE as u16).to_le_bytes());
        bytes[44..46].copy_from_slice(&(headers.len() as u16).to_le_bytes());
        for (index, &(kind, address, file_size, memory_size, flags)) in headers.iter().enumerate() {
            let at = ELF32_HEADER_SIZE + index * ELF32_PROGRAM_HEADER_SIZE;
            bytes[at..at + 4].copy_from_slice(&kind.to_le_bytes());
            bytes[at + 12..at + 16].copy_from_slice(&address.to_le_bytes());
            bytes[at + 16..at + 20].copy_from_slice(&file_size.to_le_bytes());
            bytes[at + 20..at + 24].copy_from_slice(&memory_size.to_le_bytes());
            bytes[at + 24..at + 28].copy_from_slice(&flags.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn parses_relocatable_segments_and_checks_pool_extent() {
        let bytes = elf_fixture(&[
            (PT_LOAD, 0x8b000000, 0x1000, 0x4000, QCOM_MDT_RELOCATABLE),
            (PT_LOAD, 0x8b004000, 0x2000, 0x5000, QCOM_MDT_RELOCATABLE),
            (PT_LOAD, 0x8b009000, 0x10, 0x100, QCOM_MDT_TYPE_HASH),
            (0, 0, 0, 0, 0),
        ]);
        let image = parse_mdt(&bytes).unwrap();
        assert_eq!(image.program_header_count, 4);
        assert_eq!(image.loadable_segment_count, 2);
        assert_eq!(image.linked_min_address, 0x8b000000);
        assert_eq!(image.linked_max_address, 0x8b009000);
        assert_eq!(image.memory_extent(), 0x9000);
        assert_eq!(
            image.validate_relocatable_pool(0x8b000000, 0x9800000),
            Ok((0x8b000000, 0x9000))
        );
        assert_eq!(
            image.validate_relocatable_pool(0x8b000000, 0x8fff),
            Err(MdtError::SegmentOutsideReservation)
        );
    }

    #[test]
    fn rejects_malformed_headers_and_segment_sizes() {
        assert_eq!(parse_mdt(&[0; 20]), Err(MdtError::TruncatedElfHeader));
        let mut bytes = elf_fixture(&[(PT_LOAD, 0x1000, 8, 4, QCOM_MDT_RELOCATABLE)]);
        assert_eq!(parse_mdt(&bytes), Err(MdtError::InvalidSegment));
        bytes[18..20].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(parse_mdt(&bytes), Err(MdtError::UnsupportedMachine));
    }
}
