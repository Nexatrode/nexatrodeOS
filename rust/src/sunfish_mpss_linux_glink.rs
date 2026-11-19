//! GLINK memory delegation inside the original retained SMP2P/SMEM owner.
//! No channel negotiation, cursor reset, doorbell or ready permit is provided.
#![forbid(unsafe_code)]
use super::*;
use crate::sunfish_mpss_linux_loader::AuthReturned;
use core::sync::atomic::{AtomicU64, Ordering};
const DESC: Item = memory::ITEMS[1].1;
const TX: Item = memory::ITEMS[2].1;
const RX_ID: u16 = 480;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(all(test, mpss_glink_tests))]
#[path = "../../tests/sunfish_mpss_linux_glink_scope.rs"]
mod native_scope;
// This opt-in changes the sampling granularity of peer-owned FIFO memory.
// Every aligned word stays entirely inside the caller's validated span; the
// caller retains its lock, barriers, and independent record comparisons.
#[cfg(feature = "sunfish-glink-aligned-fifo-reads")]
fn read_fifo_span(io: &impl crate::qcom_modem_status::Read, at: usize, out: &mut [u8]) {
    let mut n = 0;
    while n < out.len() && (at + n) % 4 != 0 {
        out[n] = io.read8(at + n);
        n += 1;
    }
    while out.len() - n >= 4 {
        out[n..n + 4].copy_from_slice(&io.read_le32(at + n).to_le_bytes());
        n += 4;
    }
    while n < out.len() {
        out[n] = io.read8(at + n);
        n += 1;
    }
}

fn identity() -> u64 {
    let mut old = NEXT_ID.load(Ordering::Relaxed);
    loop {
        let Some(next) = old.checked_add(1) else {
            return 0;
        };
        match NEXT_ID.compare_exchange_weak(old, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return old,
            Err(actual) => old = actual,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlinkError {
    Consumed,
    Predecessor,
    Guard,
    Directory,
    Identity,
    Layout,
    Access,
    Cursor,
    Readback,
    Unstable,
    NotReady,
    Lock,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlinkPoll {
    Waiting,
    Unstable,
    Available,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlinkReport {
    pub generation: u64,
    pub identity: u64,
    pub rx: Option<Item>,
    pub tx_head: u32,
    pub rx_tail: u32,
    pub stores_sent: u64,
    pub stores_done: u64,
    pub tx_bytes_written: u64,
    pub error: Option<GlinkError>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Tx,
    TxHead,
    RxTail,
}
/// Closed transport store: byte data only in item479, or one AP-owned cursor.
pub struct GlinkWrite<'a> {
    kind: Kind,
    offset: usize,
    bytes: &'a [u8],
    value: u32,
}
pub struct GlinkRx {
    item: Item,
}
impl GlinkRx {
    pub fn item(&self) -> Item {
        self.item
    }
}
impl GlinkWrite<'_> {
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
    pub fn value(&self) -> u32 {
        self.value
    }
    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }
}
pub trait GlinkPayload: SmpIo {
    fn claim_glink(&mut self) -> Result<(), GlinkError>;
    fn bind_glink_rx(&mut self, extent: &GlinkRx) -> Result<(), GlinkError>;
    fn glink_descriptor(&mut self, word: usize) -> Result<u32, GlinkError>;
    fn glink_store(&mut self, request: &GlinkWrite<'_>) -> Result<(), GlinkError>;
}
pub(super) struct State {
    _auth: ManuallyDrop<AuthReturned>,
    report: GlinkReport,
}
impl<I: ipa::Io, P: GlinkPayload, S: Signals> Controller<I, P, S> {
    fn glink_poison(&mut self, e: GlinkError) -> GlinkError {
        if let Some(g) = self.glink.as_mut() {
            g.report.error.get_or_insert(e);
        }
        self.fail(Error::Guard);
        e
    }
    fn glink_live(&mut self) -> Result<(), GlinkError> {
        if self.report.phase != Phase::Active
            || self.report.error.is_some()
            || self.glink.as_ref().is_none_or(|g| g.report.error.is_some())
        {
            return Err(GlinkError::Consumed);
        }
        let o = self.observe(false).map_err(|_| GlinkError::Guard)?;
        if o.lifecycle < Lifecycle::AuthReturned || o.power != Power::Active {
            return Err(GlinkError::Guard);
        }
        Ok(())
    }
    fn glink_directory(&mut self) -> Result<(), GlinkError> {
        self.outbound().map_err(|_| GlinkError::Guard)?;
        let (_, extra) = self.directory_with_extra(false, Some(RX_ID)).map_err(|_| GlinkError::Directory)?;
        let a = self.free_uncached;
        let b = self.free_cached;
        let d = extra.ok_or(GlinkError::Directory)?;
        if d.route
            != (dir::Route {
                index: 7,
                offset: memory::PART,
                size: memory::PART_BYTES,
                cacheline: 32,
            })
            || d.free_uncached != a
            || d.free_cached != b
        {
            return Err(GlinkError::Directory);
        }
        let old = self.glink.as_ref().unwrap().report.rx;
        if old.is_some() && old != d.item {
            return Err(GlinkError::Identity);
        }
        if let Some(item) = d.item {
            if item.size < 8
                || item.size % 8 != 0
                || item.offset % 8 != 0
                || item.offset < memory::PART + memory::FINAL_FREE
                || item
                    .offset
                    .checked_add(item.size)
                    .is_none_or(|e| e > memory::PART + memory::PART_BYTES)
            {
                return Err(GlinkError::Layout);
            }
            if self.report.peer_item.is_some_and(|p| {
                item.offset < p.offset + p.size && p.offset < item.offset + item.size
            }) {
                return Err(GlinkError::Layout);
            }
            if old.is_none() {
                self.parts()
                    .map_err(|_| GlinkError::Predecessor)?
                    .1
                    .bind_glink_rx(&GlinkRx { item })?;
                self.glink.as_mut().unwrap().report.rx = Some(item);
            }
        }
        Ok(())
    }
    fn glink_words(&mut self) -> Result<[u32; 8], GlinkError> {
        let mut words = [0; 8];
        for (n, v) in words.iter_mut().enumerate() {
            *v = self
                .parts()
                .map_err(|_| GlinkError::Predecessor)?
                .1
                .glink_descriptor(n)?;
        }
        let r = self.glink.as_ref().unwrap().report;
        if words[1] != r.tx_head
            || words[2] != r.rx_tail
            || words[4..] != [0; 4]
            || words[0] as usize >= TX.size
            || words[..4].iter().any(|v| v % 8 != 0)
            || r.rx.is_some_and(|rx| words[3] as usize >= rx.size)
            || (r.rx.is_none() && words[3] != 0)
        {
            return Err(GlinkError::Cursor);
        }
        Ok(words)
    }
    fn glink_locked<T>(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<T, GlinkError>,
    ) -> Result<T, GlinkError> {
        if self.busy {
            return Err(self.glink_poison(GlinkError::Consumed));
        }
        self.busy = true;
        if let Err(e) = self
            .glink_live()
            .and_then(|_| self.acquire().map_err(|_| GlinkError::Lock))
        {
            return Err(self.glink_poison(e));
        }
        let result = (|| {
            self.glink_directory()?;
            self.glink_words()?;
            self.glink_live()?;
            if !self.owned().map_err(|_| GlinkError::Lock)? {
                return Err(GlinkError::Lock);
            }
            let r = action(self)?;
            self.glink_live()?;
            self.glink_directory()?;
            self.glink_words()?;
            Ok(r)
        })();
        if self.release().is_err() {
            return Err(self.glink_poison(GlinkError::Lock));
        }
        match result {
            Ok(v) => {
                self.busy = false;
                Ok(v)
            }
            Err(e) => Err(self.glink_poison(e)),
        }
    }
    fn glink_sample_peer(&mut self) -> Result<bool, GlinkError> {
        self.glink_live().map_err(|e| self.glink_poison(e))?;
        match self.poll() {
            Ok(Poll::Unstable) => Ok(false),
            Ok(_) => Ok(true),
            Err(_) => Err(self.glink_poison(GlinkError::Guard)),
        }
    }
    /// Consumes the actual successful loader receipt. The original extension,
    /// IPA497 allocation and native writer stay inside this same Controller.
    /// This claims only memory access, not a negotiated channel or ready bit.
    pub fn claim_glink(&mut self, auth: AuthReturned) -> Result<(), GlinkError> {
        let auth = ManuallyDrop::new(auth);
        if self.glink.is_some() {
            return Err(self.glink_poison(GlinkError::Consumed));
        }
        let generation = auth.generation();
        let identity = identity();
        self.glink = Some(State {
            _auth: auth,
            report: GlinkReport {
                generation,
                identity,
                rx: None,
                tx_head: 0,
                rx_tail: 0,
                stores_sent: 0,
                stores_done: 0,
                tx_bytes_written: 0,
                error: None,
            },
        });
        if generation != self.generation || identity == 0 {
            return Err(self.glink_poison(GlinkError::Predecessor));
        }
        // No payload/cursor stores here. A changing peer can be sampled again
        // by glink_poll before any transport I/O; uncertainty after I/O stops.
        let _ = self.glink_sample_peer()?;
        self.glink_locked(|s| {
            s.parts()
                .map_err(|_| GlinkError::Predecessor)?
                .1
                .claim_glink()
        })
    }
    pub fn glink_report(&self) -> Option<GlinkReport> {
        self.glink.as_ref().map(|g| {
            let mut r = g.report;
            if self.busy && r.error.is_none() {
                r.error = Some(GlinkError::Consumed);
            }
            r
        })
    }
    pub fn glink_poll(&mut self) -> Result<GlinkPoll, GlinkError> {
        if !self.glink_sample_peer()? {
            return Ok(GlinkPoll::Unstable);
        }
        self.glink_locked(|s| {
            Ok(if s.glink.as_ref().unwrap().report.rx.is_some() {
                GlinkPoll::Available
            } else {
                GlinkPoll::Waiting
            })
        })
    }
    fn glink_io<T>(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<T, GlinkError>,
    ) -> Result<T, GlinkError> {
        if !self.glink_sample_peer()? {
            return Err(self.glink_poison(GlinkError::Unstable));
        }
        self.glink_locked(action)
    }
    pub fn glink_descriptor(&mut self) -> Result<[u32; 8], GlinkError> {
        self.glink_io(|s| s.glink_words())
    }
    /// Preserve the local length lookup before the live descriptor read.
    /// Caller brackets the complete read-only operation with domain validation.
    #[cfg(feature = "sunfish-glink-combined-cursor-sample")]
    pub fn glink_cursor_snapshot(&mut self) -> Result<(Option<usize>, [u32; 8]), GlinkError> {
        let report = self.glink_report().ok_or(GlinkError::Predecessor)?;
        if report.error.is_some() { return Err(GlinkError::Consumed); }
        let length = report.rx.map(|item| item.size);
        Ok((length, self.glink_descriptor()?))
    }
    pub fn glink_barrier(&mut self) -> Result<(), GlinkError> {
        self.glink_io(|s| {
            s.parts().map_err(|_| GlinkError::Predecessor)?.0.barrier();
            Ok(())
        })
    }
    pub fn glink_read_tx(&mut self, at: usize, out: &mut [u8]) -> Result<(), GlinkError> {
        self.glink_read(false, at, out)
    }
    pub fn glink_read_rx(&mut self, at: usize, out: &mut [u8]) -> Result<(), GlinkError> {
        self.glink_read(true, at, out)
    }
    fn glink_read(&mut self, rx: bool, at: usize, out: &mut [u8]) -> Result<(), GlinkError> {
        self.glink_io(|s| {
            let item = if rx {
                s.glink
                    .as_ref()
                    .unwrap()
                    .report
                    .rx
                    .ok_or(GlinkError::NotReady)?
            } else {
                TX
            };
            if out.is_empty() || at.checked_add(out.len()).is_none_or(|e| e > item.size) {
                return Err(GlinkError::Layout);
            }
            let (io, _) = s.parts().map_err(|_| GlinkError::Predecessor)?;
            io.barrier();
            #[cfg(feature = "sunfish-glink-aligned-fifo-reads")]
            read_fifo_span(io, item.offset + at, out);
            #[cfg(not(feature = "sunfish-glink-aligned-fifo-reads"))]
            for (n, v) in out.iter_mut().enumerate() {
                *v = io.read8(item.offset + at + n);
            }
            io.barrier();
            Ok(())
        })
    }
    pub fn glink_write_tx(&mut self, at: usize, bytes: &[u8]) -> Result<(), GlinkError> {
        self.glink_io(|s| {
            if bytes.is_empty() || at.checked_add(bytes.len()).is_none_or(|e| e > TX.size) {
                return Err(GlinkError::Layout);
            }
            let r = GlinkWrite {
                kind: Kind::Tx,
                offset: at,
                bytes,
                value: 0,
            };
            s.glink_store(&r)
        })
    }
    pub fn glink_store_tx_head(&mut self, value: u32) -> Result<(), GlinkError> {
        self.glink_cursor(true, value)
    }
    pub fn glink_store_rx_tail(&mut self, value: u32) -> Result<(), GlinkError> {
        self.glink_cursor(false, value)
    }
    fn glink_cursor(&mut self, tx: bool, value: u32) -> Result<(), GlinkError> {
        self.glink_io(|s| {
            let length = if tx {
                TX.size
            } else {
                s.glink
                    .as_ref()
                    .unwrap()
                    .report
                    .rx
                    .ok_or(GlinkError::NotReady)?
                    .size
            };
            if value % 8 != 0 || value as usize >= length {
                return Err(GlinkError::Cursor);
            }
            let r = GlinkWrite {
                kind: if tx { Kind::TxHead } else { Kind::RxTail },
                offset: if tx { 1 } else { 2 },
                bytes: &[],
                value,
            };
            s.glink_store(&r)
        })
    }
    fn glink_store(&mut self, r: &GlinkWrite<'_>) -> Result<(), GlinkError> {
        self.glink_live()?;
        if !self.owned().map_err(|_| GlinkError::Lock)? {
            return Err(GlinkError::Lock);
        }
        self.glink.as_mut().unwrap().report.stores_sent += 1;
        self.parts()
            .map_err(|_| GlinkError::Predecessor)?
            .1
            .glink_store(r)?;
        let (io, _) = self.parts().map_err(|_| GlinkError::Predecessor)?;
        io.barrier();
        match r.kind {
            Kind::Tx => {
                for (n, v) in r.bytes.iter().enumerate() {
                    if io.read8(TX.offset + r.offset + n) != *v {
                        return Err(GlinkError::Readback);
                    }
                }
            }
            Kind::TxHead | Kind::RxTail => {
                let mut actual = [0; 4];
                for (n, v) in actual.iter_mut().enumerate() {
                    *v = io.read8(DESC.offset + r.offset * 4 + n);
                }
                if u32::from_le_bytes(actual) != r.value {
                    return Err(GlinkError::Readback);
                }
            }
        }
        if !self.owned().map_err(|_| GlinkError::Lock)? {
            return Err(GlinkError::Lock);
        }
        let report = &mut self.glink.as_mut().unwrap().report;
        report.stores_done += 1;
        match r.kind {
            Kind::Tx => report.tx_bytes_written += r.bytes.len() as u64,
            Kind::TxHead => report.tx_head = r.value,
            Kind::RxTail => report.rx_tail = r.value,
        }
        Ok(())
    }
}
impl<I: ipa::Io, P: GlinkPayload, S: GlinkSignals> Controller<I, P, S> {
    pub fn glink_notify(&mut self) -> Result<(), GlinkError> {
        self.glink_io(|s| s.signals.notify_glink().map_err(|_| GlinkError::Access))
    }
}
