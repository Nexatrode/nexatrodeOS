//! A single wallet and a single worker exchange one bounded job per mapping.
//! Only the state word is shared while the worker owns the payload.

use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU32, Ordering},
};

const FREE: u32 = 0; // Microkit zero-initializes the mapping.
const WRITING: u32 = 1;
const QUEUED: u32 = 2;
const RUNNING: u32 = 3;
const FINISHED: u32 = 4;
const CANCELLED: u32 = 5;

#[derive(Debug, PartialEq)]
pub enum Completion<T> {
    Waiting,
    Failed,
    Ready(T),
}

#[repr(C, align(64))]
pub struct Mailbox<const I: usize, const O: usize> {
    state: AtomicU32,
    input_len: UnsafeCell<usize>,
    output_len: UnsafeCell<usize>,
    input: UnsafeCell<[u8; I]>,
    output: UnsafeCell<[u8; O]>,
    // Only the diagnostic wallet and worker use these fields. The worker
    // publishes them before FINISHED and the wallet reads them inside take.
    #[cfg(feature = "wallet-range-profile")]
    times: [core::sync::atomic::AtomicU64; 3],
}

// Every access to payload/lengths requires exclusive ownership of the slot,
// transferred by the state word. Cancellation of a running job only touches state.
unsafe impl<const I: usize, const O: usize> Sync for Mailbox<I, O> {}

fn wipe(buf: &mut [u8]) {
    for b in buf {
        unsafe { core::ptr::write_volatile(b, 0) }
    }
}

impl<const I: usize, const O: usize> Mailbox<I, O> {
    #[cfg(test)]
    pub const fn empty() -> Self {
        Self {
            state: AtomicU32::new(FREE),
            input_len: UnsafeCell::new(0),
            output_len: UnsafeCell::new(0),
            input: UnsafeCell::new([0; I]),
            output: UnsafeCell::new([0; O]),
            #[cfg(feature = "wallet-range-profile")]
            times: [const { core::sync::atomic::AtomicU64::new(0) }; 3],
        }
    }

    #[cfg(feature = "wallet-range-profile")]
    pub fn set_worker_times(&self, us: [u64; 3]) {
        for (field, value) in self.times.iter().zip(us) {
            field.store(value, Ordering::Relaxed);
        }
    }

    #[cfg(feature = "wallet-range-profile")]
    pub fn worker_times(&self) -> [u64; 3] {
        core::array::from_fn(|i| self.times[i].load(Ordering::Relaxed))
    }

    fn transition(&self, from: u32, to: u32) -> bool {
        self.state
            .compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn finished(&self) -> bool {
        self.state.load(Ordering::Acquire) == FINISHED
    }

    pub fn running(&self) -> bool {
        self.state.load(Ordering::Acquire) == RUNNING
    }

    pub fn available(&self) -> bool {
        self.state.load(Ordering::Acquire) == FREE
    }

    pub fn submit(&self, encode: impl FnOnce(&mut [u8]) -> Option<usize>) -> bool {
        if !self.transition(FREE, WRITING) {
            return false;
        }
        let buf = unsafe { &mut *self.input.get() };
        if let Some(len) = encode(buf).filter(|n| (1..=I).contains(n)) {
            unsafe {
                *self.input_len.get() = len;
            }
            self.state.store(QUEUED, Ordering::Release);
            true
        } else {
            wipe(buf);
            self.state.store(FREE, Ordering::Release);
            false
        }
    }

    pub fn execute(&self, scan: impl FnOnce(&[u8], &mut [u8]) -> Option<usize>) -> bool {
        if !self.transition(QUEUED, RUNNING) {
            return false;
        }
        let input = unsafe { &mut (&mut *self.input.get())[..*self.input_len.get()] };
        let output = unsafe { &mut *self.output.get() };
        let len = scan(input, output)
            .filter(|n| (1..=O).contains(n))
            .unwrap_or(0);
        wipe(input);
        if len == 0 {
            wipe(output);
        }
        unsafe {
            *self.output_len.get() = len;
        }
        if !self.transition(RUNNING, FINISHED) {
            // Wallet has abandoned this generation while we were scanning.
            wipe(&mut output[..len]);
            self.state.store(FREE, Ordering::Release);
        }
        true
    }

    pub fn take<T>(&self, decode: impl FnOnce(&[u8]) -> T) -> Completion<T> {
        if !self.transition(FINISHED, WRITING) {
            return Completion::Waiting;
        }
        let result = unsafe { &mut (&mut *self.output.get())[..*self.output_len.get()] };
        let value = if result.is_empty() {
            Completion::Failed
        } else {
            Completion::Ready(decode(result))
        };
        wipe(result);
        self.state.store(FREE, Ordering::Release);
        value
    }

    // The wallet is the sole producer and calls this only for its outstanding ticket.
    pub fn cancel(&self) {
        loop {
            match self.state.load(Ordering::Acquire) {
                QUEUED if self.transition(QUEUED, WRITING) => {
                    unsafe {
                        wipe(&mut (&mut *self.input.get())[..*self.input_len.get()]);
                    }
                    self.state.store(FREE, Ordering::Release);
                    return;
                }
                RUNNING if self.transition(RUNNING, CANCELLED) => return,
                FINISHED => {
                    self.take(|_| ());
                    return;
                }
                QUEUED | RUNNING => continue,
                _ => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    #[test]
    fn cancels_queued_and_finished_jobs_before_reuse() {
        let slot = Mailbox::<16, 16>::empty();
        for finish in [false, true] {
            assert!(slot.submit(|dst| {
                dst[..3].copy_from_slice(b"key");
                Some(3)
            }));
            if finish {
                assert!(slot.execute(|src, dst| {
                    assert_eq!(src, b"key");
                    dst[..2].copy_from_slice(b"ok");
                    Some(2)
                }));
            }
            slot.cancel();
            assert!(slot.available());
            assert_eq!(slot.take(|_| 1), Completion::Waiting);
            unsafe {
                assert_eq!(*slot.input.get(), [0; 16]);
                assert_eq!(*slot.output.get(), [0; 16]);
            }
        }
    }

    #[test]
    fn a_cancelled_running_job_cannot_complete_into_a_new_scan() {
        let slot = Arc::new(Mailbox::<16, 16>::empty());
        let barrier = Arc::new(Barrier::new(2));
        assert!(slot.submit(|dst| {
            dst[0] = 42;
            Some(1)
        }));
        thread::scope(|scope| {
            let worker = slot.clone();
            let block = barrier.clone();
            scope.spawn(move || {
                worker.execute(|src, dst| {
                    assert_eq!(src, &[42]);
                    block.wait();
                    block.wait();
                    dst[0] = 98;
                    Some(1)
                })
            });
            barrier.wait();
            slot.cancel();
            assert!(!slot.submit(|_| Some(1)));
            barrier.wait();
        });
        assert!(slot.available());
        assert!(slot.submit(|dst| {
            dst[0] = 3;
            Some(1)
        }));
        assert!(slot.execute(|src, dst| {
            dst[0] = src[0] + 1;
            Some(1)
        }));
        assert_eq!(slot.take(|r| r[0]), Completion::Ready(4));
        assert!(slot.available());
    }

    #[test]
    fn partial_or_oversized_messages_never_become_successful_results() {
        let slot = Mailbox::<8, 8>::empty();
        assert!(!slot.submit(|dst| {
            dst.fill(13);
            None
        }));
        assert!(slot.available());
        unsafe {
            assert_eq!(*slot.input.get(), [0; 8]);
        }
        assert!(slot.submit(|_| Some(1)));
        assert!(slot.execute(|_, dst| {
            dst.fill(17);
            Some(9)
        }));
        assert_eq!(slot.take(|_| ()), Completion::Failed);
        unsafe {
            assert_eq!(*slot.output.get(), [0; 8]);
        }
    }
}
