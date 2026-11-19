//! One range being committed and one bounded lookahead, from TLS through validation.
//! Only the wallet's ordinary block commit updates recovery state.
use super::{
    HEAP_BYTES, SYNC_TIMER_CHANNEL, SyncPhase, SyncRpcKind, WALLET_SHELL_CHANNEL, WalletService,
    heap_used_bytes, wallet_tls_platform,
};
use crate::wallet_range_fetch::{RangeConnection, RangeError as FetchError, RangeFetch};
use crate::wallet_range_plan::{BlockRange, RangePlan, continue_range_slice};
use alloc::vec::{IntoIter, Vec};
use sel4_microkit::{MessageInfo, debug_println, get_mr, set_mr};
use walletcore_core::{
    MAX_RANGE_BLOCKS, MAX_RANGE_BODY, RangeBlock, RangeBody, encode_blocks_request, scan_request_delay_ns,
};

fn clock_ns() -> u64 {
    let _ = SYNC_TIMER_CHANNEL.pp_call(MessageInfo::new(0, 0));
    get_mr(0)
}

pub(super) fn monotonic_ms() -> u64 {
    clock_ns() / 1_000_000
}

pub(super) fn clock_us() -> u64 {
    clock_ns() / 1_000
}

/// An opt-in interval for successful binary reads on the measured HTTPS node.
/// Retry deadlines are scheduled separately by `defer_retry`.
#[cfg(any(test, feature = "wallet-scan-benchmark"))]
pub(super) fn benchmark_range_pace_ms(
    setting: Option<&str>, host: &str, port: u16, retries: u8,
) -> Option<u64> {
    let ms = match setting {
        Some("50") => 50,
        Some("100") => 100,
        _ => return None,
    };
    (retries == 0 && port == 443 && host.eq_ignore_ascii_case("rpc.nexawal.com")).then_some(ms)
}

#[cfg(feature = "wallet-range-profile")]
#[derive(Default)]
pub(super) struct RangeProfile {
    start: u64,
    began_us: u64,
    work_us: u64,
    fetch_us: u64,
    decode_us: u64,
    commit_us: u64,
    pool_us: u64,
    // Time outside the wallet handler, classified by its state at timer arm.
    // A worker or socket can make progress during any of these intervals.
    wake: Option<(u64, RangeWait)>,
    wait_us: [u64; 4],
    transfer_start_us: u64,
    first_body_us: Option<u64>,
    pub(super) checkpoint_us: u64,
    pub(super) socket_signals: u64,
    pumps: u64,
    steps: u64,
    report_next: Option<u64>,
    #[cfg(feature = "wallet-scan-workers")]
    first_batch: Option<BatchOrigin>,
}

#[cfg(feature = "wallet-range-profile")]
#[derive(Clone, Copy)]
struct BatchOrigin {
    at: u64,
    fetch: u64,
    decode: u64,
    waits: [u64; 4],
}

#[cfg(feature = "wallet-range-profile")]
#[derive(Clone, Copy)]
enum RangeWait { Socket, Worker, Pace, Ready }

#[cfg(feature = "wallet-range-profile")]
impl RangeProfile {
    #[cfg(feature = "wallet-scan-workers")]
    fn first_batch(&mut self) {
        if self.first_batch.is_none() {
            debug_println!(
                "UIAPP|WALLET|RANGE|FIRST_WORK start={} fetch_us={} validate_us={} socket_wake_us={} ready_wake_us={} steps={} socket_signals={}",
                self.start, self.fetch_us, self.decode_us, self.wait_us[0], self.wait_us[3], self.steps, self.socket_signals
            );
            self.first_batch = Some(BatchOrigin {
                at: clock_us(), fetch: self.fetch_us, decode: self.decode_us, waits: self.wait_us,
            });
        }
    }

    #[cfg(feature = "wallet-scan-workers")]
    fn report_commit(&self, next: u64, end: u64, pool: &crate::wallet_scan_pool::PoolProfile) {
        let Some(origin) = self.first_batch else { return };
        let waits: [u64; 4] = core::array::from_fn(|i| self.wait_us[i] - origin.waits[i]);
        let wall = end - origin.at;
        let fetch = self.fetch_us - origin.fetch;
        let validation = self.decode_us - origin.decode;
        let other = wall.saturating_sub(fetch + validation + self.pool_us + self.commit_us + waits.iter().sum::<u64>());
        debug_println!(
            "UIAPP|WALLET|RANGE|POST_BATCH next={} wall_us={} fetch_us={} validation_us={} pool_us={} ledger_us={} checkpoint_us={} socket_wait_us={} worker_wait_us={} pace_wait_us={} ready_wait_us={} other_us={}",
            next, wall, fetch, validation, self.pool_us, self.commit_us - self.checkpoint_us,
            self.checkpoint_us, waits[0], waits[1], waits[2], waits[3], other
        );
        debug_println!(
            "UIAPP|WALLET|RANGE|POOL next={} dispatch_us={} harvest_us={} encode_us={} local_us={} result_decode_us={} result_check_us={}",
            next, pool.dispatch_us, pool.harvest_us, pool.encode_us, pool.local_us, pool.decode_us, pool.check_us
        );
        debug_println!(
            "UIAPP|WALLET|RANGE|REMOTE next={} jobs={} decode_us={} scan_us={} encode_us={}",
            next, pool.worker_jobs, pool.worker_us[0], pool.worker_us[1], pool.worker_us[2]
        );
    }

    fn finish_step(&mut self, entered: u64) {
        let now = clock_us();
        self.work_us += now - entered;
        self.steps += 1;
        if let Some(next) = self.report_next.take() {
            // fetch/decode/commit are disjoint wall intervals on the wallet thread.
            // Checkpoint persistence is nested inside commit; report it separately.
            debug_println!(
                "UIAPP|WALLET|RANGE|PROFILE next={} wall_us={} work_us={} fetch_us={} decode_us={} scan_commit_us={} checkpoint_us={} pumps={} steps={}",
                next,
                now - self.began_us,
                self.work_us,
                self.fetch_us,
                self.decode_us,
                self.pool_us + self.commit_us - self.checkpoint_us,
                self.checkpoint_us,
                self.pumps,
                self.steps
            );
            // The serial scanner performs its ownership check inside commit_restore_block.
            // Only the scan-pool path exposes ownership and ledger as separate intervals.
            #[cfg(feature = "wallet-scan-workers")]
            debug_println!(
                "UIAPP|WALLET|RANGE|PHASE next={} pool_us={} ledger_us={} socket_wait_us={} worker_wait_us={} pace_wait_us={} ready_wait_us={}",
                next, self.pool_us, self.commit_us - self.checkpoint_us,
                self.wait_us[0], self.wait_us[1], self.wait_us[2], self.wait_us[3]
            );
        }
    }
}

fn wake_after(ms: u64) {
    set_mr(0, ms.max(1).saturating_mul(1_000_000));
    let _ = SYNC_TIMER_CHANNEL.pp_call(MessageInfo::new(1, 1));
}

pub(super) struct BulkScan {
    plan: RangePlan,
    scanning: IntoIter<RangeBlock>,
    #[cfg(feature = "wallet-scan-workers")]
    workers: crate::wallet_scan_pool::ScanPool,
    #[cfg(feature = "wallet-scan-benchmark")]
    transcript: sha2::Sha256,
    #[cfg(feature = "wallet-scan-benchmark")]
    scan_start: u64,
    fetch: Option<RangeFetch>,
    deferred_fetch_error: Option<FetchError>,
    idle_connection: Option<RangeConnection>,
    lookahead: Lookahead,
    // These follow validated replies, independently of the ordered commit cursor.
    validated_parent: Option<[u8; 32]>,
    validated_rct: Option<u64>,
    #[cfg(feature = "wallet-scan-benchmark")]
    overlap_blocks: usize,
    earliest_fetch_ms: u64,
    retries: u8,
    next_rct: Option<u64>,
    last_publish_ms: u64,
}

enum Lookahead {
    Empty,
    Body(RangeBody),
    Ready { blocks: Vec<RangeBlock>, bytes: usize },
    Json,
    ChainChanged,
    Failed(&'static [u8]),
}

enum Progress {
    Continue(u64),
    Json,
    CheckTip,
    Failed,
}

impl BulkScan {
    fn new(start: u64, tip: u64, now: u64) -> Self {
        Self {
            plan: RangePlan::new(start, tip, MAX_RANGE_BLOCKS),
            scanning: Vec::new().into_iter(),
            #[cfg(feature = "wallet-scan-workers")]
            workers: crate::wallet_scan_pool::ScanPool::new(),
            #[cfg(feature = "wallet-scan-benchmark")]
            transcript: Default::default(),
            #[cfg(feature = "wallet-scan-benchmark")]
            scan_start: start,
            fetch: None,
            deferred_fetch_error: None,
            idle_connection: None,
            lookahead: Lookahead::Empty,
            validated_parent: None,
            validated_rct: None,
            #[cfg(feature = "wallet-scan-benchmark")]
            overlap_blocks: 0,
            earliest_fetch_ms: now,
            retries: 0,
            next_rct: None,
            last_publish_ms: now,
        }
    }

    #[cfg(feature = "wallet-https")]
    pub(super) fn adopt_preflight(&mut self, session: nexatrode_guest_https::Session,
                                  node: walletcore_core::NodeEndpoint, ip: [u8; 4]) {
        self.idle_connection = Some(RangeConnection::from_preflight(session, node, ip));
    }

    fn remaining(&self) -> usize {
        #[cfg(feature = "wallet-scan-workers")]
        { self.scanning.len() + self.workers.pending() }
        #[cfg(not(feature = "wallet-scan-workers"))]
        { self.scanning.len() }
    }

    fn cancel(&mut self, wallet: &mut WalletService) {
        if let Some(mut fetch) = self.fetch.take() {
            fetch.cancel(wallet.tcp_mut());
        }
        if self.idle_connection.take().is_some() {
            let _ = wallet.tcp_mut().reset();
        }
    }

    fn defer_retry(&mut self, wallet: &mut WalletService, now: u64, rate_limited: bool) {
        self.retries = self.retries.saturating_add(1);
        let delay = scan_request_delay_ns(true, self.retries).unwrap() / 1_000_000;
        self.earliest_fetch_ms = now.saturating_add(delay);
        let label: &[u8] = if rate_limited {
            b"RATE WAIT"
        } else {
            b"NETWORK WAIT"
        };
        wallet.publish_tip_scan(wallet.daemon_tip_height, wallet.scan_height, label);
        WALLET_SHELL_CHANNEL.notify();
    }

    fn handle_error(
        &mut self,
        wallet: &mut WalletService,
        req: BlockRange,
        error: FetchError,
        now: u64,
    ) -> Result<(), &'static [u8]> {
        self.cancel(wallet);
        debug_println!(
            "UIAPP|WALLET|RANGE|ERROR start={} count={} kind={:?}",
            req.start,
            req.count,
            error
        );
        if error.too_large() {
            if !self.plan.reduce(req) {
                self.lookahead = Lookahead::Json;
                wallet.bulk_unsupported = true;
            }
        } else if error.unsupported() {
            wallet.bulk_unsupported = true;
            self.lookahead = Lookahead::Json;
        } else if error.transient() {
            self.defer_retry(wallet, now, error.rate_limited());
        } else {
            self.lookahead = Lookahead::Failed(b"NEXAWAL | SCAN FAIL | RANGE HTTPS");
        }
        Ok(())
    }

    fn poll_fetch(&mut self, wallet: &mut WalletService, now: u64) -> Result<(), &'static [u8]> {
        #[cfg(feature = "wallet-range-profile")]
        let started = clock_us();
        let result = self.poll_range_transport(wallet, now);
        #[cfg(feature = "wallet-range-profile")]
        {
            wallet.range_profile.fetch_us += clock_us() - started;
        }
        result
    }

    fn poll_range_transport(
        &mut self,
        wallet: &mut WalletService,
        now: u64,
    ) -> Result<(), &'static [u8]> {
        let Some(req) = self.plan.request() else {
            return Ok(());
        };
        // A ready lookahead is fully validated, so the plan already knows the
        // exact successor height (also for short replies). Send its request on
        // the idle connection while scanning. Read it only after promoting
        // this lookahead, keeping the existing one-response allocation bound.
        let prime = match &self.lookahead {
            Lookahead::Empty => false,
            Lookahead::Ready { .. } if self.remaining() != 0 => true,
            _ => return Ok(()),
        };
        if prime {
            if self.deferred_fetch_error.is_some() || (self.fetch.is_none() && self.idle_connection.is_none()) {
                return Ok(());
            }
        } else if let Some(error) = self.deferred_fetch_error.take() {
            return self.handle_error(wallet, req, error, now);
        }
        if self.fetch.is_none() {
            if now < self.earliest_fetch_ms {
                return Ok(());
            }
            #[cfg(feature = "wallet-scan-benchmark")]
            { self.overlap_blocks = 0; }
            let node = wallet.node;
            let ip = wallet.node_ip;
            let body = encode_blocks_request(req.start, req.count);
            let unix = wallet_tls_platform::unix_time();
            debug_println!(
                "UIAPP|WALLET|RANGE|FETCH start={} count={} unscanned={} ms={}",
                req.start,
                req.count,
                self.remaining() + match &self.lookahead {
                    Lookahead::Ready { blocks, .. } => blocks.len(),
                    _ => 0,
                },
                now
            );
            let interval_ms = scan_request_delay_ns(true, 0).unwrap() / 1_000_000;
            #[cfg(feature = "wallet-scan-benchmark")]
            let interval_ms = benchmark_range_pace_ms(
                if cfg!(feature = "wallet-handoff-prime") { Some("100") } else { option_env!("NEXAWAL_BENCH_HEALTHY_MS") },
                node.host(), node.port, self.retries,
            ).unwrap_or(interval_ms);
            self.earliest_fetch_ms = now.saturating_add(interval_ms);
            #[cfg(feature = "wallet-range-profile")]
            {
                wallet.range_profile.transfer_start_us = clock_us();
                wallet.range_profile.first_body_us = None;
            }
            match RangeFetch::start(
                wallet.tcp_mut(),
                node,
                ip,
                req.start,
                req.count,
                &body,
                unix,
                now,
                &mut self.idle_connection,
            ) {
                Ok(fetch) => {
                    #[cfg(feature = "wallet-scan-benchmark")]
                    debug_println!("UIAPP|WALLET|RANGE|CONNECTION start={} reused={}", req.start, fetch.reused);
                    self.fetch = Some(fetch);
                }
                Err(error) if prime => {
                    self.deferred_fetch_error = Some(error);
                    return Ok(());
                }
                Err(error) => return self.handle_error(wallet, req, error, now),
            }
        }

        let fetch = self.fetch.as_mut().unwrap();
        if fetch.start_height != req.start || fetch.count != req.count {
            return Err(b"NEXAWAL | SCAN FAIL | RANGE STATE");
        }
        let result = if prime {
            fetch.prime_request(wallet.tcp_mut(), now).map(|flushed| {
                if cfg!(feature = "wallet-scan-benchmark") && flushed {
                    debug_println!("UIAPP|WALLET|RANGE|PRIMED start={} ms={}", req.start, now);
                }
                None
            })
        } else {
            fetch.step(wallet.tcp_mut(), now, &mut self.idle_connection)
        };
        #[cfg(feature = "wallet-range-profile")]
        if wallet.range_profile.first_body_us.is_none() && fetch.body_progress() > 0 {
            wallet.range_profile.first_body_us = Some(clock_us());
        }
        match result {
            Ok(Some(body)) => {
                #[cfg(feature = "wallet-range-profile")]
                {
                    let finished = clock_us();
                    let start = wallet.range_profile.transfer_start_us;
                    debug_println!(
                        "UIAPP|WALLET|RANGE|TRANSFER start={} bytes={} elapsed_us={} first_body_us={}",
                        req.start, body.len(), finished - start,
                        wallet.range_profile.first_body_us.unwrap_or(finished) - start
                    );
                }
                self.fetch = None;
                self.lookahead = Lookahead::Body(body);
            }
            Ok(None) => {}
            Err(FetchError::Tls(kind)) if fetch.reused && fetch.body_progress() == 0 && kind.transient() => {
                // An idle HTTP/1.1 connection can close between requests. Retry
                // this read on a fresh socket at the existing pacing deadline.
                // A failed fresh attempt follows the regular timed backoff.
                self.cancel(wallet);
            }
            Err(error) if prime => {
                self.cancel(wallet);
                self.deferred_fetch_error = Some(error);
            }
            Err(error) => self.handle_error(wallet, req, error, now)?,
        }
        Ok(())
    }

    /// Keep the raw response and all decoded blocks together until its HTTP
    /// framing, daemon status, ancestry and output indices have been checked.
    fn validate_lookahead(
        &mut self,
        wallet: &mut WalletService,
        now: u64,
    ) -> Result<(), &'static [u8]> {
        // A write for the successor may already be in flight behind a
        // validated lookahead. Its body stays untouched until promotion.
        if matches!(self.lookahead, Lookahead::Ready { .. }) { return Ok(()); }
        let remaining = self.remaining();
        if let Some(body) = self.fetch.as_mut().and_then(RangeFetch::partial_body) {
            // Account for the unfinished body's possible growth. The other half
            // of the heap is reserved for decoder scratch, scans and checkpoints.
            #[cfg(feature = "wallet-range-profile")]
            let started = clock_us();
            // Check a short burst even if the current wave has committed.
            // This overlaps hashing with the remainder of the download,
            // including the first batch, while leaving time to service workers.
            for _ in 0..3 {
                if heap_used_bytes().saturating_add(MAX_RANGE_BODY.saturating_sub(body.capacity())) > HEAP_BYTES / 2 {
                    break;
                }
                #[cfg(feature = "wallet-scan-benchmark")]
                let running = self.workers.running();
                if !body.validate_next() {
                    break;
                }
                #[cfg(feature = "wallet-scan-benchmark")]
                if running != 0 || self.workers.running() != 0 {
                    self.overlap_blocks += 1;
                }
            }
            #[cfg(feature = "wallet-range-profile")]
            { wallet.range_profile.decode_us += clock_us() - started; }
            return Ok(());
        }

        if !matches!(self.lookahead, Lookahead::Body(_)) {
            return Ok(());
        }
        if heap_used_bytes() > HEAP_BYTES / 2 {
            if remaining == 0 {
                self.lookahead = Lookahead::Json;
                wallet.bulk_unsupported = true;
            }
            return Ok(());
        }
        let Lookahead::Body(body) = core::mem::replace(&mut self.lookahead, Lookahead::Empty)
        else {
            unreachable!()
        };
        let req = self
            .plan
            .request()
            .ok_or(b"NEXAWAL | SCAN FAIL | RANGE STATE".as_slice())?;
        let bytes = body.len();
        #[cfg(feature = "wallet-scan-benchmark")]
        let (early, running) = (body.validated_blocks(), self.workers.running());
        #[cfg(feature = "wallet-range-profile")]
        let started = clock_us();
        let decoded = body.finish();
        #[cfg(feature = "wallet-range-profile")]
        {
            wallet.range_profile.decode_us += clock_us() - started;
        }
        let blocks = match decoded {
            Ok(blocks) => blocks,
            Err(error) => {
                debug_println!(
                    "UIAPP|WALLET|RANGE|DECODE_ERROR start={} kind={:?}",
                    req.start,
                    error
                );
                if error.is_busy() {
                    self.defer_retry(wallet, now, false);
                } else if !(error.is_limit() && self.plan.reduce(req)) {
                    if error.is_limit() || error.needs_full_transactions() {
                        self.lookahead = Lookahead::Json;
                        wallet.bulk_unsupported = true;
                    } else {
                        self.lookahead = Lookahead::Failed(b"NEXAWAL | SCAN FAIL | RANGE DATA");
                    }
                }
                return Ok(());
            }
        };
        let (Some(first), Some(last)) = (blocks.first(), blocks.last()) else {
            return Err(b"NEXAWAL | SCAN FAIL | RANGE COUNT");
        };
        let parent = first.block.header.previous;
        let links_to_predecessor = match self.validated_parent {
            Some(hash) => hash == parent,
            None => wallet
                .restore_chain
                .as_ref()
                .is_some_and(|chain| chain.accepts(req.start, parent)),
        };
        if !links_to_predecessor {
            self.lookahead = Lookahead::ChainChanged;
            return Ok(());
        }
        let first_index = blocks.iter().find_map(|block| block.first_rct);
        if self
            .validated_rct
            .zip(first_index)
            .is_some_and(|(expected, found)| expected != found)
        {
            self.lookahead = Lookahead::Failed(b"NEXAWAL | SCAN FAIL | RANGE INDICES");
            return Ok(());
        }
        if !self.plan.accept(req, blocks.len()) {
            return Err(b"NEXAWAL | SCAN FAIL | RANGE COUNT");
        }
        self.validated_parent = Some(last.block.hash());
        self.validated_rct = blocks
            .iter()
            .rev()
            .find_map(|block| block.next_rct)
            .or(self.validated_rct);
        self.retries = 0;
        debug_println!(
            "UIAPP|WALLET|RANGE|BATCH start={} blocks={} bytes={}",
            req.start,
            blocks.len(),
            bytes
        );
        #[cfg(all(feature = "wallet-range-profile", feature = "wallet-scan-workers"))]
        wallet.range_profile.first_batch();
        #[cfg(feature = "wallet-scan-benchmark")]
        debug_println!(
            "UIAPP|WALLET|RANGE|VALIDATION start={} scan_remaining={} worker_pending={} worker_running={} early_blocks={} running_overlap={}",
            req.start,
            remaining,
            self.workers.pending_remote(),
            running.max(self.workers.running()),
            early,
            self.overlap_blocks
        );
        self.lookahead = Lookahead::Ready { blocks, bytes };
        Ok(())
    }

    fn advance(&mut self, wallet: &mut WalletService, now: u64) -> Result<Progress, &'static [u8]> {
        #[cfg(feature = "wallet-scan-workers")]
        // Collect completed work and refill idle lanes while earlier heights await commit.
        if self.remaining() != 0 {
            let view = wallet
                .state
                .view_pair()
                .ok_or(b"NEXAWAL | SCAN NO VIEW".as_slice())?;
            #[cfg(feature = "wallet-range-profile")]
            let before = clock_us();
            self.workers.dispatch(view, &mut self.scanning, now);
            #[cfg(feature = "wallet-range-profile")]
            {
                let elapsed = clock_us() - before;
                wallet.range_profile.pool_us += elapsed;
                self.workers.profile.dispatch_us += elapsed;
            }
        }
        self.poll_fetch(wallet, now)?;
        self.validate_lookahead(wallet, now)?;
        if self.remaining() == 0 {
            match core::mem::replace(&mut self.lookahead, Lookahead::Empty) {
                Lookahead::Ready { blocks, .. } => {
                    self.scanning = blocks.into_iter();
                    // Dispatch workers and open the following request on the next
                    // productive step in the same bounded wallet timer slice.
                    return Ok(Progress::Continue(1));
                }
                Lookahead::Json => return Ok(Progress::Json),
                Lookahead::ChainChanged => {
                    wallet.reset_restore_chain();
                    return Err(b"NEXAWAL | SCAN FAIL | CHAIN CHANGED");
                }
                Lookahead::Failed(error) => return Err(error),
                Lookahead::Empty => {}
                Lookahead::Body(_) => unreachable!(),
            }
        }
        if self.remaining() != 0 {
            #[cfg(feature = "wallet-scan-workers")]
            let (height, first_rct, next_rct, result) = {
                let view = wallet.state.view_pair().ok_or(b"NEXAWAL | SCAN NO VIEW".as_slice())?;
                #[cfg(feature = "wallet-range-profile")]
                let before = clock_us();
                let result = self.workers.next(view, now);
                #[cfg(feature = "wallet-range-profile")]
                { wallet.range_profile.pool_us += clock_us() - before; }
                let Some(done) = result? else {
                    return Ok(Progress::Continue(1));
                };
                (done.block.height, done.first_rct, done.next_rct, done.block)
            };
            #[cfg(not(feature = "wallet-scan-workers"))]
            let (height, first_rct, next_rct, item) = {
                let item = self.scanning.next().unwrap();
                (u64::try_from(item.block.number()).map_err(|_| b"NEXAWAL | SCAN FAIL | BAD HEIGHT".as_slice())?,
                    item.first_rct, item.next_rct, item)
            };
            #[cfg(feature = "wallet-range-profile")]
            let started = clock_us();
            wallet.scan_height = height;
            if let Some(first) = first_rct {
                if self.next_rct.is_some_and(|expected| expected != first) {
                    return Err(b"NEXAWAL | SCAN FAIL | RANGE INDICES");
                }
                self.next_rct = next_rct;
            }
            #[cfg(not(feature = "wallet-scan-workers"))]
            let committed = wallet.commit_restore_block(height, item.block, item.txs, first_rct);
            #[cfg(feature = "wallet-scan-workers")]
            let committed = {
                #[cfg(feature = "wallet-scan-benchmark")]
                {
                    use sha2::Digest;
                    self.transcript.update(height.to_le_bytes());
                    self.transcript.update(result.hash);
                    self.transcript.update((result.outputs.len() as u64).to_le_bytes());
                    for output in &result.outputs {
                        let serialized = zeroize::Zeroizing::new(output.serialize());
                        self.transcript.update((serialized.len() as u64).to_le_bytes());
                        self.transcript.update(&*serialized);
                    }
                }
                wallet.commit_scan_result(result)
            };
            #[cfg(feature = "wallet-range-profile")]
            {
                wallet.range_profile.commit_us += clock_us() - started;
            }
            let Some(next) = committed else {
                return Ok(Progress::Failed);
            };
            #[cfg(all(feature = "wallet-range-profile", feature = "wallet-scan-workers"))]
            let profile_end = (next - wallet.range_profile.start == 222).then(clock_us);
            #[cfg(feature = "wallet-scan-benchmark")]
            if next - self.scan_start == 222 || next == wallet.daemon_tip_height {
                use sha2::Digest;
                debug_println!("UIAPP|WALLET|RANGE|WORKERS next={} counts={:?} fallback={} overlap={} digest={:x}",
                    next, self.workers.counts, self.workers.fallbacks, self.workers.overlaps,
                    self.transcript.clone().finalize());
            }
            #[cfg(feature = "wallet-range-profile")]
            if self.remaining() == 0 || next - wallet.range_profile.start == 222 {
                wallet.range_profile.report_next = Some(next);
            }
            if self.remaining() == 0 {
                let wire = self.fetch.as_ref().filter(|fetch| fetch.start_height == next)
                    .map_or(0, RangeFetch::wire_progress);
                let (body, ready) = match &self.lookahead {
                    Lookahead::Ready { bytes, .. } => (*bytes, true),
                    Lookahead::Body(body) => (body.len(), false),
                    _ => (self.fetch.as_ref().map_or(0, RangeFetch::body_progress), false),
                };
                debug_println!(
                    "UIAPP|WALLET|RANGE|COMMIT next={} prefetch_wire={} prefetch_body={} prefetch_ready={}",
                    next,
                    wire,
                    body,
                    ready
                );
            }
            #[cfg(all(feature = "wallet-range-profile", feature = "wallet-scan-workers"))]
            if let Some(end) = profile_end {
                wallet.range_profile.report_commit(next, end, &self.workers.profile);
            }
            if next == wallet.daemon_tip_height {
                return Ok(Progress::CheckTip);
            }
            if now.saturating_sub(self.last_publish_ms) >= 150 {
                wallet.publish_tip_scan(wallet.daemon_tip_height, height, &[]);
                self.last_publish_ms = now;
                WALLET_SHELL_CHANNEL.notify();
            }
        }
        // Protect space for TLS, the next decoded range and the journal. A wallet
        // carrying many owned outputs can continue via the smaller JSON requests.
        if self.remaining() == 0 && self.fetch.is_none() && matches!(self.lookahead, Lookahead::Empty) && heap_used_bytes() > HEAP_BYTES / 2 {
            self.lookahead = Lookahead::Json;
            wallet.bulk_unsupported = true;
        }
        let delay = if self.remaining() == 0
            && matches!(self.lookahead, Lookahead::Empty)
            && self.fetch.is_none()
        {
            self.earliest_fetch_ms.saturating_sub(now).max(1)
        } else {
            1
        };
        Ok(Progress::Continue(delay))
    }
}

impl WalletService {
    /// Protected calls run between complete block commits. Release the TCP slot
    /// on the next notification to avoid a nested network call from the shell.
    pub(super) fn interrupt_history_scan(&mut self) {
        if matches!(self.sync_phase, SyncPhase::WaitTimer | SyncPhase::Range) {
            self.sync_phase = SyncPhase::Idle;
            self.tip_locked = false;
            self.sync_rate_retries = 0;
            self.bulk_begin = None;
        }
    }

    pub(super) fn start_bulk_scan(&mut self, height: u64) {
        let now = monotonic_ms();
        if self.bulk_begin.is_none() {
            #[cfg(feature = "wallet-range-profile")]
            {
                self.range_profile = RangeProfile {
                    start: height,
                    began_us: clock_us(),
                    ..RangeProfile::default()
                };
            }
            self.bulk_begin = Some((height, now));
            debug_println!(
                "UIAPP|WALLET|RANGE|BEGIN start={} tip={} ms={}",
                height,
                self.daemon_tip_height,
                now
            );
        }
        self.scan_height = height;
        self.sync_rpc = SyncRpcKind::GetBlock;
        self.sync_phase = SyncPhase::Range;
        self.bulk = Some(BulkScan::new(height, self.daemon_tip_height, now));
        self.publish_tip_scan(self.daemon_tip_height, height, &[]);
        wake_after(1);
    }

    pub(super) fn cancel_bulk_scan(&mut self) {
        if let Some(mut scan) = self.bulk.take() {
            scan.cancel(self);
        }
        self.bulk_begin = None;
    }

    pub(super) fn pump_bulk_scan(&mut self) {
        let Some(mut scan) = self.bulk.take() else {
            self.sync_fail(b"NEXAWAL | SCAN FAIL | RANGE STATE");
            return;
        };
        let started = clock_us();
        let mut entered = started;
        let mut steps = 0;
        #[cfg(feature = "wallet-range-profile")]
        {
            self.range_profile.pumps += 1;
            if let Some((armed, reason)) = self.range_profile.wake.take() {
                self.range_profile.wait_us[reason as usize] += started - armed;
            }
        }
        let outcome = loop {
            let before = scan
                .fetch
                .as_ref()
                .map(|f| (f.start_height, f.wire_progress()));
            let outcome = scan.advance(self, entered / 1_000);
            #[cfg(feature = "wallet-range-profile")]
            self.range_profile.finish_step(entered);
            if !matches!(outcome, Ok(Progress::Continue(1))) {
                break outcome;
            }
            steps += 1;
            let received_bytes = scan.fetch.as_ref().is_some_and(|f| {
                let previous = before.filter(|(start, _)| *start == f.start_height);
                f.wire_progress() > previous.map_or(0, |(_, wire)| wire)
            });
            let mut ready = scan.remaining() != 0 || !matches!(scan.lookahead, Lookahead::Empty);
            #[cfg(feature = "wallet-scan-workers")]
            if scan.workers.head_waiting() {
                // A completed lookahead cannot be consumed before this wave.
                // Give the scheduler a turn if network progress has also stopped.
                ready = false;
            }
            let ready = ready || received_bytes;
            entered = clock_us();
            if !continue_range_slice(ready, steps, entered - started) {
                break outcome;
            }
        };
        match outcome {
            Ok(Progress::Continue(delay)) => {
                #[cfg(feature = "wallet-range-profile")]
                {
                    let mut reason = if scan.remaining() != 0 || !matches!(scan.lookahead, Lookahead::Empty) {
                        RangeWait::Ready
                    } else if scan.fetch.is_some() {
                        RangeWait::Socket
                    } else {
                        RangeWait::Pace
                    };
                    #[cfg(feature = "wallet-scan-workers")]
                    if scan.workers.head_waiting() {
                        reason = RangeWait::Worker;
                    }
                    self.range_profile.wake = Some((clock_us(), reason));
                }
                self.bulk = Some(scan);
                wake_after(delay);
                return;
            }
            Ok(Progress::Json) => {
                scan.cancel(self);
                let height = self.restore_chain.as_ref().unwrap().next();
                debug_println!("UIAPP|WALLET|RANGE|JSON_FALLBACK start={}", height);
                self.arm_get_block(height);
            }
            Ok(Progress::CheckTip) => {
                scan.cancel(self);
                self.tip_locked = false;
                self.begin_restore_spent_checks();
            }
            Ok(Progress::Failed) => scan.cancel(self),
            Err(reason) => {
                scan.cancel(self);
                self.sync_fail(reason);
            }
        }
        WALLET_SHELL_CHANNEL.notify();
    }
}
