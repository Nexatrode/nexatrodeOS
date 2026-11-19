//! Fixed segment14 experiment: two pages of non-executable data and BSS.
//! A successful PAS memory-setup session precedes every reserved-memory access.
//! Two adjacent read-only guard pages must not overlap any MDT load. No secure
//! call, executable segment, reset, repair, retry or DMA is implemented here.
use crate::qcom_pas_mem_setup::{MemSetupOnce, Report as MemReport};
use crate::qcom_pas_metadata::{diagnostic_crc32, validate_image};

pub const PAGE: usize = 4096;
pub const WINDOW_PHYSICAL: u64 = 0x8b77f000;
pub const SEGMENT_PHYSICAL: u64 = 0x8b780000;
pub const WINDOW_BYTES: usize = 4 * PAGE;
pub const FILE_BYTES: usize = 4160;
pub const MEMORY_BYTES: usize = 8192;
pub const ZERO_BYTES: usize = MEMORY_BYTES - FILE_BYTES;
pub const DATA_START: usize = PAGE;
pub const DATA_END: usize = PAGE + MEMORY_BYTES;

pub trait Memory {
    fn read_byte(&mut self, offset: usize) -> u8;
    fn write_byte(&mut self, offset: usize, value: u8);
    fn sync(&mut self);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    SourcePlan,
    MemNotPassed,
    WrongStage,
    Payload,
    ZeroTail,
    Guard,
    Unstable,
}
impl Error {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Ready,
    ReadGuards,
    Copy,
    Zero,
    Verify,
    Done,
    Failed,
}
impl Phase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "SEGMENT LOAD READY",
            Self::ReadGuards => "READ SEGMENT GUARDS",
            Self::Copy => "COPY SEGMENT DATA",
            Self::Zero => "ZERO SEGMENT TAIL",
            Self::Verify => "VERIFY SEGMENT",
            Self::Done => "SEGMENT LOAD PASS",
            Self::Failed => "SEGMENT LOAD STOPPED",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub memory_setup: MemReport,
    pub phase: Phase,
    pub file_writes: u32,
    pub zero_writes: u32,
    pub payload_bad: u32,
    pub zero_bad: u32,
    pub guard_bad: u32,
    pub unstable_bytes: u32,
    pub first_bad: Option<u32>,
    pub source_crc32: u32,
    pub payload_crc32: Option<u32>,
    pub zero_crc32: Option<u32>,
    pub guard_before_crc32: Option<u32>,
    pub guard_after_crc32: Option<u32>,
    pub error: Option<Error>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.phase == Phase::Done
            && self.error.is_none()
            && self.file_writes == FILE_BYTES as u32
            && self.zero_writes == ZERO_BYTES as u32
            && self.payload_bad == 0
            && self.zero_bad == 0
            && self.guard_bad == 0
            && self.unstable_bytes == 0
            && self.memory_setup.passed()
    }
}
pub struct SegmentOnce {
    source: [u8; FILE_BYTES],
    report: Report,
}
impl SegmentOnce {
    pub fn after_setup(memory: MemSetupOnce, mdt: &[u8], source: &[u8]) -> Result<Self, Error> {
        let image = validate_image(mdt).map_err(|_| Error::SourcePlan)?;
        let segment = image
            .segments
            .iter()
            .flatten()
            .find(|s| s.index == 14)
            .ok_or(Error::SourcePlan)?;
        if image.program_header_count != 31
            || image.loadable_segment_count != 29
            || image.linked_min_address != 0x8b000000
            || image.linked_max_address != 0x94800000
            || segment.linked_address as u64 != SEGMENT_PHYSICAL
            || segment.file_size as usize != FILE_BYTES
            || segment.memory_size as usize != MEMORY_BYTES
            || segment.flags != 0x08000006
            || source.len() != FILE_BYTES
        {
            return Err(Error::SourcePlan);
        }
        for other in image.segments.iter().flatten().filter(|s| s.index != 14) {
            let start = other.linked_address as u64;
            let end = start + other.memory_size as u64;
            if start < WINDOW_PHYSICAL + WINDOW_BYTES as u64 && WINDOW_PHYSICAL < end {
                return Err(Error::SourcePlan);
            }
        }
        let memory_setup = memory.into_completed().map_err(|_| Error::MemNotPassed)?;
        let mut bytes = [0; FILE_BYTES];
        bytes.copy_from_slice(source);
        Ok(Self {
            source: bytes,
            report: Report {
                memory_setup,
                phase: Phase::Ready,
                file_writes: 0,
                zero_writes: 0,
                payload_bad: 0,
                zero_bad: 0,
                guard_bad: 0,
                unstable_bytes: 0,
                first_bad: None,
                source_crc32: diagnostic_crc32(source),
                payload_crc32: None,
                zero_crc32: None,
                guard_before_crc32: None,
                guard_after_crc32: None,
                error: None,
            },
        })
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
    fn fail(&mut self, error: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error.get_or_insert(error);
        error
    }
    pub fn run(
        &mut self,
        memory: &mut impl Memory,
        mut progress: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Ready {
            return Err(self.fail(Error::WrongStage));
        }
        self.report.phase = Phase::ReadGuards;
        progress(&self.report);
        let mut guards = [0; 2 * PAGE];
        memory.sync();
        for i in 0..PAGE {
            guards[i] = memory.read_byte(i);
            guards[PAGE + i] = memory.read_byte(DATA_END + i);
        }
        self.report.guard_before_crc32 = Some(diagnostic_crc32(&guards));
        self.report.phase = Phase::Copy;
        progress(&self.report);
        for (i, byte) in self.source.iter().enumerate() {
            memory.write_byte(DATA_START + i, *byte);
            self.report.file_writes += 1;
        }
        memory.sync();
        self.report.phase = Phase::Zero;
        progress(&self.report);
        for i in FILE_BYTES..MEMORY_BYTES {
            memory.write_byte(DATA_START + i, 0);
            self.report.zero_writes += 1;
        }
        memory.sync();
        self.report.phase = Phase::Verify;
        progress(&self.report);
        let mut snapshot = [0; WINDOW_BYTES];
        for (i, byte) in snapshot.iter_mut().enumerate() {
            *byte = memory.read_byte(i);
        }
        self.report.payload_crc32 = Some(diagnostic_crc32(
            &snapshot[DATA_START..DATA_START + FILE_BYTES],
        ));
        self.report.zero_crc32 = Some(diagnostic_crc32(
            &snapshot[DATA_START + FILE_BYTES..DATA_END],
        ));
        for (i, actual) in snapshot.iter().enumerate() {
            let expected = if i < DATA_START {
                guards[i]
            } else if i < DATA_START + FILE_BYTES {
                self.source[i - DATA_START]
            } else if i < DATA_END {
                0
            } else {
                guards[PAGE + i - DATA_END]
            };
            if *actual != expected {
                self.report.first_bad.get_or_insert(i as u32);
                if !(DATA_START..DATA_END).contains(&i) {
                    self.report.guard_bad += 1;
                } else if i < DATA_START + FILE_BYTES {
                    self.report.payload_bad += 1;
                } else {
                    self.report.zero_bad += 1;
                }
            }
        }
        guards[..PAGE].copy_from_slice(&snapshot[..PAGE]);
        guards[PAGE..].copy_from_slice(&snapshot[DATA_END..]);
        self.report.guard_after_crc32 = Some(diagnostic_crc32(&guards));
        memory.sync();
        for (i, byte) in snapshot.iter().enumerate() {
            self.report.unstable_bytes += u32::from(memory.read_byte(i) != *byte);
        }
        if self.report.payload_bad != 0 {
            return Err(self.fail(Error::Payload));
        }
        if self.report.zero_bad != 0 {
            return Err(self.fail(Error::ZeroTail));
        }
        if self.report.guard_bad != 0 {
            return Err(self.fail(Error::Guard));
        }
        if self.report.unstable_bytes != 0 {
            return Err(self.fail(Error::Unstable));
        }
        self.report.phase = Phase::Done;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Ram {
        bytes: [u8; WINDOW_BYTES],
        writes: std::vec::Vec<usize>,
        reads: usize,
        corrupt: Option<usize>,
        drift: bool,
    }
    impl Ram {
        fn new() -> Self {
            Self {
                bytes: [0xa5; WINDOW_BYTES],
                writes: vec![],
                reads: 0,
                corrupt: None,
                drift: false,
            }
        }
    }
    impl Memory for Ram {
        fn read_byte(&mut self, i: usize) -> u8 {
            assert!(i < WINDOW_BYTES);
            self.reads += 1;
            if self.drift && self.reads == 2 * PAGE + WINDOW_BYTES + 1 {
                self.bytes[WINDOW_BYTES - 1] ^= 1;
            }
            self.bytes[i]
        }
        fn write_byte(&mut self, i: usize, value: u8) {
            assert!((DATA_START..DATA_END).contains(&i));
            self.writes.push(i);
            self.bytes[i] = value;
        }
        fn sync(&mut self) {
            if self.writes.len() == MEMORY_BYTES {
                if let Some(i) = self.corrupt.take() {
                    self.bytes[i] ^= 1;
                }
            }
        }
    }
    fn mdt() -> std::vec::Vec<u8> {
        let mut b = vec![0; 52 + 31 * 32];
        b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        b[18..20].copy_from_slice(&164u16.to_le_bytes());
        b[20..24].copy_from_slice(&1u32.to_le_bytes());
        b[28..32].copy_from_slice(&52u32.to_le_bytes());
        b[42..44].copy_from_slice(&32u16.to_le_bytes());
        b[44..46].copy_from_slice(&31u16.to_le_bytes());
        for n in 2..31 {
            let address = if n == 14 {
                SEGMENT_PHYSICAL as u32
            } else if n == 30 {
                0x947ff000
            } else {
                0x8b000000 + (n as u32 - 2) * 4096
            };
            for (offset, value) in [
                (0, 1u32),
                (12, address),
                (16, if n == 14 { FILE_BYTES as u32 } else { 1 }),
                (20, if n == 14 { MEMORY_BYTES as u32 } else { 4096 }),
                (24, if n == 14 { 0x08000006 } else { 0x08000004 }),
            ] {
                let at = 52 + n * 32 + offset;
                b[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        b
    }
    fn ready() -> SegmentOnce {
        SegmentOnce::after_setup(
            crate::qcom_pas_mem_setup::tests::completed_setup(),
            &mdt(),
            &[0x5a; FILE_BYTES],
        )
        .unwrap()
    }
    #[test]
    fn crosses_page_boundary_and_zeros_exact_bss_without_touching_guards() {
        let mut ram = Ram::new();
        let mut s = ready();
        let mut phases = vec![];
        s.run(&mut ram, |r| phases.push(r.phase)).unwrap();
        assert!(s.report().passed());
        assert_eq!(
            phases,
            [Phase::ReadGuards, Phase::Copy, Phase::Zero, Phase::Verify]
        );
        assert_eq!(
            ram.writes,
            (DATA_START..DATA_END).collect::<std::vec::Vec<_>>()
        );
        assert_eq!(ram.reads, 2 * PAGE + 2 * WINDOW_BYTES);
        assert!(ram.bytes[..PAGE].iter().all(|v| *v == 0xa5));
        assert!(ram.bytes[DATA_END..].iter().all(|v| *v == 0xa5));
        assert!(
            ram.bytes[DATA_START..DATA_START + FILE_BYTES]
                .iter()
                .all(|v| *v == 0x5a)
        );
        assert!(
            ram.bytes[DATA_START + FILE_BYTES..DATA_END]
                .iter()
                .all(|v| *v == 0)
        );
        assert_eq!(s.run(&mut ram, |_| {}), Err(Error::WrongStage));
        assert_eq!(ram.writes.len(), MEMORY_BYTES);
    }
    #[test]
    fn changed_header_source_length_or_any_guard_load_overlap_is_rejected() {
        for offset in [12, 16, 20, 24] {
            let mut b = mdt();
            b[52 + 14 * 32 + offset] ^= 1;
            assert!(matches!(
                SegmentOnce::after_setup(
                    crate::qcom_pas_mem_setup::tests::completed_setup(),
                    &b,
                    &[0; FILE_BYTES]
                ),
                Err(Error::SourcePlan)
            ));
        }
        for length in [0, FILE_BYTES - 1, FILE_BYTES + 1] {
            assert!(matches!(
                SegmentOnce::after_setup(
                    crate::qcom_pas_mem_setup::tests::completed_setup(),
                    &mdt(),
                    &vec![0; length]
                ),
                Err(Error::SourcePlan)
            ));
        }
        for addr in [
            WINDOW_PHYSICAL as u32,
            (SEGMENT_PHYSICAL + MEMORY_BYTES as u64) as u32,
        ] {
            let mut b = mdt();
            b[52 + 13 * 32 + 12..52 + 13 * 32 + 16].copy_from_slice(&addr.to_le_bytes());
            assert!(matches!(
                SegmentOnce::after_setup(
                    crate::qcom_pas_mem_setup::tests::completed_setup(),
                    &b,
                    &[0; FILE_BYTES]
                ),
                Err(Error::SourcePlan)
            ));
        }
    }
    #[test]
    fn payload_bss_guard_or_drift_failure_is_terminal_without_repairs() {
        for (at, error) in [
            (DATA_START, Error::Payload),
            (DATA_START + FILE_BYTES - 1, Error::Payload),
            (DATA_START + FILE_BYTES, Error::ZeroTail),
            (DATA_END - 1, Error::ZeroTail),
            (0, Error::Guard),
            (WINDOW_BYTES - 1, Error::Guard),
        ] {
            let mut ram = Ram::new();
            ram.corrupt = Some(at);
            let mut s = ready();
            assert_eq!(s.run(&mut ram, |_| {}), Err(error));
            assert_eq!(s.report().first_bad, Some(at as u32));
            assert!(s.run(&mut ram, |_| {}).is_err());
            assert_eq!(ram.writes.len(), MEMORY_BYTES);
        }
        let mut ram = Ram::new();
        ram.drift = true;
        let mut s = ready();
        assert_eq!(s.run(&mut ram, |_| {}), Err(Error::Unstable));
        assert_eq!(s.report().unstable_bytes, 1);
    }
}
