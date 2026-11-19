#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
pub mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
pub mod sunfish_ipa_linux_gsi;
#[path = "../rust/src/sunfish_ipa_linux_receive.rs"]
pub mod sunfish_ipa_linux_receive;
#[path = "../rust/src/sunfish_ipa_linux_receive_pool.rs"]
pub mod sunfish_ipa_linux_receive_pool;

use sunfish_ipa_linux_receive::*;

#[test]
fn receive_full_linux_start_suspend_replenish_and_default_route_match_compiled_c() {
    let plan = Plan::new().unwrap();
    let start = std::fs::read_to_string(env!("SUNFISH_RECEIVE_START_C")).unwrap();
    assert_eq!(
        START_WORDS
            .iter()
            .map(|w| format!("{:05x} {:08x}\n", w.offset, w.value))
            .collect::<String>(),
        start
    );
    let oracle = std::fs::read_to_string(env!("SUNFISH_RECEIVE_C")).unwrap();
    let lines: Vec<_> = oracle.lines().collect();
    assert_eq!(lines[0], "geometry 8 2 256 6 251 8192");
    let writes: Vec<_> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("write "))
        .collect();
    let expected = SUSPEND_WORDS
        .into_iter()
        .chain((0..DOORBELL_COUNT).map(|n| plan.doorbell(n).unwrap().word))
        .chain([DEFAULT_ROUTE]);
    assert_eq!(
        expected
            .map(|w| format!("{:05x} {:08x}", w.offset, w.value))
            .collect::<Vec<_>>(),
        writes
    );
    let buffers: Vec<_> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("buffer "))
        .collect();
    assert_eq!(buffers.len(), BUFFER_COUNT);
    for (n, line) in buffers.iter().enumerate() {
        let b = plan.buffer(n).unwrap();
        assert_eq!(
            *line,
            format!(
                "{n} {:016x} {HEADROOM} {DMA_BYTES} {}",
                b.allocation_iova(),
                b.tre()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            )
        );
    }
}
#[test]
fn receive_full_capacity_has_unique_aligned_backing_and_five_unused_tre_slots() {
    let p = Plan::new().unwrap();
    assert_eq!(DATA_USED_BYTES, 0x1f6000);
    assert_eq!(DATA_USED_PAGES, 502);
    assert_eq!(DATA_RESERVED_BYTES - DATA_USED_BYTES, 0xa000);
    assert_eq!((p.event_iova(), p.event_offset()), (0x80008000, 0x8000));
    for n in 0..BUFFER_COUNT {
        let b = p.buffer(n).unwrap();
        assert_eq!(b.index(), n);
        assert_eq!(b.allocation_iova() % 8192, 0);
        assert_eq!(b.dma_iova() - b.allocation_iova(), 64);
        assert_eq!(
            b.dma_iova() + b.dma_bytes() as u64,
            b.allocation_iova() + 8192
        );
        assert_eq!(b.tre_iova(), 0x80009000 + n as u64 * 16);
        assert_eq!(b.tre_offset(), 0x9000 + n * 16);
        assert_eq!(b.allocation_offset(), n * 8192);
        assert!(b.allocation_offset() + 8192 <= DATA_USED_BYTES);
        assert_eq!(&b.tre()[10..12], &[0, 0]);
        assert_eq!(
            u32::from_le_bytes(b.tre()[12..16].try_into().unwrap()),
            0x20200
        );
    }
    assert_eq!(RING_COUNT - BUFFER_COUNT, 5);
    for n in [251, 252, 255, 256, usize::MAX] {
        assert_eq!(p.buffer(n), Err(Error::Bounds));
    }
}
#[test]
fn receive_batch_doorbells_publish_all_251_buffers_and_flush_the_short_final_batch() {
    let p = Plan::new().unwrap();
    let mut last = 0;
    assert_eq!(DOORBELL_COUNT, 16);
    for n in 0..DOORBELL_COUNT {
        let db = p.doorbell(n).unwrap();
        assert_eq!(db.word.space, Space::Gsi);
        assert_eq!(db.word.offset, 0x1e010);
        assert_eq!(db.published_buffers - last, if n == 15 { 11 } else { 16 });
        assert_eq!(db.word.value, 0x80009000 + db.published_buffers as u32 * 16);
        last = db.published_buffers;
    }
    assert_eq!(last, 251);
    assert_eq!(p.doorbell(15).unwrap().word.value, 0x80009fb0);
    for n in [16, 17, usize::MAX] {
        assert_eq!(p.doorbell(n), Err(Error::Bounds));
    }
}
#[test]
fn receive_requires_a_separate_full_window_and_preserves_existing_command_uc_enables() {
    let p = Plan::new().unwrap();
    assert_eq!(p.validate_window(DATA_IOVA, DATA_RESERVED_BYTES), Ok(()));
    for (a, b) in [
        (0x80000000, 0x20000),
        (DATA_IOVA, DATA_USED_BYTES),
        (DATA_IOVA, 8192),
        (DATA_IOVA + 4096, DATA_RESERVED_BYTES),
        (0xffffe000, DATA_RESERVED_BYTES),
        (u64::MAX, usize::MAX),
    ] {
        assert_eq!(p.validate_window(a, b), Err(Error::Window));
    }
    assert_eq!(GSI_IEOB_MASK, 1 | (1 << EVENT));
    assert_eq!(IPA_IRQ_MASK & 0xc, 0xc);
    assert_eq!(IPA_IRQ_MASK & !0xc, 1 << 14);
    assert_eq!(START_WORDS[4].value, 0x01000000 | CHANNEL as u32);
    assert_eq!(DEFAULT_ROUTE.space, Space::Ipa);
    assert_eq!(DEFAULT_ROUTE.value & (1 << 0), 0);
    assert_eq!((DEFAULT_ROUTE.value >> 1) & 31, ENDPOINT as u32);
    assert_eq!((DEFAULT_ROUTE.value >> 17) & 31, ENDPOINT as u32);
}

use std::{cell::RefCell, rc::Rc};
use sunfish_ipa_linux_buffers::{Memory, MemoryError};
use sunfish_ipa_linux_receive_pool::{DataPool, Error as PoolError, HEADROOM_CANARY, TAIL_CANARY};
struct RamState {
    words: Vec<u32>,
    reads: usize,
    writes: usize,
    fail_read: Option<usize>,
    fail_write: Option<usize>,
    suppress: Option<usize>,
}
#[derive(Clone)]
struct Ram(Rc<RefCell<RamState>>, usize);
impl Memory for Ram {
    fn bytes(&self) -> usize {
        self.1
    }
    fn read32(&mut self, offset: usize) -> Result<u32, MemoryError> {
        let mut s = self.0.borrow_mut();
        let n = s.reads;
        s.reads += 1;
        if s.fail_read == Some(n) {
            return Err(MemoryError::Io);
        }
        Ok(s.words[offset / 4])
    }
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError> {
        let mut s = self.0.borrow_mut();
        let n = s.writes;
        s.writes += 1;
        if s.suppress != Some(n) {
            s.words[offset / 4] = value;
        }
        if s.fail_write == Some(n) {
            Err(MemoryError::Io)
        } else {
            Ok(())
        }
    }
}
fn ram() -> Ram {
    Ram(
        Rc::new(RefCell::new(RamState {
            words: vec![0xa5a5a5a5; DATA_RESERVED_BYTES / 4],
            reads: 0,
            writes: 0,
            fail_read: None,
            fail_write: None,
            suppress: None,
        })),
        DATA_RESERVED_BYTES,
    )
}
#[test]
fn receive_pool_initializes_all_backing_and_checks_twice_without_hardware_authority() {
    let r = ram();
    let mut p = DataPool::new(r.clone()).unwrap();
    let mut guards = 0;
    let report = p.prepare(|| {
        guards += 1;
        Ok(())
    });
    assert_eq!(report.error, None);
    assert!(p.prepared());
    assert_eq!(
        (
            report.writes_sent,
            report.writes_done,
            report.checked_words,
            report.passes
        ),
        (524288, 524288, 1048576, 2)
    );
    assert_eq!(guards, 757);
    assert_eq!(p.plan(), Plan::new().unwrap());
    let s = r.0.borrow();
    for n in 0..BUFFER_COUNT {
        let start = n * BUFFER_BYTES / 4;
        assert!(
            s.words[start..start + HEADROOM / 4]
                .iter()
                .all(|v| *v == HEADROOM_CANARY)
        );
        assert!(
            s.words[start + HEADROOM / 4..start + BUFFER_BYTES / 4]
                .iter()
                .all(|v| *v == 0)
        );
    }
    assert!(
        s.words[DATA_USED_BYTES / 4..]
            .iter()
            .all(|v| *v == TAIL_CANARY)
    );
    drop(s);
    let before = (r.0.borrow().reads, r.0.borrow().writes);
    let checked = p.verify_guards(|| Ok(()));
    assert_eq!(checked.error, None);
    assert_eq!(
        checked.checked_words,
        sunfish_ipa_linux_receive_pool::GUARD_WORDS
    );
    assert_eq!(checked.checked_words, 14256);
    assert_eq!(r.0.borrow().writes, before.1);
}
#[test]
fn receive_pool_rejects_wrong_capacity_and_never_retries_or_reuses_consumed_backing() {
    for size in [
        0,
        0x20000,
        DATA_USED_BYTES,
        DATA_RESERVED_BYTES + 4096,
        usize::MAX,
    ] {
        let mut r = ram();
        r.1 = size;
        assert!(matches!(DataPool::new(r.clone()), Err(PoolError::Layout)));
        assert_eq!((r.0.borrow().reads, r.0.borrow().writes), (0, 0));
    }
    let r = ram();
    let mut p = DataPool::new(r.clone()).unwrap();
    assert_eq!(p.verify_guards(|| Ok(())).error, Some(PoolError::Consumed));
    assert_eq!(p.prepare(|| Ok(())).error, None);
    let before = (r.0.borrow().reads, r.0.borrow().writes);
    assert_eq!(
        p.prepare(|| panic!("no second guard")).error,
        Some(PoolError::Consumed)
    );
    assert_eq!((r.0.borrow().reads, r.0.borrow().writes), before);
    p.quarantine();
    assert!(!p.prepared());
    assert_eq!(
        p.verify_guards(|| panic!("quarantined guard")).error,
        Some(PoolError::Quarantined)
    );
    assert_eq!(
        p.prepare(|| panic!("quarantined guard")).error,
        Some(PoolError::Quarantined)
    );
    assert_eq!((r.0.borrow().reads, r.0.borrow().writes), before);
}
#[test]
fn receive_pool_guard_faults_at_each_phase_edge_consume_and_quarantine_the_attempt() {
    for failed in [0, 1, 250, 251, 252, 502, 503, 504, 755, 756] {
        let r = ram();
        let mut p = DataPool::new(r.clone()).unwrap();
        let mut guards = 0;
        let report = p.prepare(|| {
            let n = guards;
            guards += 1;
            if n == failed {
                Err(PoolError::Guard)
            } else {
                Ok(())
            }
        });
        assert_eq!(report.error, Some(PoolError::Guard), "guard {failed}");
        assert!(!p.prepared());
        if failed == 0 {
            assert_eq!(report.writes_sent, 0);
        }
        let before = (r.0.borrow().reads, r.0.borrow().writes);
        assert_eq!(p.prepare(|| Ok(())).error, Some(PoolError::Quarantined));
        assert_eq!((r.0.borrow().reads, r.0.borrow().writes), before);
    }
}
#[test]
fn receive_pool_uncertain_cpu_stores_reads_and_nonsticking_words_never_publish_prepared() {
    let words = DATA_RESERVED_BYTES / 4;
    for kind in 0..3 {
        let points = if kind == 0 {
            vec![
                0,
                16,
                DATA_USED_BYTES / 4 - 1,
                words - 1,
                words,
                2 * words - 1,
            ]
        } else {
            vec![
                0,
                16,
                BUFFER_BYTES / 4,
                DATA_USED_BYTES / 4 - 1,
                DATA_USED_BYTES / 4,
                words - 1,
            ]
        };
        for n in points {
            let r = ram();
            let mut p = DataPool::new(r.clone()).unwrap();
            match kind {
                0 => r.0.borrow_mut().fail_read = Some(n),
                1 => r.0.borrow_mut().fail_write = Some(n),
                _ => r.0.borrow_mut().suppress = Some(n),
            }
            let report = p.prepare(|| Ok(()));
            assert_eq!(
                report.error,
                Some(if kind == 2 {
                    PoolError::Readback
                } else {
                    PoolError::Memory
                }),
                "{kind}/{n}"
            );
            assert!(!p.prepared());
            if kind == 1 {
                assert_eq!(report.writes_sent, report.writes_done + 1);
            }
            let before = (r.0.borrow().reads, r.0.borrow().writes);
            assert_eq!(p.prepare(|| Ok(())).error, Some(PoolError::Quarantined));
            assert_eq!((r.0.borrow().reads, r.0.borrow().writes), before);
        }
    }
}
#[test]
fn receive_pool_guard_observation_ignores_packet_body_and_contains_header_tail_or_access_faults() {
    for fault in 0..7 {
        let r = ram();
        let mut p = DataPool::new(r.clone()).unwrap();
        assert_eq!(p.prepare(|| Ok(())).error, None);
        let reads = r.0.borrow().reads;
        match fault {
            0 => r.0.borrow_mut().words[HEADROOM / 4] = 0x1234,
            1 => r.0.borrow_mut().words[0] ^= 1,
            2 => r.0.borrow_mut().words[(250 * BUFFER_BYTES + HEADROOM - 4) / 4] ^= 1,
            3 => r.0.borrow_mut().words[DATA_USED_BYTES / 4] ^= 1,
            4 => r.0.borrow_mut().words[DATA_RESERVED_BYTES / 4 - 1] ^= 1,
            5 => r.0.borrow_mut().fail_read = Some(reads),
            _ => r.0.borrow_mut().fail_read = Some(reads + 14255),
        }
        let writes = r.0.borrow().writes;
        let report = p.verify_guards(|| Ok(()));
        assert_eq!(r.0.borrow().writes, writes);
        if fault == 0 {
            assert_eq!(report.error, None);
            assert!(p.prepared());
        } else {
            assert_eq!(
                report.error,
                Some(if fault < 5 {
                    PoolError::Readback
                } else {
                    PoolError::Memory
                })
            );
            assert!(!p.prepared());
        }
    }
    let r = ram();
    let mut p = DataPool::new(r).unwrap();
    p.prepare(|| Ok(()));
    assert_eq!(
        p.verify_guards(|| Err(PoolError::Guard)).error,
        Some(PoolError::Guard)
    );
    assert!(!p.prepared());
}
