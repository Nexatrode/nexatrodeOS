//! Prepare an owned, contiguous Sunfish MDT buffer without any PAS operation.
//!
//! Allocation ownership/physical exclusion is a build-and-capability precondition.
//! The checked address alone grants no authority. Byte-volatile access supports
//! the pinned caller's Device-nGnRnE mapping without unaligned/vector copies.
//! No firmware segment, modem-pool mapping, SMC, reset, or DMA is supplied.

use crate::qcom_mdt::{MdtError, MdtImage, parse_mdt};

pub const PAGE: usize = 4096;
pub const CAPACITY: usize = 3 * PAGE;
pub const MAPPING_SIZE: usize = 5 * PAGE;
pub const MODEM_BASE: u64 = 0x8b00_0000;
pub const MODEM_SIZE: u64 = 0x0980_0000;
const CANARY: u8 = 0xa5;

pub trait Memory {
    fn read_byte(&mut self, offset: usize) -> u8;
    fn write_byte(&mut self, offset: usize, value: u8);
    fn sync(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Address,
    Length,
    Mdt(MdtError),
    Relocation,
    Overlap,
    DirtyMapping,
    Copy,
    Guard,
    Padding,
}
impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::Address => 1,
            Self::Length => 2,
            Self::Mdt(e) => 0x100 + e as u32,
            Self::Relocation => 3,
            Self::Overlap => 4,
            Self::DirtyMapping => 5,
            Self::Copy => 6,
            Self::Guard => 7,
            Self::Padding => 8,
        }
    }
}

/// Diagnostic result, not permission for a secure operation or DMA exposure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prepared {
    pub metadata_physical: u64,
    pub metadata_length: usize,
    pub program_headers: usize,
    pub loadable_segments: usize,
    pub linked_start: u32,
    pub linked_end: u32,
}

pub(crate) fn validate_image(data: &[u8]) -> Result<MdtImage, Error> {
    if data.is_empty() || data.len() > CAPACITY {
        return Err(Error::Length);
    }
    let image = parse_mdt(data).map_err(Error::Mdt)?;
    if image.linked_min_address != MODEM_BASE as u32
        || image
            .segments
            .iter()
            .flatten()
            .any(|s| s.flags & (1 << 27) == 0)
    {
        return Err(Error::Relocation);
    }
    image
        .validate_relocatable_pool(MODEM_BASE, MODEM_SIZE)
        .map_err(Error::Mdt)?;
    for (i, a) in image.segments.iter().flatten().enumerate() {
        // The parser already checked these sums for overflow.
        let end = a.linked_address + a.memory_size;
        for b in image.segments.iter().flatten().skip(i + 1) {
            if a.linked_address < b.linked_address + b.memory_size && b.linked_address < end {
                return Err(Error::Overlap);
            }
        }
    }
    Ok(image)
}

pub(crate) fn validate_address(physical: u64) -> Result<(), Error> {
    let end = physical
        .checked_add(MAPPING_SIZE as u64)
        .ok_or(Error::Address)?;
    if physical == 0
        || physical & (PAGE as u64 - 1) != 0
        || end > (1 << 48)
        || (physical < MODEM_BASE + MODEM_SIZE && MODEM_BASE < end)
    {
        return Err(Error::Address);
    }
    Ok(())
}

/// Own exactly five contiguous, initially zero RAM pages: guard, three metadata
/// pages, guard. Verify structure before any write; retain zero padding and
/// check every payload/guard/padding byte after a barrier. The caller must keep
/// this owned memory mapped for the complete future consumer lifetime.
pub fn prepare(memory: &mut impl Memory, physical: u64, data: &[u8]) -> Result<Prepared, Error> {
    validate_address(physical)?;
    validate_image(data)?;
    if (0..MAPPING_SIZE).any(|i| memory.read_byte(i) != 0) {
        return Err(Error::DirtyMapping);
    }
    for i in 0..PAGE {
        memory.write_byte(i, CANARY);
        memory.write_byte(PAGE + CAPACITY + i, CANARY);
    }
    for (i, byte) in data.iter().enumerate() {
        memory.write_byte(PAGE + i, *byte);
    }
    verify(memory, physical, data)
}

/// Recheck a prepared buffer without writing or restoring any byte. Useful
/// after unrelated platform work and before a future secure consumer.
pub fn verify(memory: &mut impl Memory, physical: u64, data: &[u8]) -> Result<Prepared, Error> {
    validate_address(physical)?;
    let image = validate_image(data)?;
    memory.sync();
    let mut readback = [0u8; CAPACITY];
    for (i, byte) in data.iter().enumerate() {
        let actual = memory.read_byte(PAGE + i);
        if actual != *byte {
            return Err(Error::Copy);
        }
        readback[i] = actual;
    }
    for i in 0..PAGE {
        if memory.read_byte(i) != CANARY || memory.read_byte(PAGE + CAPACITY + i) != CANARY {
            return Err(Error::Guard);
        }
    }
    if (data.len()..CAPACITY).any(|i| memory.read_byte(PAGE + i) != 0) {
        return Err(Error::Padding);
    }
    let staged_image = validate_image(&readback[..data.len()])?;
    if staged_image != image {
        return Err(Error::Copy);
    }
    Ok(Prepared {
        metadata_physical: physical + PAGE as u64,
        metadata_length: data.len(),
        program_headers: image.program_header_count,
        loadable_segments: image.loadable_segment_count,
        linked_start: image.linked_min_address,
        linked_end: image.linked_max_address,
    })
}

/// Diagnostic snapshot only. Never authorizes later PAS operations or treats
/// an unexplained RW-consumer change as an acceptable new metadata image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Inspection {
    pub low_guard_bad: u32,
    pub high_guard_bad: u32,
    pub padding_bad: u32,
    pub payload_diff: u32,
    pub payload_zero: u32,
    /// Payload-relative offsets; words below start at the first changed byte.
    pub first_diff: Option<u32>,
    pub last_diff: Option<u32>,
    pub expected_words: [u32; 2],
    pub actual_words: [u32; 2],
    pub input_crc32: u32,
    pub actual_crc32: u32,
    pub actual_parse_error: u32,
    /// Exact byte comparison of a second full allocation read to the snapshot.
    pub unstable_bytes: u32,
}
impl Inspection {
    pub fn strict_error(&self) -> u32 {
        if self.payload_diff != 0 {
            Error::Copy.code()
        } else if self.low_guard_bad != 0 || self.high_guard_bad != 0 {
            Error::Guard.code()
        } else if self.padding_bad != 0 {
            Error::Padding.code()
        } else if self.unstable_bytes != 0 {
            9
        } else {
            self.actual_parse_error
        }
    }
}

/// CRC32/IEEE is a diagnostic fingerprint, not an authentication check. The
/// build pins SHA-256 separately; stability below uses exact bytes, not CRC.
pub const fn diagnostic_crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
            bit += 1;
        }
        i += 1;
    }
    !crc
}

/// Read every byte twice without early mismatch exit, writes, repairs, or a new
/// secure call. All payload, padding and guard results refer to the first full
/// snapshot; the second pass detects changes during this observation window.
pub fn inspect(memory: &mut impl Memory, physical: u64, data: &[u8]) -> Result<Inspection, Error> {
    validate_address(physical)?;
    validate_image(data)?;
    let mut snapshot = [0u8; MAPPING_SIZE];
    memory.sync();
    for (i, byte) in snapshot.iter_mut().enumerate() {
        *byte = memory.read_byte(i);
    }
    let payload = &snapshot[PAGE..PAGE + data.len()];
    let mut r = Inspection {
        low_guard_bad: snapshot[..PAGE].iter().filter(|v| **v != CANARY).count() as u32,
        high_guard_bad: snapshot[PAGE + CAPACITY..]
            .iter()
            .filter(|v| **v != CANARY)
            .count() as u32,
        padding_bad: snapshot[PAGE + data.len()..PAGE + CAPACITY]
            .iter()
            .filter(|v| **v != 0)
            .count() as u32,
        payload_diff: 0,
        payload_zero: payload.iter().filter(|v| **v == 0).count() as u32,
        first_diff: None,
        last_diff: None,
        expected_words: [0; 2],
        actual_words: [0; 2],
        input_crc32: diagnostic_crc32(data),
        actual_crc32: diagnostic_crc32(payload),
        actual_parse_error: validate_image(payload).err().map_or(0, |e| e.code()),
        unstable_bytes: 0,
    };
    for (i, (actual, expected)) in payload.iter().zip(data).enumerate() {
        if actual != expected {
            r.payload_diff += 1;
            r.first_diff.get_or_insert(i as u32);
            r.last_diff = Some(i as u32);
        }
    }
    if let Some(first) = r.first_diff {
        for n in 0..8 {
            let at = first as usize + n;
            // Near the final byte, zero-fill only the diagnostic word. Never
            // read beyond the payload or confuse padding with metadata bytes.
            if at < data.len() {
                r.expected_words[n / 4] |= (data[at] as u32) << (8 * (n % 4));
                r.actual_words[n / 4] |= (payload[at] as u32) << (8 * (n % 4));
            }
        }
    }
    memory.sync();
    for (i, byte) in snapshot.iter().enumerate() {
        r.unstable_bytes += u32::from(memory.read_byte(i) != *byte);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    struct Ram {
        bytes: Vec<u8>,
        writes: usize,
        corrupt_after_sync: Option<usize>,
    }
    impl Ram {
        fn new() -> Self {
            Self {
                bytes: vec![0; MAPPING_SIZE],
                writes: 0,
                corrupt_after_sync: None,
            }
        }
    }
    impl Memory for Ram {
        fn read_byte(&mut self, offset: usize) -> u8 {
            self.bytes[offset]
        }
        fn write_byte(&mut self, offset: usize, value: u8) {
            self.writes += 1;
            self.bytes[offset] = value;
        }
        fn sync(&mut self) {
            if let Some(i) = self.corrupt_after_sync {
                self.bytes[i] ^= 1;
            }
        }
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 52 + 64];
        b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        b[18..20].copy_from_slice(&164u16.to_le_bytes());
        b[20..24].copy_from_slice(&1u32.to_le_bytes());
        b[28..32].copy_from_slice(&52u32.to_le_bytes());
        b[42..44].copy_from_slice(&32u16.to_le_bytes());
        b[44..46].copy_from_slice(&2u16.to_le_bytes());
        for (i, addr) in [0x8b000000u32, 0x8b002000].into_iter().enumerate() {
            let at = 52 + i * 32;
            for (offset, value) in [
                (0, 1u32),
                (12, addr),
                (16, 0x100),
                (20, 0x1000),
                (24, 1 << 27),
            ] {
                b[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        b
    }
    #[test]
    fn prepares_exact_payload_with_zero_padding_and_two_guard_pages() {
        let mut ram = Ram::new();
        let input = fixture();
        let r = prepare(&mut ram, 0x80080000, &input).unwrap();
        assert_eq!(r.metadata_physical, 0x80081000);
        assert_eq!(r.metadata_length, input.len());
        assert_eq!((r.program_headers, r.loadable_segments), (2, 2));
        assert_eq!(&ram.bytes[PAGE..PAGE + input.len()], &input);
        assert!(ram.bytes[..PAGE].iter().all(|b| *b == CANARY));
        assert!(ram.bytes[PAGE + CAPACITY..].iter().all(|b| *b == CANARY));
        assert!(
            ram.bytes[PAGE + input.len()..PAGE + CAPACITY]
                .iter()
                .all(|b| *b == 0)
        );
    }
    #[test]
    fn rejects_bad_geometry_or_modem_overlap_before_any_memory_write() {
        for address in [
            0,
            0x80080001,
            u64::MAX - 0xfff,
            1 << 48,
            MODEM_BASE - PAGE as u64,
            MODEM_BASE,
            MODEM_BASE + MODEM_SIZE - PAGE as u64,
        ] {
            let mut ram = Ram::new();
            assert_eq!(
                prepare(&mut ram, address, &fixture()).err(),
                Some(Error::Address)
            );
            assert_eq!(ram.writes, 0);
        }
    }
    #[test]
    fn rejects_oversized_malformed_mixed_relocation_and_overlapping_images_before_writes() {
        let mut invalid = vec![vec![], vec![0; CAPACITY + 1], vec![0; 52]];
        let mut mixed = fixture();
        mixed[52 + 32 + 24..52 + 32 + 28].copy_from_slice(&0u32.to_le_bytes());
        invalid.push(mixed);
        let mut overlap = fixture();
        overlap[52 + 32 + 12..52 + 32 + 16].copy_from_slice(&0x8b000800u32.to_le_bytes());
        invalid.push(overlap);
        let mut outside = fixture();
        outside[52 + 32 + 12..52 + 32 + 16].copy_from_slice(&0x94800000u32.to_le_bytes());
        invalid.push(outside);
        for input in invalid {
            let mut ram = Ram::new();
            assert!(prepare(&mut ram, 0x80080000, &input).is_err());
            assert_eq!(ram.writes, 0);
        }
    }
    #[test]
    fn dirty_mapping_is_not_overwritten() {
        for i in [0, PAGE, PAGE + CAPACITY - 1, MAPPING_SIZE - 1] {
            let mut ram = Ram::new();
            ram.bytes[i] = 1;
            assert_eq!(
                prepare(&mut ram, 0x80080000, &fixture()).err(),
                Some(Error::DirtyMapping)
            );
            assert_eq!(ram.writes, 0);
        }
    }

    #[test]
    fn reverify_does_not_write_or_repair_a_previously_staged_buffer() {
        let mut ram = Ram::new();
        let input = fixture();
        prepare(&mut ram, 0x80080000, &input).unwrap();
        let writes = ram.writes;
        verify(&mut ram, 0x80080000, &input).unwrap();
        assert_eq!(ram.writes, writes);
        ram.bytes[0] ^= 1;
        assert_eq!(
            verify(&mut ram, 0x80080000, &input).err(),
            Some(Error::Guard)
        );
        assert_eq!(ram.writes, writes);
    }
    #[test]
    fn corruption_in_payload_padding_or_either_guard_fails_without_retry() {
        let input = fixture();
        for (i, error) in [
            (0, Error::Guard),
            (PAGE - 1, Error::Guard),
            (PAGE, Error::Copy),
            (PAGE + input.len() - 1, Error::Copy),
            (PAGE + input.len(), Error::Padding),
            (PAGE + CAPACITY - 1, Error::Padding),
            (PAGE + CAPACITY, Error::Guard),
            (MAPPING_SIZE - 1, Error::Guard),
        ] {
            let mut ram = Ram::new();
            ram.corrupt_after_sync = Some(i);
            assert_eq!(prepare(&mut ram, 0x80080000, &input).err(), Some(error));
            assert_eq!(ram.writes, 2 * PAGE + input.len());
        }
    }

    #[test]
    fn diagnostic_reads_all_regions_despite_payload_mismatch_and_never_repairs() {
        let input = fixture();
        let mut ram = Ram::new();
        prepare(&mut ram, 0x80080000, &input).unwrap();
        let writes = ram.writes;
        ram.bytes[0] = 0;
        ram.bytes[PAGE - 1] = 0;
        ram.bytes[PAGE + CAPACITY] = 0;
        ram.bytes[PAGE + input.len()] = 1;
        ram.bytes[PAGE] = 0;
        ram.bytes[PAGE + input.len() - 1] = 1;
        let r = inspect(&mut ram, 0x80080000, &input).unwrap();
        assert_eq!(
            (r.low_guard_bad, r.high_guard_bad, r.padding_bad),
            (2, 1, 1)
        );
        assert_eq!(
            (r.payload_diff, r.first_diff, r.last_diff),
            (2, Some(0), Some(input.len() as u32 - 1))
        );
        assert_eq!(r.expected_words[0], 0x464c457f);
        assert_eq!(r.actual_words[0], 0x464c4500);
        assert_ne!(r.input_crc32, r.actual_crc32);
        assert_eq!(r.unstable_bytes, 0);
        assert_eq!(r.strict_error(), Error::Copy.code());
        assert_eq!(ram.writes, writes);
        // Integrity failures in other regions remain terminal when payload is exact.
        ram.bytes[PAGE..PAGE + input.len()].copy_from_slice(&input);
        assert_eq!(
            inspect(&mut ram, 0x80080000, &input)
                .unwrap()
                .strict_error(),
            Error::Guard.code()
        );
        ram.bytes[..PAGE].fill(CANARY);
        ram.bytes[PAGE + CAPACITY..].fill(CANARY);
        assert_eq!(
            inspect(&mut ram, 0x80080000, &input)
                .unwrap()
                .strict_error(),
            Error::Padding.code()
        );
    }

    #[test]
    fn diagnostic_handles_unchanged_zeroed_and_last_byte_only_payloads() {
        assert_eq!(diagnostic_crc32(b"123456789"), 0xcbf43926);
        let input = fixture();
        let mut ram = Ram::new();
        prepare(&mut ram, 0x80080000, &input).unwrap();
        let r = inspect(&mut ram, 0x80080000, &input).unwrap();
        assert_eq!(r.strict_error(), 0);
        assert_eq!((r.first_diff, r.last_diff), (None, None));
        assert_eq!(r.input_crc32, r.actual_crc32);
        ram.bytes[PAGE + input.len() - 1] = 0xff;
        let r = inspect(&mut ram, 0x80080000, &input).unwrap();
        assert_eq!(r.first_diff, Some(input.len() as u32 - 1));
        assert_eq!(r.actual_words, [0xff, 0]);
        ram.bytes[PAGE..PAGE + input.len()].fill(0);
        let r = inspect(&mut ram, 0x80080000, &input).unwrap();
        assert_eq!(r.payload_zero, input.len() as u32);
        assert_eq!(
            r.payload_diff,
            input.iter().filter(|v| **v != 0).count() as u32
        );
        assert_eq!(
            (r.low_guard_bad, r.high_guard_bad, r.padding_bad),
            (0, 0, 0)
        );
        assert_ne!(r.actual_parse_error, 0);
    }

    #[test]
    fn diagnostic_detects_second_read_drift_and_refuses_bad_input_before_reads() {
        struct Drift {
            ram: Ram,
            reads: usize,
            drift: bool,
        }
        impl Memory for Drift {
            fn read_byte(&mut self, i: usize) -> u8 {
                self.reads += 1;
                if self.drift && self.reads == MAPPING_SIZE + 1 {
                    self.ram.bytes[PAGE] ^= 1;
                }
                self.ram.bytes[i]
            }
            fn write_byte(&mut self, _: usize, _: u8) {
                panic!("diagnostic must not write");
            }
            fn sync(&mut self) {}
        }
        let input = fixture();
        let mut ram = Ram::new();
        prepare(&mut ram, 0x80080000, &input).unwrap();
        let mut io = Drift {
            ram,
            reads: 0,
            drift: true,
        };
        let r = inspect(&mut io, 0x80080000, &input).unwrap();
        assert_eq!(r.payload_diff, 0);
        assert_eq!(r.unstable_bytes, 1);
        assert_eq!(r.strict_error(), 9);
        assert_eq!(io.reads, 2 * MAPPING_SIZE);
        io.reads = 0;
        assert_eq!(inspect(&mut io, 0, &input), Err(Error::Address));
        assert_eq!(io.reads, 0);
        assert!(inspect(&mut io, 0x80080000, &[]).is_err());
        assert_eq!(io.reads, 0);
    }
}
