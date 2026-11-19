//! Bounded QRTR transmit-credit model for the pinned Linux flow semantics.
//!
//! This is an offline accounting component: it does not publish a packet,
//! release a GLINK buffer, or establish peer receipt. A caller must retain
//! each GLINK buffer until its actual receive-done event, and must quarantine
//! uncertain publication. Closed destinations remain fenced for the boot so
//! a delayed resume cannot grant credit to a reused port.
#![forbid(unsafe_code)]
use core::sync::atomic::{AtomicU64, Ordering};
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

pub const LOW_WATERMARK: u8 = 5;
pub const HIGH_WATERMARK: u8 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Destination {
    pub node: u32,
    pub port: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidGeneration,
    InvalidDestination,
    Capacity,
    Blocked,
    Closed,
    Stale,
    Quarantined,
    SequenceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reservation {
    owner: u64,
    generation: u64,
    sequence: u64,
    destination: Destination,
    confirm: bool,
}
impl Reservation {
    pub const fn confirm_rx(&self) -> bool { self.confirm }
    pub const fn destination(&self) -> Destination { self.destination }
    pub const fn sequence(&self) -> u64 { self.sequence }
}

#[derive(Clone, Copy)]
struct Flow {
    destination: Destination,
    pending: u8,
    retry_confirm: bool,
    closed: bool,
}

/// All counters and outstanding tokens are bounded; no allocation fallback
/// can silently change the wire flag when the table is full.
pub struct Table<const N: usize> {
    owner: u64,
    generation: u64,
    sequence: u64,
    flows: [Option<Flow>; N],
    active: [Option<Reservation>; N],
    quarantined: bool,
}
impl<const N: usize> Table<N> {
    pub fn new(generation: u64) -> Result<Self, Error> {
        if generation == 0 { return Err(Error::InvalidGeneration); }
        let mut owner = NEXT_OWNER.load(Ordering::Relaxed);
        loop {
            let next = owner.checked_add(1).ok_or(Error::SequenceExhausted)?;
            match NEXT_OWNER.compare_exchange_weak(owner, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => owner = actual,
            }
        }
        Ok(Self { owner, generation, sequence: 1, flows: [None; N], active: [None; N], quarantined: false })
    }
    fn live(&self) -> Result<(), Error> {
        if self.quarantined { Err(Error::Quarantined) } else { Ok(()) }
    }
    fn flow(&self, destination: Destination) -> Option<usize> {
        self.flows.iter().position(|f| f.is_some_and(|f| f.destination == destination))
    }
    pub fn contains(&self, destination: Destination) -> bool {
        self.flow(destination).is_some()
    }
    #[cfg(feature = "sunfish-tftp-oack-resources")]
    pub(crate) fn pending_for_diagnostic(&self, destination: Destination) -> Option<u8> {
        self.flows[self.flow(destination)?].filter(|flow| !flow.closed).map(|flow| flow.pending)
    }
    pub fn usage(&self) -> (usize, usize) {
        (self.flows.iter().flatten().count(), self.active.iter().flatten().count())
    }
    /// Reserve a DATA send. The caller must finish or fail exactly this token.
    /// A known pre-publication failure is distinct from uncertain publication.
    pub fn reserve(&mut self, destination: Destination) -> Result<Reservation, Error> {
        self.reserve_with_legacy_confirmation(destination, false)
    }
    /// Account for a legacy sender which always requests confirmation. It
    /// shares the same destination counter as reference-matched sends.
    pub fn reserve_with_legacy_confirmation(&mut self, destination: Destination, force: bool) -> Result<Reservation, Error> {
        self.live()?;
        if destination.node >= 0xffff_fffe || destination.port == 0 || destination.port >= 0xffff_fffe {
            return Err(Error::InvalidDestination);
        }
        let active_slot = self.active.iter().position(Option::is_none).ok_or(Error::Capacity)?;
        let index = match self.flow(destination) {
            Some(index) => index,
            None => {
                let index = self.flows.iter().position(Option::is_none).ok_or(Error::Capacity)?;
                self.flows[index] = Some(Flow { destination, pending: 0, retry_confirm: false, closed: false });
                index
            }
        };
        let flow = self.flows[index].as_mut().ok_or(Error::Stale)?;
        if flow.closed { return Err(Error::Closed); }
        if flow.pending >= HIGH_WATERMARK && !flow.retry_confirm { return Err(Error::Blocked); }
        let next = self.sequence.checked_add(1).ok_or(Error::SequenceExhausted)?;
        let confirm = if flow.retry_confirm {
            flow.retry_confirm = false;
            true
        } else {
            flow.pending += 1;
            force || flow.pending == LOW_WATERMARK
        };
        let reservation = Reservation { owner: self.owner, generation: self.generation, sequence: self.sequence, destination, confirm };
        self.sequence = next;
        self.active[active_slot] = Some(reservation);
        Ok(reservation)
    }
    fn token_slot(&self, token: &Reservation) -> Result<usize, Error> {
        self.live()?;
        if token.owner != self.owner || token.generation != self.generation { return Err(Error::Stale); }
        self.active.iter().position(|value| value.as_ref() == Some(token)).ok_or(Error::Stale)
    }
    /// Retire only the accounting token after the caller has observed actual
    /// GLINK completion. This does not claim application-level processing.
    pub fn finish_after_glink(&mut self, token: &Reservation) -> Result<(), Error> {
        let slot = self.token_slot(token)?;
        self.active[slot] = None;
        Ok(())
    }
    /// A provably unpublished send preserves Linux's pending count; if it was
    /// the confirmation-bearing send, the next send requests confirmation.
    pub fn fail_unpublished(&mut self, token: &Reservation) -> Result<(), Error> {
        let slot = self.token_slot(token)?;
        let index = self.flow(token.destination).ok_or(Error::Stale)?;
        if token.confirm { self.flows[index].as_mut().ok_or(Error::Stale)?.retry_confirm = true; }
        self.active[slot] = None;
        Ok(())
    }
    /// Unknown publication can have affected the peer; never replay it.
    pub fn fail_uncertain(&mut self, token: &Reservation) -> Result<(), Error> {
        self.token_slot(token)?;
        self.quarantined = true;
        Ok(())
    }
    /// A validated RESUME_TX resets a destination's counter, not a message.
    pub fn resume(&mut self, destination: Destination) -> Result<bool, Error> {
        self.live()?;
        let Some(index) = self.flow(destination) else { return Ok(false); };
        let flow = self.flows[index].as_mut().ok_or(Error::Stale)?;
        if flow.closed { return Ok(false); }
        flow.pending = 0;
        Ok(true)
    }
    /// Do not reuse a destination within this boot after close; outstanding
    /// transmissions must be resolved before applying the fence.
    pub fn close(&mut self, destination: Destination) -> Result<(), Error> {
        self.live()?;
        if self.active.iter().flatten().any(|r| r.destination == destination) { return Err(Error::Blocked); }
        if destination.node >= 0xffff_fffe || destination.port == 0 || destination.port >= 0xffff_fffe {
            return Err(Error::InvalidDestination);
        }
        let index = match self.flow(destination) {
            Some(index) => index,
            None => {
                let index = self.flows.iter().position(Option::is_none).ok_or(Error::Capacity)?;
                self.flows[index] = Some(Flow { destination, pending: 0, retry_confirm: false, closed: false });
                index
            }
        };
        self.flows[index].as_mut().ok_or(Error::Stale)?.closed = true;
        Ok(())
    }
}
