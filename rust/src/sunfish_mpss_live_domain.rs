//! Shared owner primitives for the post-auth MPSS runtime.
//!
//! The gate is moved between owners by value. It never exposes a borrow that
//! can outlive the closure that acquired it, and a nested borrow permanently
//! quarantines the owner.
#![forbid(unsafe_code)]

use core::cell::{Cell, RefCell};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateError {
    BorrowConflict,
    Quarantined,
}

pub struct NativeGate<T> {
    value: RefCell<T>,
    quarantined: Cell<bool>,
}

impl<T> NativeGate<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: RefCell::new(value),
            quarantined: Cell::new(false),
        }
    }

    pub fn try_borrow_mut<R>(&self, action: impl FnOnce(&mut T) -> R) -> Result<R, GateError> {
        if self.quarantined.get() {
            return Err(GateError::Quarantined);
        }
        let mut value = match self.value.try_borrow_mut() {
            Ok(value) => value,
            Err(_) => {
                self.quarantined.set(true);
                return Err(GateError::BorrowConflict);
            }
        };
        let result = action(&mut value);
        drop(value);
        if self.quarantined.get() {
            Err(GateError::Quarantined)
        } else {
            Ok(result)
        }
    }

    pub fn quarantine(&self) {
        self.quarantined.set(true);
    }

    pub fn is_quarantined(&self) -> bool {
        self.quarantined.get()
    }
}

pub trait ReceiptGeneration {
    fn generation(&self) -> u64;
}

impl ReceiptGeneration for crate::sunfish_mpss_linux_loader::AuthReturned {
    fn generation(&self) -> u64 {
        crate::sunfish_mpss_linux_loader::AuthReturned::generation(self)
    }
}

impl ReceiptGeneration for crate::sunfish_mpss_linux_smp2p::ReadyObservation {
    fn generation(&self) -> u64 {
        crate::sunfish_mpss_linux_smp2p::ReadyObservation::generation(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoffError {
    NotReady,
    MissingReceipt,
    WrongGeneration,
    Consumed,
    Quarantined,
}

/// One-use holder for the actual PAS-returned and delivered-ready receipts.
/// Invalid handoffs retain both values and latch quarantine instead of
/// consuming or dropping uncertain boot owners.
pub struct ReceiptHandoff<A, R> {
    generation: u64,
    auth: Option<A>,
    ready: Option<R>,
    consumed: bool,
    quarantined: bool,
}

impl<A, R> ReceiptHandoff<A, R>
where
    A: ReceiptGeneration,
    R: ReceiptGeneration,
{
    pub const fn new(generation: u64, auth: Option<A>, ready: Option<R>) -> Self {
        Self {
            generation,
            auth,
            ready,
            consumed: false,
            quarantined: false,
        }
    }

    pub fn take(&mut self, phase_ready: bool) -> Result<(A, R), HandoffError> {
        if self.consumed {
            self.quarantined = true;
            return Err(HandoffError::Consumed);
        }
        if self.quarantined {
            return Err(HandoffError::Quarantined);
        }
        if !phase_ready {
            self.quarantined = true;
            return Err(HandoffError::NotReady);
        }
        let (Some(auth), Some(ready)) = (self.auth.as_ref(), self.ready.as_ref()) else {
            self.quarantined = true;
            return Err(HandoffError::MissingReceipt);
        };
        if auth.generation() != self.generation || ready.generation() != self.generation {
            self.quarantined = true;
            return Err(HandoffError::WrongGeneration);
        }
        self.consumed = true;
        Ok((self.auth.take().unwrap(), self.ready.take().unwrap()))
    }

    pub fn retained(&self) -> (bool, bool) {
        (self.auth.is_some(), self.ready.is_some())
    }

    pub fn into_retained(self) -> (Option<A>, Option<R>) {
        (self.auth, self.ready)
    }

    pub fn is_quarantined(&self) -> bool {
        self.quarantined
    }
}
