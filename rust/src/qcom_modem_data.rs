//! First bounded firmware-data transfer, not an executable-image loader.
//! Only MDT segment23 (R, no X),45 bytes at0x93200000, is admitted. A successful
//! MEM_SETUP session is consumed before any page access. The caller must own
//! the one fixed device mapping and hold the platform requests until restart.
//! This empirically tests that access; a zero SCM status is not itself proof
//! of AP ownership. There is no PAS call, retry, reset, execution or DMA here.

use crate::qcom_pas_mem_setup::{MemSetupOnce, Report as MemReport};
use crate::qcom_pas_metadata::{diagnostic_crc32, validate_image};

pub const PAGE_PHYSICAL: u64 = 0x93200000;
pub const PAGE_BYTES: usize = 4096;
pub const DATA_BYTES: usize = 45;
pub trait Page {
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
    OutsidePayload,
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
    ReadPage,
    WriteData,
    Verify,
    Done,
    Failed,
}
impl Phase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "DATA COPY READY",
            Self::ReadPage => "READ MODEM PAGE",
            Self::WriteData => "WRITE DATA SEGMENT",
            Self::Verify => "VERIFY MODEM PAGE",
            Self::Done => "DATA COPY PASS",
            Self::Failed => "DATA COPY STOPPED",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub memory_setup: MemReport,
    pub phase: Phase,
    pub writes: u32,
    pub payload_bad: u32,
    pub outside_bad: u32,
    pub unstable_bytes: u32,
    pub first_bad: Option<u32>,
    pub source_crc32: u32,
    pub payload_crc32: Option<u32>,
    pub page_before_crc32: Option<u32>,
    pub page_after_crc32: Option<u32>,
    pub error: Option<Error>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.phase == Phase::Done
            && self.error.is_none()
            && self.writes == 45
            && self.payload_bad == 0
            && self.outside_bad == 0
            && self.unstable_bytes == 0
            && self.memory_setup.passed()
    }
}
pub struct StageOnce {
    source: [u8; DATA_BYTES],
    report: Report,
}
impl StageOnce {
    pub fn after_setup(memory: MemSetupOnce, mdt: &[u8], source: &[u8]) -> Result<Self, Error> {
        let image = validate_image(mdt).map_err(|_| Error::SourcePlan)?;
        let segment = image
            .segments
            .iter()
            .flatten()
            .find(|s| s.index == 23)
            .ok_or(Error::SourcePlan)?;
        if image.program_header_count != 31
            || image.loadable_segment_count != 29
            || image.linked_min_address != 0x8b000000
            || image.linked_max_address != 0x94800000
            || segment.linked_address as u64 != PAGE_PHYSICAL
            || segment.file_size != 45
            || segment.memory_size != 45
            || segment.flags != 0x08000004
            || source.len() != 45
        {
            return Err(Error::SourcePlan);
        }
        let memory_setup = memory.into_completed().map_err(|_| Error::MemNotPassed)?;
        let mut bytes = [0; DATA_BYTES];
        bytes.copy_from_slice(source);
        Ok(Self {
            source: bytes,
            report: Report {
                memory_setup,
                phase: Phase::Ready,
                writes: 0,
                payload_bad: 0,
                outside_bad: 0,
                unstable_bytes: 0,
                first_bad: None,
                source_crc32: diagnostic_crc32(source),
                payload_crc32: None,
                page_before_crc32: None,
                page_after_crc32: None,
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
        page: &mut impl Page,
        mut before: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Ready {
            return Err(self.fail(Error::WrongStage));
        }
        self.report.phase = Phase::ReadPage;
        before(&self.report);
        let mut original = [0; PAGE_BYTES];
        page.sync();
        for (i, byte) in original.iter_mut().enumerate() {
            *byte = page.read_byte(i);
        }
        self.report.page_before_crc32 = Some(diagnostic_crc32(&original));
        self.report.phase = Phase::WriteData;
        before(&self.report);
        // Exactly the declared file bytes. p_memsz==p_filesz, so there is no
        // BSS tail to zero; leave the other4051 bytes of the page untouched.
        for (i, byte) in self.source.iter().enumerate() {
            page.write_byte(i, *byte);
            self.report.writes += 1;
        }
        page.sync();
        self.report.phase = Phase::Verify;
        before(&self.report);
        let mut snapshot = [0; PAGE_BYTES];
        for (i, byte) in snapshot.iter_mut().enumerate() {
            *byte = page.read_byte(i);
        }
        self.report.payload_crc32 = Some(diagnostic_crc32(&snapshot[..DATA_BYTES]));
        self.report.page_after_crc32 = Some(diagnostic_crc32(&snapshot));
        for i in 0..PAGE_BYTES {
            let expected = if i < DATA_BYTES {
                self.source[i]
            } else {
                original[i]
            };
            if snapshot[i] != expected {
                self.report.first_bad.get_or_insert(i as u32);
                if i < DATA_BYTES {
                    self.report.payload_bad += 1;
                } else {
                    self.report.outside_bad += 1;
                }
            }
        }
        page.sync();
        for (i, byte) in snapshot.iter().enumerate() {
            self.report.unstable_bytes += u32::from(page.read_byte(i) != *byte);
        }
        if self.report.payload_bad != 0 {
            return Err(self.fail(Error::Payload));
        }
        if self.report.outside_bad != 0 {
            return Err(self.fail(Error::OutsidePayload));
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
        bytes: [u8; PAGE_BYTES],
        writes: std::vec::Vec<usize>,
        reads: usize,
        corrupt: Option<usize>,
        drift: bool,
    }
    impl Ram {
        fn new() -> Self {
            Self {
                bytes: core::array::from_fn(|i| (i % 251) as u8),
                writes: vec![],
                reads: 0,
                corrupt: None,
                drift: false,
            }
        }
    }
    impl Page for Ram {
        fn read_byte(&mut self, i: usize) -> u8 {
            self.reads += 1;
            if self.drift && self.reads == 2 * PAGE_BYTES + 1 {
                self.bytes[4000] ^= 1;
            }
            self.bytes[i]
        }
        fn write_byte(&mut self, i: usize, v: u8) {
            assert!(i < 45);
            self.writes.push(i);
            self.bytes[i] = v;
        }
        fn sync(&mut self) {
            if self.writes.len() == 45 {
                if let Some(i) = self.corrupt.take() {
                    self.bytes[i] ^= 1;
                }
            }
        }
    }
    fn mdt() -> std::vec::Vec<u8> {
        let mut b = vec![0u8; 52 + 31 * 32];
        b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        b[18..20].copy_from_slice(&164u16.to_le_bytes());
        b[20..24].copy_from_slice(&1u32.to_le_bytes());
        b[28..32].copy_from_slice(&52u32.to_le_bytes());
        b[42..44].copy_from_slice(&32u16.to_le_bytes());
        b[44..46].copy_from_slice(&31u16.to_le_bytes());
        //29 nonoverlapping relocatable loads; index23 is the only fixed target.
        for n in 2..31 {
            let at = 52 + n * 32;
            let addr = if n == 23 {
                0x93200000
            } else if n == 30 {
                0x947ff000
            } else {
                0x8b000000 + (n as u32 - 2) * 0x1000
            };
            for (offset, value) in [
                (0, 1u32),
                (12, addr),
                (16, if n == 23 { 45 } else { 1 }),
                (20, if n == 23 { 45 } else { 4096 }),
                (24, 0x08000004),
            ] {
                b[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        b
    }
    fn ready() -> StageOnce {
        StageOnce::after_setup(
            crate::qcom_pas_mem_setup::tests::completed_setup(),
            &mdt(),
            &[0x55; 45],
        )
        .unwrap()
    }
    #[test]
    fn copies_exactly_45_bytes_and_preserves_every_other_byte_without_retry() {
        let mut r = Ram::new();
        let original = r.bytes;
        let mut s = ready();
        let mut phases = vec![];
        s.run(&mut r, |p| phases.push(p.phase)).unwrap();
        assert!(s.report().passed());
        assert_eq!(phases, [Phase::ReadPage, Phase::WriteData, Phase::Verify]);
        assert_eq!(r.writes, (0..45).collect::<std::vec::Vec<_>>());
        assert_eq!(&r.bytes[..45], &[0x55; 45]);
        assert_eq!(&r.bytes[45..], &original[45..]);
        assert_eq!(r.reads, 3 * PAGE_BYTES);
        assert_eq!(s.run(&mut r, |_| {}), Err(Error::WrongStage));
        assert_eq!(r.writes.len(), 45);
    }
    #[test]
    fn wrong_segment_or_source_length_is_refused_without_page_access() {
        for offset in [
            52 + 23 * 32 + 12,
            52 + 23 * 32 + 16,
            52 + 23 * 32 + 20,
            52 + 23 * 32 + 24,
        ] {
            let mut b = mdt();
            b[offset] ^= 1;
            assert!(matches!(
                StageOnce::after_setup(
                    crate::qcom_pas_mem_setup::tests::completed_setup(),
                    &b,
                    &[0; 45]
                ),
                Err(Error::SourcePlan)
            ));
        }
        for length in [0, 44, 46] {
            assert!(matches!(
                StageOnce::after_setup(
                    crate::qcom_pas_mem_setup::tests::completed_setup(),
                    &mdt(),
                    &vec![0; length]
                ),
                Err(Error::SourcePlan)
            ));
        }
    }
    #[test]
    fn payload_tail_and_read_drift_fail_after_one_transfer_without_repairs() {
        for (at, error) in [
            (0, Error::Payload),
            (44, Error::Payload),
            (45, Error::OutsidePayload),
            (4095, Error::OutsidePayload),
        ] {
            let mut ram = Ram::new();
            ram.corrupt = Some(at);
            let mut s = ready();
            assert_eq!(s.run(&mut ram, |_| {}), Err(error));
            assert_eq!(s.report().first_bad, Some(at as u32));
            assert_eq!(ram.writes.len(), 45);
            assert!(s.run(&mut ram, |_| {}).is_err());
            assert_eq!(ram.writes.len(), 45);
        }
        let mut ram = Ram::new();
        ram.drift = true;
        let mut s = ready();
        assert_eq!(s.run(&mut ram, |_| {}), Err(Error::Unstable));
        assert_eq!(s.report().unstable_bytes, 1);
    }
}
