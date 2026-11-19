//! Complete pinned MDT segment staging, stopping before AUTH_AND_RESET.
//! Every load is RW/NX in one fixed modem reservation. No branch, reset,
//! secure call, radio, DMA, retry or repair occurs here. Gaps are snapshotted
//! into separate owned RAM and compared byte-for-byte after two verification
//! passes. All reserved-memory access requires a completed MEM_SETUP session.
use crate::qcom_pas_mem_setup::{MemSetupOnce, Report as MemReport};
use crate::qcom_pas_metadata::validate_image;

pub const POOL_BASE: u64 = 0x8b000000;
pub const POOL_BYTES: usize = 0x09800000;
pub const LOAD_COUNT: usize = 29;
pub const FILE_BYTES: usize = 68_393_962;
pub const MEMORY_BYTES: usize = 157_744_117;
pub const ZERO_BYTES: usize = MEMORY_BYTES - FILE_BYTES;
pub const GAP_BYTES: usize = POOL_BYTES - MEMORY_BYTES;
pub const SNAPSHOT_BYTES: usize = 0x200000;
pub const LOAD_MASK: u32 = 0x7ffffffc;
const CHUNK: usize = 0x1000000;
#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub index: u16,
    pub bytes: &'a [u8],
}
#[derive(Clone, Copy)]
struct Load<'a> {
    index: u16,
    offset: usize,
    memory_bytes: usize,
    source: &'a [u8],
}
#[derive(Clone, Copy)]
struct Gap {
    offset: usize,
    bytes: usize,
}
pub struct Plan<'a> {
    loads: [Load<'a>; LOAD_COUNT],
    gaps: [Gap; LOAD_COUNT + 1],
    gap_count: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    SourcePlan,
    MemNotPassed,
    WrongStage,
    Payload,
    ZeroTail,
    Gap,
}
impl Error {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}
impl<'a> Plan<'a> {
    pub fn new(mdt: &[u8], sources: &[Source<'a>]) -> Result<Self, Error> {
        let image = validate_image(mdt).map_err(|_| Error::SourcePlan)?;
        if image.program_header_count != 31
            || image.loadable_segment_count != LOAD_COUNT
            || image.linked_min_address as u64 != POOL_BASE
            || image.linked_max_address as u64 != POOL_BASE + POOL_BYTES as u64
            || sources.len() != LOAD_COUNT
        {
            return Err(Error::SourcePlan);
        }
        let empty = Load {
            index: 0,
            offset: 0,
            memory_bytes: 0,
            source: &[],
        };
        let mut loads = [empty; LOAD_COUNT];
        let mut mask = 0;
        for (n, segment) in image.segments.iter().flatten().enumerate() {
            if !(2..=30).contains(&segment.index) {
                return Err(Error::SourcePlan);
            }
            let mut matches = sources.iter().filter(|s| s.index == segment.index);
            let source = matches.next().ok_or(Error::SourcePlan)?;
            if matches.next().is_some() || source.bytes.len() != segment.file_size as usize {
                return Err(Error::SourcePlan);
            }
            let offset = segment.linked_address as usize - POOL_BASE as usize;
            let memory_bytes = segment.memory_size as usize;
            if offset
                .checked_add(memory_bytes)
                .is_none_or(|end| end > POOL_BYTES)
            {
                return Err(Error::SourcePlan);
            }
            loads[n] = Load {
                index: segment.index,
                offset,
                memory_bytes,
                source: source.bytes,
            };
            mask |= 1 << segment.index;
        }
        // Fixed29-element ordering avoids pulling a general sorter and its
        // memmove runtime dependency into this freestanding probe.
        for i in 0..LOAD_COUNT {
            for j in i + 1..LOAD_COUNT {
                if loads[j].offset < loads[i].offset {
                    loads.swap(i, j);
                }
            }
        }
        let mut gaps = [Gap {
            offset: 0,
            bytes: 0,
        }; LOAD_COUNT + 1];
        let (mut cursor, mut gap_count, mut total_gaps, mut total_files, mut total_memory) =
            (0, 0, 0, 0, 0);
        for load in &loads {
            if load.offset < cursor {
                return Err(Error::SourcePlan);
            }
            if load.offset != cursor {
                gaps[gap_count] = Gap {
                    offset: cursor,
                    bytes: load.offset - cursor,
                };
                total_gaps += load.offset - cursor;
                gap_count += 1;
            }
            cursor = load.offset + load.memory_bytes;
            total_files += load.source.len();
            total_memory += load.memory_bytes;
        }
        if cursor < POOL_BYTES {
            gaps[gap_count] = Gap {
                offset: cursor,
                bytes: POOL_BYTES - cursor,
            };
            total_gaps += POOL_BYTES - cursor;
            gap_count += 1;
        }
        if mask != LOAD_MASK
            || total_files != FILE_BYTES
            || total_memory != MEMORY_BYTES
            || total_gaps != GAP_BYTES
            || total_gaps > SNAPSHOT_BYTES
        {
            return Err(Error::SourcePlan);
        }
        Ok(Self {
            loads,
            gaps,
            gap_count,
        })
    }
}
/// Pool word operations must be four-byte aligned. Snapshot operations access
/// a distinct owned RAM region, never the modem pool or metadata allocation.
pub trait Memory {
    fn read_word(&mut self, offset: usize) -> u32;
    fn write_word(&mut self, offset: usize, value: u32);
    fn read_byte(&mut self, offset: usize) -> u8;
    fn write_byte(&mut self, offset: usize, value: u8);
    fn snapshot_read(&mut self, offset: usize) -> u8;
    fn snapshot_write(&mut self, offset: usize, value: u8);
    fn sync(&mut self);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Ready,
    SnapshotGaps,
    Copy,
    Zero,
    VerifyFirst,
    VerifySecond,
    VerifyGaps,
    Done,
    Failed,
}
impl Phase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "IMAGE STAGE READY",
            Self::SnapshotGaps => "SNAPSHOT IMAGE GAPS",
            Self::Copy => "COPY IMAGE SEGMENT",
            Self::Zero => "ZERO IMAGE BSS",
            Self::VerifyFirst => "VERIFY IMAGE FIRST",
            Self::VerifySecond => "VERIFY IMAGE SECOND",
            Self::VerifyGaps => "VERIFY IMAGE GAPS",
            Self::Done => "IMAGE STAGE PASS",
            Self::Failed => "IMAGE STAGE STOPPED",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub memory_setup: MemReport,
    pub phase: Phase,
    pub current_index: Option<u16>,
    pub loaded_mask: u32,
    pub verified_masks: [u32; 2],
    pub file_writes: u32,
    pub zero_writes: u32,
    pub checked_bytes: [u32; 2],
    pub snapshot_bytes: u32,
    pub gap_checked: u32,
    pub payload_bad: u32,
    pub zero_bad: u32,
    pub gap_bad: u32,
    pub first_bad: Option<u32>,
    pub error: Option<Error>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.phase == Phase::Done
            && self.error.is_none()
            && self.loaded_mask == LOAD_MASK
            && self.verified_masks == [LOAD_MASK; 2]
            && self.file_writes == FILE_BYTES as u32
            && self.zero_writes == ZERO_BYTES as u32
            && self.checked_bytes == [MEMORY_BYTES as u32; 2]
            && self.snapshot_bytes == GAP_BYTES as u32
            && self.gap_checked == GAP_BYTES as u32
            && self.payload_bad == 0
            && self.zero_bad == 0
            && self.gap_bad == 0
            && self.memory_setup.passed()
    }
}
pub struct ImageOnce<'a> {
    plan: Plan<'a>,
    report: Report,
}
impl<'a> ImageOnce<'a> {
    pub fn after_setup(memory: MemSetupOnce, plan: Plan<'a>) -> Result<Self, Error> {
        let memory_setup = memory.into_completed().map_err(|_| Error::MemNotPassed)?;
        Ok(Self {
            plan,
            report: Report {
                memory_setup,
                phase: Phase::Ready,
                current_index: None,
                loaded_mask: 0,
                verified_masks: [0; 2],
                file_writes: 0,
                zero_writes: 0,
                checked_bytes: [0; 2],
                snapshot_bytes: 0,
                gap_checked: 0,
                payload_bad: 0,
                zero_bad: 0,
                gap_bad: 0,
                first_bad: None,
                error: None,
            },
        })
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
    pub(crate) fn into_completed(self) -> Result<Report, Error> {
        if !self.report.passed() {
            return Err(Error::WrongStage);
        }
        Ok(self.report)
    }
    fn fail(&mut self, error: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error.get_or_insert(error);
        error
    }
    fn bad_byte(&mut self, offset: usize, file: bool) -> Error {
        self.report.first_bad.get_or_insert(offset as u32);
        if file {
            self.report.payload_bad += 1;
            self.fail(Error::Payload)
        } else {
            self.report.zero_bad += 1;
            self.fail(Error::ZeroTail)
        }
    }
    pub fn run(
        &mut self,
        memory: &mut impl Memory,
        mut progress: impl FnMut(&Report),
    ) -> Result<(), Error> {
        if self.report.phase != Phase::Ready {
            return Err(self.fail(Error::WrongStage));
        }
        self.report.phase = Phase::SnapshotGaps;
        progress(&self.report);
        memory.sync();
        let mut snapshot_at = 0;
        for gap in &self.plan.gaps[..self.plan.gap_count] {
            let mut i = 0;
            while i < gap.bytes {
                let at = gap.offset + i;
                let n = if at % 4 == 0 && gap.bytes - i >= 4 {
                    4
                } else {
                    1
                };
                let bytes = if n == 4 {
                    memory.read_word(at).to_le_bytes()
                } else {
                    [memory.read_byte(at), 0, 0, 0]
                };
                for byte in &bytes[..n] {
                    memory.snapshot_write(snapshot_at, *byte);
                    snapshot_at += 1;
                }
                i += n;
            }
        }
        self.report.snapshot_bytes = snapshot_at as u32;
        memory.sync();
        for load in self.plan.loads {
            self.report.current_index = Some(load.index);
            self.report.phase = Phase::Copy;
            progress(&self.report);
            let mut i = 0;
            while i + 4 <= load.source.len() {
                if (load.offset + i) % 4 != 0 {
                    break;
                }
                memory.write_word(
                    load.offset + i,
                    u32::from_le_bytes(load.source[i..i + 4].try_into().unwrap()),
                );
                i += 4;
                self.report.file_writes += 4;
                if i % CHUNK == 0 {
                    progress(&self.report);
                }
            }
            while i < load.source.len() {
                memory.write_byte(load.offset + i, load.source[i]);
                i += 1;
                self.report.file_writes += 1;
            }
            memory.sync();
            self.report.phase = Phase::Zero;
            progress(&self.report);
            while i < load.memory_bytes && (load.offset + i) % 4 != 0 {
                memory.write_byte(load.offset + i, 0);
                i += 1;
                self.report.zero_writes += 1;
            }
            while i + 4 <= load.memory_bytes {
                memory.write_word(load.offset + i, 0);
                i += 4;
                self.report.zero_writes += 4;
                if i % CHUNK == 0 {
                    progress(&self.report);
                }
            }
            while i < load.memory_bytes {
                memory.write_byte(load.offset + i, 0);
                i += 1;
                self.report.zero_writes += 1;
            }
            memory.sync();
            self.report.loaded_mask |= 1 << load.index;
        }
        for pass in 0..2 {
            self.report.phase = if pass == 0 {
                Phase::VerifyFirst
            } else {
                Phase::VerifySecond
            };
            memory.sync();
            for load in self.plan.loads {
                self.report.current_index = Some(load.index);
                progress(&self.report);
                let mut i = 0;
                while i < load.memory_bytes {
                    let at = load.offset + i;
                    let n = if at % 4 == 0 && load.memory_bytes - i >= 4 {
                        4
                    } else {
                        1
                    };
                    let actual = if n == 4 {
                        memory.read_word(at).to_le_bytes()
                    } else {
                        [memory.read_byte(at), 0, 0, 0]
                    };
                    let mut expected = [0; 4];
                    if i < load.source.len() {
                        let count = n.min(load.source.len() - i);
                        expected[..count].copy_from_slice(&load.source[i..i + count]);
                    }
                    if actual[..n] != expected[..n] {
                        for j in 0..n {
                            if actual[j] != expected[j] {
                                return Err(self.bad_byte(at + j, i + j < load.source.len()));
                            }
                        }
                    }
                    i += n;
                    self.report.checked_bytes[pass] += n as u32;
                    if i % CHUNK == 0 {
                        progress(&self.report);
                    }
                }
                self.report.verified_masks[pass] |= 1 << load.index;
            }
        }
        self.report.phase = Phase::VerifyGaps;
        self.report.current_index = None;
        progress(&self.report);
        memory.sync();
        snapshot_at = 0;
        for gap in &self.plan.gaps[..self.plan.gap_count] {
            let mut i = 0;
            while i < gap.bytes {
                let at = gap.offset + i;
                let n = if at % 4 == 0 && gap.bytes - i >= 4 {
                    4
                } else {
                    1
                };
                let actual = if n == 4 {
                    memory.read_word(at).to_le_bytes()
                } else {
                    [memory.read_byte(at), 0, 0, 0]
                };
                for (j, byte) in actual[..n].iter().enumerate() {
                    if *byte != memory.snapshot_read(snapshot_at) {
                        self.report.first_bad = Some((at + j) as u32);
                        self.report.gap_bad += 1;
                        return Err(self.fail(Error::Gap));
                    }
                    snapshot_at += 1;
                    self.report.gap_checked += 1;
                }
                i += n;
            }
        }
        self.report.phase = Phase::Done;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    // Miniature private plans exercise transfer edges without allocating152MiB
    // for each unit test. The production constructor's full pinned inputs are
    // exercised separately by the actual-adapter/installed-image test.
    fn miniature() -> Plan<'static> {
        let mut loads = [Load {
            index: 0,
            offset: 0,
            memory_bytes: 0,
            source: &[],
        }; LOAD_COUNT];
        let mut gaps = [Gap {
            offset: 0,
            bytes: 0,
        }; LOAD_COUNT + 1];
        let mut cursor = 0;
        for n in 0..LOAD_COUNT {
            gaps[n] = Gap {
                offset: cursor,
                bytes: 3,
            };
            cursor += 3;
            let source: &[u8] = match n % 3 {
                0 => &[],
                1 => &[0x55],
                _ => &[1, 2, 3, 4, 5],
            };
            loads[n] = Load {
                index: n as u16 + 2,
                offset: cursor,
                memory_bytes: source.len() + 5,
                source,
            };
            cursor += source.len() + 5;
        }
        gaps[LOAD_COUNT] = Gap {
            offset: cursor,
            bytes: 7,
        };
        Plan {
            loads,
            gaps,
            gap_count: LOAD_COUNT + 1,
        }
    }
    struct Ram {
        pool: std::vec::Vec<u8>,
        snapshot: std::vec::Vec<u8>,
        ranges: std::vec::Vec<(usize, usize)>,
        writes: usize,
        total: usize,
        word_writes: usize,
        completed_syncs: usize,
        corrupt: Option<(usize, usize)>,
    }
    impl Ram {
        fn new(p: &Plan<'_>) -> Self {
            Self {
                pool: vec![0xa5; 4096],
                snapshot: vec![0; 4096],
                ranges: p
                    .loads
                    .iter()
                    .map(|s| (s.offset, s.offset + s.memory_bytes))
                    .collect(),
                writes: 0,
                total: p.loads.iter().map(|s| s.memory_bytes).sum(),
                word_writes: 0,
                completed_syncs: 0,
                corrupt: None,
            }
        }
        fn allowed(&self, i: usize, n: usize) -> bool {
            self.ranges.iter().any(|(a, b)| i >= *a && i + n <= *b)
        }
    }
    impl Memory for Ram {
        fn read_word(&mut self, i: usize) -> u32 {
            assert_eq!(i % 4, 0);
            u32::from_le_bytes(self.pool[i..i + 4].try_into().unwrap())
        }
        fn write_word(&mut self, i: usize, v: u32) {
            assert_eq!(i % 4, 0);
            assert!(self.allowed(i, 4));
            self.pool[i..i + 4].copy_from_slice(&v.to_le_bytes());
            self.writes += 4;
            self.word_writes += 1;
        }
        fn read_byte(&mut self, i: usize) -> u8 {
            self.pool[i]
        }
        fn write_byte(&mut self, i: usize, v: u8) {
            assert!(self.allowed(i, 1));
            self.pool[i] = v;
            self.writes += 1;
        }
        fn snapshot_read(&mut self, i: usize) -> u8 {
            self.snapshot[i]
        }
        fn snapshot_write(&mut self, i: usize, v: u8) {
            self.snapshot[i] = v;
        }
        fn sync(&mut self) {
            if self.writes == self.total {
                self.completed_syncs += 1;
                if let Some((sync, at)) = self.corrupt {
                    if sync == self.completed_syncs {
                        self.pool[at] ^= 1;
                        self.corrupt = None;
                    }
                }
            }
        }
    }
    fn session(p: Plan<'static>) -> ImageOnce<'static> {
        ImageOnce::after_setup(crate::qcom_pas_mem_setup::tests::completed_setup(), p).unwrap()
    }
    #[test]
    fn serial_copy_pure_bss_unaligned_tails_two_passes_and_exact_gaps() {
        let p = miniature();
        let expected_files: usize = p.loads.iter().map(|l| l.source.len()).sum();
        let mut ram = Ram::new(&p);
        let mut s = session(p);
        s.run(&mut ram, |_| {}).unwrap();
        assert_eq!(s.report().phase, Phase::Done);
        assert_eq!(s.report().loaded_mask, LOAD_MASK);
        assert_eq!(s.report().verified_masks, [LOAD_MASK; 2]);
        assert_eq!(s.report().file_writes, expected_files as u32);
        assert_eq!(s.report().zero_writes, (ram.total - expected_files) as u32);
        assert_eq!(s.report().checked_bytes, [ram.total as u32; 2]);
        assert!(ram.word_writes > 0);
        for load in &s.plan.loads {
            assert_eq!(
                &ram.pool[load.offset..load.offset + load.source.len()],
                load.source
            );
            assert!(
                ram.pool[load.offset + load.source.len()..load.offset + load.memory_bytes]
                    .iter()
                    .all(|v| *v == 0)
            );
        }
        for gap in &s.plan.gaps[..s.plan.gap_count] {
            assert!(
                ram.pool[gap.offset..gap.offset + gap.bytes]
                    .iter()
                    .all(|v| *v == 0xa5)
            );
        }
        let written = ram.writes;
        assert_eq!(s.run(&mut ram, |_| {}), Err(Error::WrongStage));
        assert_eq!(ram.writes, written);
    }
    #[test]
    fn payload_zero_second_read_and_gap_errors_stop_without_repairs() {
        for (sync, which, error) in [
            (2, 0, Error::Payload),
            (2, 1, Error::ZeroTail),
            (3, 0, Error::Payload),
            (4, 2, Error::Gap),
        ] {
            let p = miniature();
            let at = match which {
                0 => p.loads[1].offset,
                1 => p.loads[0].offset,
                _ => p.gaps[0].offset,
            };
            let mut ram = Ram::new(&p);
            ram.corrupt = Some((sync, at));
            let mut s = session(p);
            assert_eq!(s.run(&mut ram, |_| {}), Err(error));
            assert_eq!(s.report().first_bad, Some(at as u32));
            assert_eq!(ram.writes, ram.total);
            if sync == 3 {
                assert_eq!(s.report().verified_masks[0], LOAD_MASK);
                assert_ne!(s.report().verified_masks[1], LOAD_MASK);
            }
            assert!(s.run(&mut ram, |_| {}).is_err());
            assert_eq!(ram.writes, ram.total);
        }
    }
    #[test]
    fn malformed_or_incomplete_source_plan_is_refused_before_memory_access() {
        assert!(matches!(Plan::new(&[], &[]), Err(Error::SourcePlan)));
        assert!(GAP_BYTES < SNAPSHOT_BYTES);
        assert_eq!(FILE_BYTES + ZERO_BYTES, MEMORY_BYTES);
        assert_eq!(MEMORY_BYTES + GAP_BYTES, POOL_BYTES);
    }
    /// Synthetic session fixture only; no secure completion is established.
    pub(crate) fn auth_fixture(completed: bool) -> ImageOnce<'static> {
        let mut s = session(miniature());
        if completed {
            s.report.phase = Phase::Done;
            s.report.loaded_mask = LOAD_MASK;
            s.report.verified_masks = [LOAD_MASK; 2];
            s.report.file_writes = FILE_BYTES as u32;
            s.report.zero_writes = ZERO_BYTES as u32;
            s.report.checked_bytes = [MEMORY_BYTES as u32; 2];
            s.report.snapshot_bytes = GAP_BYTES as u32;
            s.report.gap_checked = GAP_BYTES as u32;
        }
        s
    }
}
