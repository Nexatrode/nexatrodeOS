//! Drive the production bulk wallet pump with a deterministic clock and TCP slot.
//! The range decoder and RestoreChain hash/height checks are the production implementations.
#![allow(dead_code, unexpected_cfgs)]

extern crate alloc;
extern crate self as sel4_microkit;
extern crate self as walletcore_core;
extern crate walletcore_core as real_core;

use real_core::{Block, Pruned, RestoreChain, Transaction};
pub use real_core::{
    MAX_RANGE_BLOCKS, MAX_RANGE_BODY, RangeBlock, encode_blocks_request, scan_request_delay_ns,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};
#[path = "../../src/wallet_bulk.rs"]
mod wallet_bulk;
#[path = "../../src/wallet_range_plan.rs"]
mod wallet_range_plan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Fetch(u64, usize),
    Prime(u64),
    Partial(u64),
    ValidatedBlock(u64),
    EndOfBody(u64),
    Decode(u64, bool),
    Commit(u64),
    Reset,
    ReleaseIdle,
    Reused(u64),
    Cancel(u64),
}
thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    static HEAP_USED: Cell<usize> = const { Cell::new(0) };
    static NS: Cell<u64> = const { Cell::new(0) };
    static WAKE_NS: Cell<u64> = const { Cell::new(0) };
}
fn record(event: Event) {
    EVENTS.with_borrow_mut(|events| events.push(event));
}
fn history() -> Vec<Event> {
    EVENTS.with_borrow(Clone::clone)
}

pub struct RangeBody {
    start: u64,
    body: real_core::RangeBody,
}
impl RangeBody {
    fn new(start: u64, count: usize) -> Self {
        Self {
            start,
            body: real_core::RangeBody::new(start, count),
        }
    }
    fn len(&self) -> usize {
        self.body.len()
    }
    fn capacity(&self) -> usize {
        self.body.capacity()
    }
    fn validated_blocks(&self) -> usize {
        self.body.validated_blocks()
    }
    fn validate_next(&mut self) -> bool {
        let before = self.validated_blocks();
        let progressed = self.body.validate_next();
        if self.validated_blocks() > before {
            record(Event::ValidatedBlock(self.start));
        }
        progressed
    }
    fn finish(self) -> Result<Vec<RangeBlock>, real_core::RangeError> {
        let result = self.body.finish();
        record(Event::Decode(self.start, result.is_ok()));
        result
    }
}

// Clock advances past request pacing between turns. The dispatch slice executes one step.
pub fn get_mr(_: usize) -> u64 {
    NS.with(|clock| clock.replace(clock.get() + 300_000_000))
}
pub fn set_mr(_: usize, value: u64) {
    WAKE_NS.with(|delay| delay.set(value));
}
pub struct MessageInfo;
impl MessageInfo {
    pub fn new(_: usize, _: usize) -> Self {
        Self
    }
}
struct Channel;
impl Channel {
    fn pp_call(&self, _: MessageInfo) {}
    fn notify(&self) {}
}
const SYNC_TIMER_CHANNEL: Channel = Channel;
const WALLET_SHELL_CHANNEL: Channel = Channel;
const HEAP_BYTES: usize = 32 * 1024 * 1024;
fn heap_used_bytes() -> usize {
    HEAP_USED.with(Cell::get)
}
#[macro_export]
macro_rules! debug_println { ($($arg:tt)*) => { let _ = format_args!($($arg)*); }; }
mod wallet_tls_platform {
    pub fn unix_time() -> Option<u64> {
        Some(1_800_000_000)
    }
}

struct Reply {
    start: u64,
    pending: usize,
    partial_len: usize,
    body: Vec<u8>,
    failure: Option<wallet_range_fetch::RangeError>,
}
struct TestSocket(VecDeque<Reply>);
impl TestSocket {
    fn reset(&mut self) -> Result<(), ()> {
        record(Event::ReleaseIdle);
        Ok(())
    }
}
mod wallet_range_fetch {
    use super::*;
    #[derive(Debug, Clone, Copy)]
    pub struct PeerClosed;
    impl PeerClosed {
        pub fn transient(self) -> bool { true }
    }
    #[derive(Debug, Clone, Copy)]
    pub enum RangeError {
        RateLimited,
        Tls(PeerClosed),
        Protocol,
    }
    impl RangeError {
        pub fn too_large(self) -> bool {
            false
        }
        pub fn unsupported(self) -> bool {
            false
        }
        pub fn transient(self) -> bool {
            !matches!(self, Self::Protocol)
        }
        pub fn rate_limited(self) -> bool {
            matches!(self, Self::RateLimited)
        }
    }
    pub struct RangeConnection;
    pub struct RangeFetch {
        pub reused: bool,
        pub start_height: u64,
        pub count: usize,
        reply: Reply,
        body: Option<RangeBody>,
    }
    impl RangeFetch {
        pub fn prime_request(&mut self, _: &mut TestSocket, _: u64) -> Result<bool, RangeError> {
            assert!(self.reused);
            record(Event::Prime(self.start_height));
            if let Some(error @ (RangeError::Tls(_) | RangeError::Protocol)) = self.reply.failure {
                return Err(error);
            }
            Ok(true)
        }
        pub fn start(
            tcp: &mut TestSocket,
            _: (),
            _: [u8; 4],
            start: u64,
            count: usize,
            _: &[u8],
            _: Option<u64>,
            _: u64,
            idle: &mut Option<RangeConnection>,
        ) -> Result<Self, RangeError> {
            record(Event::Fetch(start, count));
            let reply = tcp.0.pop_front().expect("extra range requested");
            assert_eq!(start, reply.start);
            let reused = idle.take().is_some();
            if reused { record(Event::Reused(start)); }
            Ok(Self {
                reused,
                start_height: start,
                count,
                reply,
                body: Some(RangeBody::new(start, count)),
            })
        }
        pub fn step(
            &mut self,
            _: &mut TestSocket,
            _: u64,
            idle: &mut Option<RangeConnection>,
        ) -> Result<Option<RangeBody>, RangeError> {
            if let Some(error) = self.reply.failure {
                return Err(error);
            }
            let body = self.body.as_mut().unwrap();
            let delivered = body.len();
            if self.reply.pending > 0 {
                self.reply.pending -= 1;
                // The footer, including the final output indices, has not arrived.
                let until = self.reply.partial_len;
                body.body
                    .append(&self.reply.body[delivered..until])
                    .unwrap();
                record(Event::Partial(self.start_height));
                return Ok(None);
            }
            body.body.append(&self.reply.body[delivered..]).unwrap();
            record(Event::EndOfBody(self.start_height));
            *idle = Some(RangeConnection);
            Ok(self.body.take())
        }
        pub fn partial_body(&mut self) -> Option<&mut RangeBody> {
            self.body.as_mut()
        }
        pub fn cancel(&mut self, _: &mut TestSocket) {
            record(Event::Cancel(self.start_height));
        }
        pub fn wire_progress(&self) -> u64 {
            self.body_progress() as u64
        }
        pub fn body_progress(&self) -> usize {
            self.body.as_ref().map_or(0, RangeBody::len)
        }
    }
}

enum SyncPhase {
    Idle,
    WaitTimer,
    Range,
}
enum SyncRpcKind {
    GetBlock,
}
struct WalletService {
    daemon_tip_height: u64,
    scan_height: u64,
    bulk_unsupported: bool,
    tcp: TestSocket,
    node: (),
    node_ip: [u8; 4],
    bulk: Option<wallet_bulk::BulkScan>,
    bulk_begin: Option<(u64, u64)>,
    sync_phase: SyncPhase,
    tip_locked: bool,
    sync_rpc: SyncRpcKind,
    sync_rate_retries: u8,
    restore_chain: Option<RestoreChain>,
    terminal: Option<&'static [u8]>,
}
impl WalletService {
    fn start(replies: Vec<Reply>) -> Self {
        EVENTS.with_borrow_mut(Vec::clear);
        NS.with(|clock| clock.set(0));
        HEAP_USED.with(|heap| heap.set(0));
        let mut wallet = Self {
            daemon_tip_height: 10,
            scan_height: 1,
            bulk_unsupported: false,
            tcp: TestSocket(replies.into()),
            node: (),
            node_ip: [0; 4],
            bulk: None,
            bulk_begin: None,
            sync_phase: SyncPhase::Idle,
            tip_locked: true,
            sync_rpc: SyncRpcKind::GetBlock,
            sync_rate_retries: 0,
            restore_chain: Some(RestoreChain::new(1)),
            terminal: None,
        };
        wallet.start_bulk_scan(1);
        wallet
    }
    fn pump_until(&mut self, event: Event) {
        for _ in 0..16 {
            self.pump_bulk_scan();
            if history().contains(&event) {
                return;
            }
            assert!(self.terminal.is_none());
        }
        panic!("missing {event:?} in {:?}", history());
    }
    fn finish(&mut self) {
        for _ in 0..64 {
            self.pump_bulk_scan();
            if self.terminal.is_some() {
                return;
            }
        }
        panic!("did not finish; {:?}", history());
    }
    fn tcp_mut(&mut self) -> &mut TestSocket {
        &mut self.tcp
    }
    fn sync_fail(&mut self, reason: &'static [u8]) {
        self.terminal = Some(reason);
    }
    fn arm_get_block(&mut self, height: u64) {
        assert_eq!(self.restore_chain.as_ref().unwrap().next(), height);
        self.terminal = Some(b"json");
    }
    fn reset_restore_chain(&mut self) {
        record(Event::Reset);
        self.restore_chain.as_mut().unwrap().reset();
    }
    fn begin_restore_spent_checks(&mut self) {
        self.terminal = Some(b"tip");
    }
    fn publish_tip_scan(&mut self, _: u64, _: u64, _: &[u8]) {}
    fn commit_restore_block(
        &mut self,
        height: u64,
        block: Block,
        _: Vec<Transaction<Pruned>>,
        _: Option<u64>,
    ) -> Option<u64> {
        let chain = self.restore_chain.as_mut().unwrap();
        assert!(chain.commit(height, block.header.previous, block.hash()));
        record(Event::Commit(height));
        Some(chain.next())
    }
}

// Reuse the real v2 coinbase. Generate a tiny linked chain with one RingCT
// output per block, and wrap it in the daemon's EPEE block/index envelopes.
fn replies() -> Vec<Reply> {
    make_ranges(None, 6)
}

fn make_ranges(second_parent: Option<[u8; 32]>, second_index: u64) -> Vec<Reply> {
    use monero_wallet::transaction::Input;
    let template = real_core::decode_block_range(
        include_bytes!("fixtures/mainnet-3806161-range2.bin"),
        3_806_161,
        2,
    )
    .unwrap()
    .remove(0)
    .block;
    let mut header = template.header.clone();
    let mut miner = template.miner_transaction().clone();
    assert_eq!(miner.prefix().outputs.len(), 1);
    [(1u64, 5u8), (6, 2), (8, 2)]
        .into_iter()
        .map(|(start, count)| {
            let mut body = monero_epee::HEADER.to_vec();
            body.extend([monero_epee::VERSION, 5 << 2]);
            body.extend(b"\x06blocks\x8c");
            body.push(count << 2);
            if start == 6 {
                header.previous = second_parent.unwrap_or(header.previous);
            }
            for height in start..start + u64::from(count) {
                miner.prefix_mut().inputs = vec![Input::Gen(height as usize)];
                let block = Block::new(header.clone(), miner.clone(), vec![]).unwrap();
                header.previous = block.hash();
                let bytes = block.serialize();
                body.extend(b"\x08\x06pruned\x0b\x01\x05block\x0a");
                body.extend(((u16::try_from(bytes.len()).unwrap() << 2) | 1).to_le_bytes());
                body.extend(bytes);
            }
            let partial_len = body.len();
            body.extend(b"\x0eoutput_indices\x8c");
            body.push(count << 2);
            let first_index = if start == 6 { second_index } else { start };
            for index in first_index..first_index + u64::from(count) {
                body.extend(b"\x04\x07indices\x8c\x04\x04\x07indices\x85\x04");
                body.extend(index.to_le_bytes());
            }
            body.extend(b"\x06status\x0a\x08OK\x0cstart_height\x05");
            body.extend(start.to_le_bytes());
            body.extend(b"\x0ecurrent_height\x05");
            body.extend(10u64.to_le_bytes());
            Reply {
                start,
                pending: if start == 6 { 2 } else { 0 },
                partial_len,
                body,
                failure: None,
            }
        })
        .collect()
}

#[test]
fn rate_limited_range_waits_for_each_retry_and_then_finishes_in_order() {
    let mut script = replies();
    for _ in 0..3 {
        script.insert(
            0,
            Reply {
                start: 1,
                pending: 0,
                partial_len: 0,
                body: Vec::new(),
                failure: Some(wallet_range_fetch::RangeError::RateLimited),
            },
        );
    }
    let mut wallet = WalletService::start(script);
    for (attempt, seconds) in [60, 120, 240].into_iter().enumerate() {
        wallet.pump_bulk_scan();
        let delay = WAKE_NS.with(Cell::get);
        assert_eq!(delay, seconds * 1_000_000_000);
        wallet.pump_bulk_scan(); // A notification while the retry timer is outstanding.
        let attempts = history()
            .into_iter()
            .filter(|e| matches!(e, Event::Fetch(..)))
            .count();
        assert_eq!(attempts, attempt + 1);
        NS.with(|clock| clock.set(clock.get() + delay));
    }
    wallet.finish();
    assert_eq!(wallet.terminal, Some(b"tip".as_slice()));
    let committed: Vec<_> = history()
        .into_iter()
        .filter_map(|event| match event {
            Event::Commit(height) => Some(height),
            _ => None,
        })
        .collect();
    assert_eq!(committed, (1..10).collect::<Vec<_>>());
}

fn position(events: &[Event], event: Event) -> usize {
    events
        .iter()
        .position(|&seen| seen == event)
        .unwrap_or_else(|| panic!("missing {event:?} in {events:?}"))
}

#[test]
fn pacing_override_only_applies_to_healthy_reads_on_the_benchmark_node() {
    use wallet_bulk::benchmark_range_pace_ms as pace;
    for (setting, expected) in [("50", 50), ("100", 100)] {
        assert_eq!(
            pace(Some(setting), "RPC.NexaWal.com", 443, 0),
            Some(expected)
        );
        for host in ["other.example", "rpc.nexawal.com.example"] {
            assert_eq!(pace(Some(setting), host, 443, 0), None);
        }
        assert_eq!(pace(Some(setting), "rpc.nexawal.com", 18081, 0), None);
        for retries in 1..=u8::MAX {
            assert_eq!(pace(Some(setting), "rpc.nexawal.com", 443, retries), None);
            let seconds = match retries {
                1 => 60,
                2 => 120,
                _ => 240,
            };
            assert_eq!(
                scan_request_delay_ns(true, retries),
                Some(seconds * 1_000_000_000)
            );
            assert_eq!(scan_request_delay_ns(false, retries), None);
        }
    }
    for setting in [None, Some("250"), Some("0")] {
        assert_eq!(pace(setting, "rpc.nexawal.com", 443, 0), None);
    }
}

#[test]
fn validates_the_short_lookahead_before_drain_and_commits_in_order() {
    use Event::*;
    let mut wallet = WalletService::start(replies());
    wallet.finish();
    assert_eq!(wallet.terminal, Some(b"tip".as_slice()));
    let events = history();
    assert_eq!(
        events
            .iter()
            .copied()
            .filter(|e| matches!(e, Fetch(_, _)))
            .collect::<Vec<_>>(),
        [Fetch(1, 9), Fetch(6, 4), Fetch(8, 2)]
    );
    assert!(position(&events, Partial(6)) < position(&events, ValidatedBlock(6)));
    assert!(position(&events, ValidatedBlock(6)) < position(&events, EndOfBody(6)));
    assert!(
        position(&events, Decode(6, true)) < position(&events, Commit(5)),
        "{events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|e| if let Commit(h) = e { Some(*h) } else { None })
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
}

#[test]
fn primes_the_third_request_after_validation_and_holds_its_body_until_drain() {
    use Event::*;
    let mut script = replies();
    script[1].pending = 0;
    let mut wallet = WalletService::start(script);
    wallet.finish();
    let events = history();
    assert!(position(&events, Decode(6, true)) < position(&events, Fetch(8, 2)));
    assert!(position(&events, Prime(8)) < position(&events, Commit(5)));
    assert!(position(&events, Commit(5)) < position(&events, EndOfBody(8)));
    assert_eq!(wallet.terminal, Some(b"tip".as_slice()));
}

#[test]
fn cancelling_a_primed_request_releases_it_without_reading_its_body() {
    use Event::*;
    let mut script = replies();
    script[1].pending = 0;
    let mut wallet = WalletService::start(script);
    wallet.pump_until(Prime(8));
    let committed = wallet.restore_chain.as_ref().unwrap().next();
    wallet.interrupt_history_scan();
    wallet.cancel_bulk_scan();
    assert_eq!(wallet.restore_chain.as_ref().unwrap().next(), committed);
    assert!(history().contains(&Cancel(8)));
    assert!(!history().contains(&EndOfBody(8)));
}

#[test]
fn failed_early_write_retries_the_same_range_after_promoting_validated_blocks() {
    use Event::*;
    use wallet_range_fetch::{PeerClosed, RangeError};
    let mut script = replies();
    script[1].pending = 0;
    script.insert(2, Reply {
        start: 8, pending: 0, partial_len: 0, body: vec![],
        failure: Some(RangeError::Tls(PeerClosed)),
    });
    let mut wallet = WalletService::start(script);
    wallet.finish();
    let events = history();
    assert!(position(&events, Cancel(8)) < position(&events, Commit(5)));
    assert_eq!(events.iter().filter(|e| **e == Fetch(8, 2)).count(), 2);
    assert_eq!(events.iter().filter_map(|e| if let Commit(h) = e {Some(*h)} else {None})
                   .collect::<Vec<_>>(), (1..10).collect::<Vec<_>>());
    assert_eq!(wallet.terminal, Some(b"tip".as_slice()));
}

#[test]
fn a_permanent_write_failure_preserves_the_previously_validated_batch() {
    use Event::*;
    let mut script = replies();
    script[1].pending = 0;
    script[2].failure = Some(wallet_range_fetch::RangeError::Protocol);
    let mut wallet = WalletService::start(script);
    wallet.finish();
    assert_eq!(wallet.terminal, Some(b"NEXAWAL | SCAN FAIL | RANGE HTTPS".as_slice()));
    let events = history();
    assert!(position(&events, Prime(8)) < position(&events, Commit(5)));
    assert!(events.contains(&Commit(7)));
    assert!(!events.contains(&Commit(8)));
    assert_eq!(wallet.restore_chain.unwrap().next(), 8);
}

#[test]
fn diagnoses_malformed_lookahead_early_and_only_commits_the_verified_prefix() {
    use Event::*;
    let mut script = replies();
    script[1].body.pop();
    let mut wallet = WalletService::start(script);
    wallet.finish();
    assert_eq!(
        wallet.terminal,
        Some(b"NEXAWAL | SCAN FAIL | RANGE DATA".as_slice())
    );
    assert_eq!(wallet.restore_chain.unwrap().next(), 6);
    let events = history();
    assert!(
        position(&events, Decode(6, false)) < position(&events, Commit(5)),
        "{events:?}"
    );
    assert_eq!(events.iter().filter(|e| matches!(e, Fetch(..))).count(), 2);
}

#[test]
fn cancelling_a_validated_lookahead_keeps_only_the_committed_prefix() {
    use Event::*;
    let mut wallet = WalletService::start(replies());
    wallet.pump_until(Decode(6, true));
    let next = wallet.restore_chain.as_ref().unwrap().next();
    assert!(
        next <= 5,
        "lookahead was decoded only after preceding range drained"
    );
    wallet.interrupt_history_scan();
    wallet.cancel_bulk_scan();
    assert!(wallet.bulk.is_none());
    assert_eq!(wallet.restore_chain.unwrap().next(), next);
    assert!(!history().contains(&Fetch(8, 2)));
    assert!(history().contains(&ReleaseIdle));
}

#[test]
fn cancelling_a_partially_received_range_releases_the_slot() {
    use Event::*;
    let mut wallet = WalletService::start(replies());
    wallet.pump_until(Partial(6));
    let next = wallet.restore_chain.as_ref().unwrap().next();
    wallet.interrupt_history_scan();
    wallet.cancel_bulk_scan();
    let events = history();
    assert!(events.contains(&Cancel(6)));
    assert!(!events.contains(&EndOfBody(6)));
    assert_eq!(wallet.restore_chain.unwrap().next(), next);
}

#[test]
fn heap_pressure_drains_verified_work_then_resumes_json_at_the_first_pending_height() {
    use Event::*;
    let mut wallet = WalletService::start(replies());
    wallet.pump_until(Partial(6));
    HEAP_USED.with(|heap| heap.set(HEAP_BYTES - 4096));
    wallet.finish();
    let events = history();
    assert!(wallet.bulk_unsupported);
    assert_eq!(wallet.terminal, Some(b"json".as_slice()));
    assert_eq!(wallet.restore_chain.unwrap().next(), 6);
    assert!(!events.contains(&Decode(6, true)));
    assert!(!events.contains(&Commit(6)));
}

#[test]
fn rejects_a_valid_reply_on_a_different_chain_before_using_its_blocks() {
    use Event::*;
    let mut wallet = WalletService::start(make_ranges(Some([0x77; 32]), 6));
    wallet.finish();
    let events = history();
    assert_eq!(
        wallet.terminal,
        Some(b"NEXAWAL | SCAN FAIL | CHAIN CHANGED".as_slice())
    );
    assert!(position(&events, Decode(6, true)) < position(&events, Commit(5)));
    assert!(position(&events, Commit(5)) < position(&events, Reset));
    assert_eq!(wallet.restore_chain.unwrap().next(), 1);
    assert!(!events.contains(&Commit(6)));
    assert!(!events.contains(&Fetch(8, 2)));
}

#[test]
fn rejects_a_gap_between_individually_valid_ringct_index_sequences() {
    use Event::*;
    let mut wallet = WalletService::start(make_ranges(None, 19));
    wallet.finish();
    let events = history();
    assert_eq!(
        wallet.terminal,
        Some(b"NEXAWAL | SCAN FAIL | RANGE INDICES".as_slice())
    );
    assert!(position(&events, Decode(6, true)) < position(&events, Commit(5)));
    assert_eq!(wallet.restore_chain.unwrap().next(), 6);
    assert!(!events.contains(&Commit(6)));
    assert!(!events.contains(&Fetch(8, 2)));
}

#[test]
fn rejects_an_initial_reply_that_does_not_extend_the_saved_anchor() {
    use Event::*;
    let mut wallet = WalletService::start(replies());
    let chain = wallet.restore_chain.as_mut().unwrap();
    assert!(chain.seed_preceding_anchor(0, [0x77; 32]));
    wallet.finish();
    let events = history();
    assert_eq!(
        wallet.terminal,
        Some(b"NEXAWAL | SCAN FAIL | CHAIN CHANGED".as_slice())
    );
    assert!(position(&events, Decode(1, true)) < position(&events, Reset));
    assert_eq!(wallet.restore_chain.unwrap().next(), 1);
    assert!(!events.contains(&Commit(1)));
    assert!(!events.contains(&Fetch(6, 4)));
}

#[test]
fn stale_persistent_read_retries_same_height_on_a_fresh_connection() {
    use wallet_range_fetch::{PeerClosed, RangeError};
    let mut script = replies();
    script.insert(1, Reply {
        start: 6, pending: 0, partial_len: 0, body: Vec::new(),
        failure: Some(RangeError::Tls(PeerClosed)),
    });
    let mut wallet = WalletService::start(script);
    wallet.finish();
    assert_eq!(wallet.terminal, Some(b"tip".as_slice()));
    let events = history();
    assert_eq!(events.iter().filter(|e| **e == Event::Fetch(6, 4)).count(), 2);
    assert_eq!(events.iter().filter(|e| matches!(e, Event::Commit(_))).count(), 9);
}

#[test]
fn verifies_available_first_batch_blocks_before_its_tail_arrives() {
    let mut script = replies();
    script[0].pending = 1;
    let mut wallet = WalletService::start(script);
    wallet.pump_until(Event::Partial(1));
    assert_eq!(history().iter().filter(|e| **e == Event::ValidatedBlock(1)).count(), 3);
    assert!(!history().contains(&Event::EndOfBody(1)));
    assert_eq!(wallet.restore_chain.as_ref().unwrap().next(), 1);
    wallet.finish();
    assert_eq!(wallet.terminal, Some(b"tip".as_slice()));
}

#[test]
fn http_rate_limit_on_a_reused_connection_preserves_backoff() {
    let mut script = replies();
    script.insert(1, Reply {
        start: 6, pending: 0, partial_len: 0, body: Vec::new(),
        failure: Some(wallet_range_fetch::RangeError::RateLimited),
    });
    let mut wallet = WalletService::start(script);
    wallet.pump_until(Event::Reused(6));
    for _ in 0..10 { wallet.pump_bulk_scan(); }
    assert_eq!(history().iter().filter(|e| **e == Event::Fetch(6, 4)).count(), 1);
    assert!(WAKE_NS.with(Cell::get) > 30_000_000_000);
    NS.with(|clock| clock.set(clock.get() + 60_000_000_000));
    wallet.finish();
    assert_eq!(wallet.terminal, Some(b"tip".as_slice()));
}
