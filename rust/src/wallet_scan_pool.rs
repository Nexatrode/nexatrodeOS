//! Scan a bounded window of verified blocks, committing only its oldest height.
use crate::wallet_scan_mailbox::{Completion, Mailbox};
use alloc::collections::VecDeque;
use alloc::vec::IntoIter;
use sel4_microkit::{Channel, var};
use walletcore_core::{
    MAX_SCAN_JOB, MAX_SCAN_RESULT, RangeBlock, ScannedBlock, ViewPair, decode_scan_result,
    encode_scan_job, scan_verified_block,
};
use zeroize::Zeroizing;

type Slot = Mailbox<MAX_SCAN_JOB, MAX_SCAN_RESULT>;
type Error = &'static [u8];
fn scan_error(error: walletcore_core::OwnershipError) -> Error {
    let _ = error;
    b"NEXAWAL | SCAN FAIL"
}
const _: () = assert!(core::mem::size_of::<Slot>() <= 0x400000);

const WINDOW: usize = 8;

enum Work {
    Local {
        speculative: bool,
    },
    Remote {
        lane: usize,
        since_ms: u64,
    },
    Buffered {
        lane: usize,
        bytes: Zeroizing<alloc::vec::Vec<u8>>,
        #[cfg(feature = "wallet-range-profile")]
        times: [u64; 3],
    },
    Scanned(Result<Finished, Error>),
}

struct Pending {
    // Until the job has been scanned locally or checked against its worker reply.
    original: Option<RangeBlock>,
    work: Work,
}

pub(super) struct Finished {
    pub block: ScannedBlock,
    pub first_rct: Option<u64>,
    pub next_rct: Option<u64>,
}

#[cfg(feature = "wallet-range-profile")]
#[derive(Default)]
pub(super) struct PoolProfile {
    pub dispatch_us: u64,
    pub harvest_us: u64,
    pub encode_us: u64,
    pub local_us: u64,
    pub decode_us: u64,
    pub check_us: u64,
    pub worker_us: [u64; 3],
    pub worker_jobs: u64,
}

#[cfg(feature = "wallet-range-profile")]
use crate::wallet_profile_clock::now_us;

pub(super) struct ScanPool {
    #[cfg(feature = "wallet-range-profile")]
    pub profile: PoolProfile,
    slots: [Option<&'static Slot>; 3],
    pending: VecDeque<Pending>,
    pub counts: [u64; 4],
    pub fallbacks: u64,
    pub overlaps: u64,
}

// The same conservative receipt allocation estimate as execute_scan_job. Hold
// at most one speculative local result and defer large local scans to the head.
fn small_local(item: &RangeBlock) -> bool {
    let mut bytes = item.block.transactions.len().saturating_mul(32);
    core::iter::once(item.block.miner_transaction().prefix())
        .chain(item.txs.iter().map(|tx| tx.prefix()))
        .all(|tx| {
            let receipt = 512usize.saturating_add(tx.extra.len().saturating_mul(16));
            bytes = bytes.saturating_add(tx.outputs.len().saturating_mul(receipt));
            bytes <= 256 * 1024
        })
}

impl ScanPool {
    pub fn new() -> Self {
        let addrs = [
            *var!(scan_mailbox_1: usize = 0),
            *var!(scan_mailbox_2: usize = 0),
            *var!(scan_mailbox_3: usize = 0),
        ];
        Self {
            #[cfg(feature = "wallet-range-profile")]
            profile: PoolProfile::default(),
            slots: addrs.map(|addr| unsafe { (addr as *const Slot).as_ref() }),
            pending: VecDeque::with_capacity(WINDOW),
            counts: [0; 4],
            fallbacks: 0,
            overlaps: 0,
        }
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    fn speculative_local(&self) -> Option<usize> {
        self.pending
            .iter()
            .position(|p| matches!(p.work, Work::Local { speculative: true }))
    }

    pub fn head_waiting(&self) -> bool {
        self.pending.front().is_some_and(|p| match p.work {
            Work::Remote { lane, .. } => {
                !self.slots[lane].unwrap().finished() && self.speculative_local().is_none()
            }
            _ => false,
        })
    }

    #[cfg(feature = "wallet-scan-benchmark")]
    pub fn pending_remote(&self) -> usize {
        self.pending
            .iter()
            .filter(|p| matches!(p.work, Work::Remote { .. }))
            .count()
    }

    #[cfg(feature = "wallet-scan-benchmark")]
    pub fn running(&self) -> usize {
        self.slots
            .iter()
            .flatten()
            .filter(|slot| slot.running())
            .count()
    }

    // Free completed secondary mailboxes even while the head is running. Retain
    // only encoded bytes; decode each receipt just before its ordered commit.
    fn harvest(&mut self) {
        let mut buffered: usize = self
            .pending
            .iter()
            .map(|p| match &p.work {
                Work::Buffered { bytes, .. } => bytes.len(),
                _ => 0,
            })
            .sum();
        for pending in self.pending.iter_mut().skip(1) {
            // Each new result is at most MAX_SCAN_RESULT. Total stays below
            // twice that cap regardless of how many owned outputs are encoded.
            if buffered >= MAX_SCAN_RESULT {
                break;
            }
            let Work::Remote { lane, .. } = pending.work else {
                continue;
            };
            let slot = self.slots[lane].unwrap();
            if !slot.finished() {
                continue;
            }
            #[cfg(feature = "wallet-range-profile")]
            let times = slot.worker_times();
            let result = slot.take(|raw| {
                let mut bytes = Zeroizing::new(alloc::vec::Vec::new());
                bytes.try_reserve_exact(raw.len()).ok()?;
                bytes.extend_from_slice(raw);
                Some(bytes)
            });
            let bytes = match result {
                Completion::Waiting => continue,
                Completion::Ready(Some(bytes)) => bytes,
                _ => Zeroizing::new(alloc::vec::Vec::new()),
            };
            buffered += bytes.len();
            pending.work = Work::Buffered {
                lane, bytes,
                #[cfg(feature = "wallet-range-profile")]
                times,
            };
        }
    }

    pub fn dispatch(&mut self, view: &ViewPair, blocks: &mut IntoIter<RangeBlock>, now: u64) {
        if blocks.len() == 0 {
            return;
        }
        #[cfg(feature = "wallet-range-profile")]
        let before = now_us();
        self.harvest();
        #[cfg(feature = "wallet-range-profile")]
        { self.profile.harvest_us += now_us() - before; }
        if self.pending.is_empty() {
            self.pending.push_back(Pending {
                original: blocks.next(),
                work: Work::Local { speculative: false },
            });
        }
        for (lane, slot) in self.slots.iter().enumerate() {
            if self.pending.len() >= WINDOW {
                break;
            }
            let Some(slot) = slot else { continue };
            let Some(item) = blocks.as_slice().first() else {
                break;
            };
            if !slot.available() {
                continue;
            }
            #[cfg(feature = "wallet-range-profile")]
            let before = now_us();
            let submitted = slot.submit(|buf| encode_scan_job(view, item, buf).ok());
            #[cfg(feature = "wallet-range-profile")]
            { self.profile.encode_us += now_us() - before; }
            if !submitted {
                // submit already cleared its buffer. Keep this item in order for
                // local execution instead of encoding it again on another lane.
                self.pending.push_back(Pending {
                    original: blocks.next(),
                    work: Work::Local { speculative: false },
                });
                break;
            }
            self.pending.push_back(Pending {
                original: blocks.next(),
                work: Work::Remote {
                    lane,
                    since_ms: now,
                },
            });
            Channel::new(16 + lane).notify();
        }
        if self.pending.len() < WINDOW
            && !self
                .pending
                .iter()
                .any(|p| matches!(p.work, Work::Local { .. } | Work::Scanned(_)))
            && let Some(item) = blocks.next()
        {
            let speculative = small_local(&item);
            self.pending.push_back(Pending {
                original: Some(item),
                work: Work::Local { speculative },
            });
        }
    }

    fn scan_local(&mut self, view: &ViewPair, idx: usize) {
        let item = self.pending[idx].original.take().unwrap();
        let first_rct = item.first_rct;
        let next_rct = item.next_rct;
        let running = self.slots.iter().flatten().any(|slot| slot.running());
        #[cfg(feature = "wallet-range-profile")]
        let before = now_us();
        let result = scan_verified_block(view, item).map_err(scan_error);
        #[cfg(feature = "wallet-range-profile")]
        { self.profile.local_us += now_us() - before; }
        if result.is_ok() && (running || self.slots.iter().flatten().any(|slot| slot.running())) {
            self.overlaps += 1;
        }
        self.pending[idx].work = Work::Scanned(result.map(|block| Finished {
            block,
            first_rct,
            next_rct,
        }));
    }

    pub fn next(&mut self, view: &ViewPair, now: u64) -> Result<Option<Finished>, Error> {
        let Some(head) = self.pending.front() else {
            return Ok(None);
        };
        if let Work::Remote { lane, since_ms } = head.work {
            if !self.slots[lane].unwrap().finished() && now.saturating_sub(since_ms) < 2000 {
                if let Some(idx) = self.speculative_local() {
                    self.scan_local(view, idx);
                }
                return Ok(None);
            }
        }
        if matches!(head.work, Work::Local { .. }) {
            self.scan_local(view, 0);
        }
        let head = self.pending.pop_front().unwrap();
        #[cfg(feature = "wallet-range-profile")]
        let decode_start = now_us();
        #[cfg(feature = "wallet-range-profile")]
        let times;
        let (lane, reply) = match head.work {
            Work::Scanned(done) => {
                let done = done?;
                self.counts[0] += 1;
                return Ok(Some(done));
            }
            Work::Remote { lane, .. } => {
                let slot = self.slots[lane].unwrap();
                #[cfg(feature = "wallet-range-profile")]
                // Acquire publication inside take, including the timeout boundary.
                let mut measured = [0; 3];
                let reply = match slot.take(|bytes| {
                    #[cfg(feature = "wallet-range-profile")]
                    { measured = slot.worker_times(); }
                    decode_scan_result(bytes)
                }) {
                    Completion::Ready(Ok(block)) => Some(block),
                    _ => {
                        slot.cancel();
                        None
                    }
                };
                #[cfg(feature = "wallet-range-profile")]
                { times = measured; }
                (lane, reply)
            }
            Work::Buffered { lane, bytes, #[cfg(feature = "wallet-range-profile")] times: measured } => {
                #[cfg(feature = "wallet-range-profile")]
                { times = measured; }
                (lane, decode_scan_result(&bytes).ok())
            }
            Work::Local { .. } => unreachable!(),
        };
        #[cfg(feature = "wallet-range-profile")]
        let checking = {
            let t = now_us();
            self.profile.decode_us += t - decode_start;
            t
        };
        let original = head.original.unwrap();
        let first_rct = original.first_rct;
        let next_rct = original.next_rct;
        let block = match reply {
            Some(block)
                if walletcore_core::validate_scanned_block(&block, &original) =>
            {
                #[cfg(feature = "wallet-range-profile")]
                {
                    self.profile.check_us += now_us() - checking;
                    for (sum, us) in self.profile.worker_us.iter_mut().zip(times) { *sum += us; }
                    self.profile.worker_jobs += 1;
                }
                self.counts[lane + 1] += 1;
                block
            }
            _ => {
                self.fallbacks += 1;
                self.counts[0] += 1;
                #[cfg(feature = "wallet-range-profile")]
                let before = {
                    let t = now_us();
                    self.profile.check_us += t - checking;
                    t
                };
                let block = scan_verified_block(view, original).map_err(scan_error)?;
                #[cfg(feature = "wallet-range-profile")]
                { self.profile.local_us += now_us() - before; }
                block
            }
        };
        Ok(Some(Finished {
            block,
            first_rct,
            next_rct,
        }))
    }
}

impl Drop for ScanPool {
    fn drop(&mut self) {
        for pending in &self.pending {
            if let Work::Remote { lane, .. } = pending.work {
                self.slots[lane].unwrap().cancel();
            }
        }
    }
}
