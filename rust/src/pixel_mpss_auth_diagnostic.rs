//! Observation-only auth diagnostics. This does not route or acknowledge IRQs.
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(super) enum Failure {
    None = 0,
    Receive = 1,
    Domain = 2,
    Preauth = 3,
    Rmtfs = 4,
}

pub(super) struct Report {
    failure: AtomicU8,
    seen: AtomicU64,
}

impl Report {
    pub(super) const fn new() -> Self {
        Self { failure: AtomicU8::new(0), seen: AtomicU64::new(0) }
    }
    pub(super) fn fail(&self, failure: Failure) {
        if failure != Failure::None {
            let _ = self.failure.compare_exchange(0, failure as u8, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
    pub(super) fn failure(&self) -> Failure {
        match self.failure.load(Ordering::Relaxed) {
            1 => Failure::Receive,
            2 => Failure::Domain,
            3 => Failure::Preauth,
            4 => Failure::Rmtfs,
            _ => Failure::None,
        }
    }
    pub(super) fn observe(&self, badge: u64) {
        self.seen.fetch_or(badge, Ordering::Relaxed);
    }
    pub(super) fn seen(&self) -> u64 {
        self.seen.load(Ordering::Relaxed)
    }
}
