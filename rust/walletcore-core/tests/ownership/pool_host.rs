//! Exercise the production ordering and mailbox ownership state machines. Only
//! the Microkit mapping lookup and notification delivery are host substitutes.
use super::{funded, wallet_scan_mailbox::Mailbox, wallet_scan_pool::ScanPool, wave};
use monero_wallet::primitives::keccak256;
use std::{
    cell::RefCell,
    sync::{Barrier, Mutex, mpsc},
    thread,
};
use walletcore_core::{
    MAX_SCAN_JOB, MAX_SCAN_RESULT, RangeBlock, RangeBody, ScannedBlock, ViewPair, encode_scan_job,
    execute_scan_job, scan_assembled_strict,
};

type Slot = Mailbox<MAX_SCAN_JOB, MAX_SCAN_RESULT>;
static SLOTS: [Slot; 3] = [const { Slot::empty() }; 3];
static POOL_TEST: Mutex<()> = Mutex::new(());
thread_local! {static NOTIFICATIONS: RefCell<Vec<usize>> = const {RefCell::new(Vec::new())};}

#[macro_export]
macro_rules! var {
    (scan_mailbox_1: usize = 0) => {
        &($crate::pool_host::address(0))
    };
    (scan_mailbox_2: usize = 0) => {
        &($crate::pool_host::address(1))
    };
    (scan_mailbox_3: usize = 0) => {
        &($crate::pool_host::address(2))
    };
}
pub(crate) fn address(lane: usize) -> usize {
    &SLOTS[lane] as *const Slot as usize
}
pub struct Channel(usize);
impl Channel {
    pub fn new(id: usize) -> Self {
        Self(id)
    }
    pub fn notify(self) {
        NOTIFICATIONS.with(|log| log.borrow_mut().push(self.0));
    }
}

fn transcript(items: &[ScannedBlock]) -> [u8; 32] {
    items.iter().fold([0; 32], |previous, b| {
        let mut bytes = previous.to_vec();
        bytes.extend(b.height.to_le_bytes());
        bytes.extend(b.hash);
        bytes.extend(b.previous);
        bytes.extend(b.miner_hash);
        bytes.extend((b.transactions.len() as u32).to_le_bytes());
        for tx in &b.transactions {
            bytes.extend(tx);
        }
        bytes.extend((b.outputs.len() as u32).to_le_bytes());
        for output in &b.outputs {
            bytes.extend(keccak256(output.serialize()));
        }
        keccak256(bytes)
    })
}

fn serial(view: &ViewPair, blocks: Vec<RangeBlock>) -> Vec<ScannedBlock> {
    blocks
        .into_iter()
        .map(|b| {
            let height = b.block.number() as u64;
            let hash = b.block.hash();
            let previous = b.block.header.previous;
            let miner_hash = b.block.miner_transaction().hash();
            let transactions = b.block.transactions.clone();
            let outputs = scan_assembled_strict(view, b.block, b.txs, b.first_rct).unwrap();
            ScannedBlock {
                height,
                hash,
                previous,
                miner_hash,
                transactions,
                outputs,
            }
        })
        .collect()
}

fn dispatch(pool: &mut ScanPool, view: &ViewPair, blocks: &mut std::vec::IntoIter<RangeBlock>) {
    assert!(SLOTS.iter().all(Slot::available));
    pool.dispatch(view, blocks, 100);
    assert_eq!(pool.pending(), 4);
    assert!(!pool.head_waiting());
    assert_eq!(pool.counts, [0; 4]);
    assert_eq!(NOTIFICATIONS.with(|q| q.take()), [16, 17, 18]);
}

fn local(pool: &mut ScanPool, view: &ViewPair) -> ScannedBlock {
    let first = pool.next(view, 100).unwrap().unwrap();
    assert!(first.first_rct.is_some() && first.next_rct.is_some());
    assert_eq!(pool.pending(), 3);
    first.block
}

#[test]
fn production_pool_orders_reverse_failures_timeouts_and_cancellation() {
    let _lock = POOL_TEST.lock().unwrap_or_else(|p| p.into_inner());
    let (view, _, base) = funded();
    let expected = serial(&view, wave(&base, 0));
    let trusted = transcript(&expected);
    assert_eq!(
        trusted.as_slice(),
        super::hex("ab40d38530ab4102335767dfe47265a12bf8898903d46f954c308f336c8af20e")
    );
    assert_eq!(expected.iter().map(|b| b.outputs.len()).sum::<usize>(), 2);

    let mut pool = ScanPool::new();
    let mut blocks = wave(&base, 0).into_iter();
    dispatch(&mut pool, &view, &mut blocks);
    let mut done = Vec::new();
    // All three secondary workers hold their real RUNNING mailbox. Release them
    // last-height first and poll after each completion. No sleeps or time races.
    {
        let entered = Barrier::new(4);
        thread::scope(|lanes| {
            let mut signals = Vec::new();
            for slot in &SLOTS {
                let (go, rx) = mpsc::channel();
                let (sent, ack) = mpsc::channel();
                let entered = &entered;
                lanes.spawn(move || {
                    assert!(slot.execute(|job, dst| {
                        entered.wait();
                        let result = execute_scan_job(job, dst).ok();
                        rx.recv().unwrap();
                        result
                    }));
                    sent.send(()).unwrap();
                });
                signals.push((go, ack));
            }
            entered.wait();
            // A complete block can be checked between wave submission and the
            // coordinator's local ownership scan, with all three mailboxes running.
            let bytes = include_bytes!("../fixtures/mainnet-3806161-range2.bin");
            let mut response = RangeBody::new(3_806_161, 2);
            response.append(&bytes[..bytes.len() - 16]).unwrap();
            assert!(SLOTS.iter().all(Slot::running));
            assert!(response.validate_next());
            assert_eq!(response.validated_blocks(), 1);
            done.push(local(&mut pool, &view));
            assert!(pool.head_waiting());
            assert_eq!(pool.overlaps, 1);
            for lane in (0..3).rev() {
                signals[lane].0.send(()).unwrap();
                signals[lane].1.recv().unwrap();
                if lane > 0 {
                    assert!(pool.next(&view, 101).unwrap().is_none());
                }
            }
        });
    }
    for _ in 0..3 {
        done.push(pool.next(&view, 110).unwrap().unwrap().block);
    }
    assert_eq!(transcript(&done), trusted);
    assert_eq!(pool.counts, [1, 1, 1, 1]);
    assert_eq!(pool.fallbacks, 0);
    assert!(pool.next(&view, 111).unwrap().is_none());

    // Wrong but valid reply for the funded height; explicit failure at the next
    // height; no reply for the last. Each falls back exactly once to its original.
    let mut pool = ScanPool::new();
    let mut blocks = wave(&base, 0).into_iter();
    dispatch(&mut pool, &view, &mut blocks);
    let mut done = vec![local(&mut pool, &view)];
    let other = wave(&base, 19).remove(1);
    let mut job = vec![0; MAX_SCAN_JOB];
    let size = encode_scan_job(&view, &other, &mut job).unwrap();
    assert!(SLOTS[0].execute(|_, dst| execute_scan_job(&job[..size], dst).ok()));
    assert!(SLOTS[1].execute(|_, _| None));
    for _ in 0..2 {
        done.push(pool.next(&view, 101).unwrap().unwrap().block);
    }
    assert!(pool.next(&view, 2099).unwrap().is_none());
    done.push(pool.next(&view, 2100).unwrap().unwrap().block);
    assert_eq!(transcript(&done), trusted);
    assert_eq!(pool.counts, [4, 0, 0, 0]);
    assert_eq!(pool.fallbacks, 3);
    assert!(SLOTS.iter().all(Slot::available));

    // Cancel the old wallet while its funded job is executing and another result
    // is already waiting. A new scan may run immediately, even before it releases.
    let other_view = ViewPair::new(
        monero_wallet::ed25519::CompressedPoint::G
            .decompress()
            .unwrap(),
        zeroize::Zeroizing::new(monero_wallet::ed25519::Scalar::ONE),
    )
    .unwrap();
    let other_expected = serial(&other_view, wave(&base, 0));
    assert!(other_expected.iter().all(|b| b.outputs.is_empty()));
    let mut old_pool = ScanPool::new();
    let mut old = wave(&base, 0).into_iter();
    dispatch(&mut old_pool, &view, &mut old);
    let _ = local(&mut old_pool, &view);
    assert!(SLOTS[2].execute(|j, o| execute_scan_job(j, o).ok()));
    let barrier = Barrier::new(2);
    thread::scope(|s| {
        s.spawn(|| {
            assert!(SLOTS[0].execute(|j, o| {
                barrier.wait();
                barrier.wait();
                execute_scan_job(j, o).ok()
            }));
        });
        barrier.wait();
        drop(old_pool);
        assert!(!SLOTS[0].available() && SLOTS[1].available() && SLOTS[2].available());
        let mut fresh = ScanPool::new();
        let mut new = wave(&base, 0).into_iter();
        fresh.dispatch(&other_view, &mut new, 500);
        let mut done = vec![fresh.next(&other_view, 500).unwrap().unwrap().block];
        assert!(!SLOTS[0].available());
        barrier.wait();
        // The wallet may encounter the cancelled lane either before or after
        // the worker frees it. Complete only jobs this new pool actually issued.
        while new.len() > 0 || fresh.pending() > 0 {
            fresh.dispatch(&other_view, &mut new, 501);
            if let Some(done_block) = fresh.next(&other_view, 501).unwrap() {
                done.push(done_block.block);
            }
            for channel in NOTIFICATIONS.with(|q| q.take()) {
                assert!(SLOTS[channel - 16].execute(|j, o| execute_scan_job(j, o).ok()));
            }
        }
        assert_eq!(transcript(&done), transcript(&other_expected));
    });
    assert!(SLOTS.iter().all(Slot::available));
}

#[test]
fn fast_lanes_refill_behind_a_running_head_with_a_bounded_ordered_window() {
    let _lock = POOL_TEST.lock().unwrap_or_else(|p| p.into_inner());
    let (view, _, base) = funded();
    let make = || (0..4).flat_map(|i| wave(&base, 4 * i)).collect::<Vec<_>>();
    let expected = serial(&view, make());
    let mut input = make().into_iter();
    let mut pool = ScanPool::new();
    dispatch(&mut pool, &view, &mut input);
    let mut done = vec![local(&mut pool, &view)];
    thread::scope(|threads| {
        let (release, hold) = mpsc::channel();
        // Hold the physical slot while later heights finish on other workers.
        let (running, receive) = mpsc::channel();
        threads.spawn(move || {
            assert!(SLOTS[0].execute(|job, out| {
                running.send(()).unwrap();
                let _ = hold.recv();
                execute_scan_job(job, out).ok()
            }))
        });
        receive.recv().unwrap();
        assert!(SLOTS[1].execute(|j, o| execute_scan_job(j, o).ok()));
        assert!(SLOTS[2].execute(|_, _| None)); // later height must fall back
        pool.dispatch(&view, &mut input, 101);
        let mut issued = NOTIFICATIONS.with(|q| q.take());
        assert!(
            issued.contains(&17),
            "free worker was idle behind an earlier running job"
        );
        for _ in 0..16 {
            assert!(pool.next(&view, 101).unwrap().is_none());
            for channel in issued.drain(..) {
                assert!(SLOTS[channel - 16].execute(|j, o| execute_scan_job(j, o).ok()));
            }
            pool.dispatch(&view, &mut input, 101);
            issued = NOTIFICATIONS.with(|q| q.take());
            assert!(!issued.contains(&16) && pool.pending() <= 8);
        }
        assert!(input.len() > 0 && pool.pending() == 8);
        release.send(()).unwrap();
    });
    while input.len() != 0 || pool.pending() != 0 {
        pool.dispatch(&view, &mut input, 102);
        for channel in NOTIFICATIONS.with(|q| q.take()) {
            assert!(SLOTS[channel - 16].execute(|j, o| execute_scan_job(j, o).ok()));
        }
        if let Some(result) = pool.next(&view, 102).unwrap() {
            done.push(result.block);
        }
    }
    assert_eq!(transcript(&done), transcript(&expected));
    assert_eq!(pool.counts.iter().sum::<u64>(), 16);
    assert_eq!(pool.fallbacks, 1);
    assert!(SLOTS.iter().all(Slot::available));
}

#[test]
fn a_failed_speculative_local_scan_is_reported_at_its_own_height() {
    let _lock = POOL_TEST.lock().unwrap_or_else(|p| p.into_inner());
    let (view, _, base) = funded();
    let mut items = wave(&base, 0);
    items.extend(wave(&base, 4));
    let expected = items
        .iter()
        .take(4)
        .map(|item| item.block.number() as u64)
        .collect::<Vec<_>>();
    items[4].first_rct = None;
    let mut input = items.into_iter();
    let mut pool = ScanPool::new();
    dispatch(&mut pool, &view, &mut input);
    assert_eq!(local(&mut pool, &view).height, expected[0]);
    pool.dispatch(&view, &mut input, 101);
    assert!(pool.next(&view, 101).unwrap().is_none());
    for slot in &SLOTS {
        assert!(slot.execute(|job, result| execute_scan_job(job, result).ok()));
    }
    for height in &expected[1..] {
        assert_eq!(
            pool.next(&view, 102).unwrap().unwrap().block.height,
            *height
        );
    }
    assert!(pool.next(&view, 103).is_err());
    assert!(SLOTS.iter().all(Slot::available));
}

#[test]
fn large_reordered_replies_apply_backpressure_and_cancel_only_current_slot_owners() {
    let _lock = POOL_TEST.lock().unwrap_or_else(|p| p.into_inner());
    let (view, _, base) = funded();
    let mut input = (0..4)
        .flat_map(|i| wave(&base, 4 * i))
        .collect::<Vec<_>>()
        .into_iter();
    let mut pool = ScanPool::new();
    dispatch(&mut pool, &view, &mut input);
    let _ = local(&mut pool, &view);
    for slot in &SLOTS[1..] {
        assert!(slot.execute(|_, out| {
            let n = MAX_SCAN_RESULT * 3 / 4;
            out[..n].fill(0x31);
            Some(n)
        }));
    }
    pool.dispatch(&view, &mut input, 101);
    assert_eq!(NOTIFICATIONS.with(|q| q.take()), [17, 18]);
    for slot in &SLOTS[1..] {
        assert!(slot.execute(|job, out| execute_scan_job(job, out).ok()));
    }
    // More than a MiB of earlier encoded results is already retained. Leave
    // these completions in place instead of allocating more coordinator memory.
    pool.dispatch(&view, &mut input, 102);
    assert!(NOTIFICATIONS.with(|q| q.take()).is_empty());
    assert!(SLOTS[1..].iter().all(Slot::finished));
    drop(pool);
    assert!(SLOTS.iter().all(Slot::available));

    let mut clean = ScanPool::new();
    let mut new_input = wave(&base, 0).into_iter();
    dispatch(&mut clean, &view, &mut new_input);
    for slot in &SLOTS {
        assert!(slot.execute(|job, out| execute_scan_job(job, out).ok()));
    }
    let mut done = Vec::new();
    while let Some(item) = clean.next(&view, 103).unwrap() {
        done.push(item.block);
    }
    assert_eq!(
        transcript(&done),
        transcript(&serial(&view, wave(&base, 0)))
    );
    assert_eq!(clean.fallbacks, 0);
    assert!(SLOTS.iter().all(Slot::available));
}

#[test]
fn unencodable_item_is_reserved_once_for_ordered_local_execution() {
    let _lock = POOL_TEST.lock().unwrap_or_else(|p| p.into_inner());
    NOTIFICATIONS.with(|q| q.take());
    let (view, _, base) = funded();
    let mut items = wave(&base, 0);
    items[1].first_rct = None;
    let first = items[0].block.number() as u64;
    let mut input = items.into_iter();
    let mut pool = ScanPool::new();
    pool.dispatch(&view, &mut input, 100);
    // Height one is consumed even though CPU zero already owns height zero.
    // No other worker is notified for this unencodable item.
    assert_eq!(input.len(), 2);
    assert_eq!(pool.pending(), 2);
    assert!(NOTIFICATIONS.with(|q| q.take()).is_empty());
    pool.dispatch(&view, &mut input, 101);
    assert_eq!(NOTIFICATIONS.with(|q| q.take()), [16, 17]);
    assert_eq!(pool.next(&view, 102).unwrap().unwrap().block.height, first);
    assert!(pool.next(&view, 103).is_err());
    drop(pool);
    assert!(SLOTS.iter().all(Slot::available));
}
