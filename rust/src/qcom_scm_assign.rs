//! Bounded selected-Linux SCM64 interrupted/busy mechanics, shared by the
//! closed assignment and PAS callers. Callers enforce their operation and
//! ownership policy; these retry mechanics grant no SMC authority.
#![forbid(unsafe_code)]

use crate::qcom_modem_handlers::Fault;

pub const SMC_INTERRUPTED: u64 = 1;
pub const SMC_BUSY: u64 = (-12i64) as u64;
pub const BUSY_WAIT_MS: u32 = 30;
/// The matched Linux driver allows 21 busy retries (22 total attempts).
pub const MAX_BUSY_RETRIES: u8 = 21;
/// Linux resumes interrupted calls without a counter; cap them for this owner.
pub const MAX_INTERRUPTED_RETRIES: u8 = 32;
pub const MAX_TOTAL_CALLS: u8 = 64;

pub trait Transport {
    fn invoke(&mut self, words: [u64; 8]) -> Result<[u64; 8], Fault>;
}

pub trait Delay {
    fn wait_ms(&mut self, milliseconds: u32) -> Result<(), Fault>;
}

pub trait Lifecycle {
    /// Called immediately before every low-level secure-monitor invocation,
    /// including each bounded retry.
    fn revalidate(&mut self) -> Result<(), Fault>;
}

/// Resume interrupted calls with their returned command/`a6`, and retry the
/// matched busy status after 30 ms. Busy handling follows Linux's 21-retry
/// bound; unlike Linux, interrupted calls and total invocations also stop at
/// fixed limits. Exhaustion returns the final nonzero status for the caller to
/// quarantine as an ambiguous ownership transition.
pub fn invoke(
    transport: &mut impl Transport,
    delay: &mut impl Delay,
    original: [u64; 8],
) -> Result<[u64; 8], Fault> {
    struct AlwaysLive;
    impl Lifecycle for AlwaysLive {
        fn revalidate(&mut self) -> Result<(), Fault> {
            Ok(())
        }
    }
    invoke_checked(transport, delay, &mut AlwaysLive, original)
}

pub fn invoke_checked(
    transport: &mut impl Transport,
    delay: &mut impl Delay,
    lifecycle: &mut impl Lifecycle,
    original: [u64; 8],
) -> Result<[u64; 8], Fault> {
    let mut words = original;
    let mut calls = 0u8;
    let mut busy_retries = 0u8;
    let mut interrupted_retries = 0u8;

    loop {
        lifecycle.revalidate()?;
        let reply = transport.invoke(words)?;
        calls = calls.saturating_add(1);
        if reply[0] == 0 {
            return Ok(reply);
        }
        if reply[0] == SMC_INTERRUPTED {
            if interrupted_retries >= MAX_INTERRUPTED_RETRIES || calls >= MAX_TOTAL_CALLS {
                return Ok(reply);
            }
            interrupted_retries += 1;
            words[0] = SMC_INTERRUPTED;
            words[6] = reply[6];
            continue;
        }
        if reply[0] == SMC_BUSY {
            if busy_retries >= MAX_BUSY_RETRIES || calls >= MAX_TOTAL_CALLS {
                return Ok(reply);
            }
            delay.wait_ms(BUSY_WAIT_MS)?;
            busy_retries += 1;
            interrupted_retries = 0;
            words = original;
            continue;
        }
        return Ok(reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc, vec::Vec};

    #[derive(Clone)]
    struct Fake(Rc<RefCell<State>>);

    struct State {
        replies: Vec<[u64; 8]>,
        calls: Vec<[u64; 8]>,
        waits: Vec<u32>,
        fail_transport: bool,
        fail_delay: bool,
    }

    impl Fake {
        fn new(replies: &[[u64; 8]]) -> Self {
            Self(Rc::new(RefCell::new(State {
                replies: replies.to_vec(),
                calls: Vec::new(),
                waits: Vec::new(),
                fail_transport: false,
                fail_delay: false,
            })))
        }
    }

    impl Transport for Fake {
        fn invoke(&mut self, words: [u64; 8]) -> Result<[u64; 8], Fault> {
            let mut state = self.0.borrow_mut();
            state.calls.push(words);
            if state.fail_transport || state.replies.is_empty() {
                return Err(Fault::Uncertain);
            }
            Ok(state.replies.remove(0))
        }
    }

    impl Delay for Fake {
        fn wait_ms(&mut self, milliseconds: u32) -> Result<(), Fault> {
            let mut state = self.0.borrow_mut();
            if state.fail_delay {
                return Err(Fault::Uncertain);
            }
            state.waits.push(milliseconds);
            Ok(())
        }
    }

    struct Guard {
        checks: u8,
        fail_at: Option<u8>,
    }
    impl Lifecycle for Guard {
        fn revalidate(&mut self) -> Result<(), Fault> {
            self.checks += 1;
            if self.fail_at == Some(self.checks) {
                Err(Fault::Uncertain)
            } else {
                Ok(())
            }
        }
    }

    fn original() -> [u64; 8] {
        [
            0x42000c16, 0x1117, 0xa2a01000, 16, 0xa2a01040, 0xa2a01100, 0, 0,
        ]
    }

    #[test]
    fn resumes_interrupted_call_with_returned_command_and_a6() {
        let fake = Fake::new(&[[1, 0, 0, 0, 0, 0, 0x55, 0], [0; 8]]);
        let reply = invoke(&mut fake.clone(), &mut fake.clone(), original()).unwrap();
        assert_eq!(reply[0], 0);
        let state = fake.0.borrow();
        assert_eq!(
            state.calls,
            [
                original(),
                [1, 0x1117, 0xa2a01000, 16, 0xa2a01040, 0xa2a01100, 0x55, 0]
            ]
        );
        assert!(state.waits.is_empty());
    }

    #[test]
    fn retries_busy_with_matched_30ms_delay_and_original_call_words() {
        let fake = Fake::new(&[[SMC_BUSY, 0, 0, 0, 0, 0, 0, 0], [0; 8]]);
        let mut transport = fake.clone();
        let mut delay = fake.clone();
        assert_eq!(
            invoke(&mut transport, &mut delay, original()).unwrap()[0],
            0
        );
        let state = fake.0.borrow();
        assert_eq!(state.calls, [original(), original()]);
        assert_eq!(state.waits, [30]);
    }

    #[test]
    fn lifecycle_is_revalidated_before_every_low_level_retry() {
        let fake = Fake::new(&[[SMC_BUSY, 0, 0, 0, 0, 0, 0, 0], [0; 8]]);
        let mut transport = fake.clone();
        let mut delay = fake.clone();
        let mut guard = Guard {
            checks: 0,
            fail_at: None,
        };
        assert_eq!(
            invoke_checked(&mut transport, &mut delay, &mut guard, original()).unwrap()[0],
            0
        );
        assert_eq!(guard.checks, 2);

        let stopped = Fake::new(&[[SMC_BUSY, 0, 0, 0, 0, 0, 0, 0], [0; 8]]);
        let mut transport = stopped.clone();
        let mut delay = stopped.clone();
        let mut guard = Guard {
            checks: 0,
            fail_at: Some(2),
        };
        assert_eq!(
            invoke_checked(&mut transport, &mut delay, &mut guard, original()),
            Err(Fault::Uncertain)
        );
        assert_eq!(guard.checks, 2);
        assert_eq!(stopped.0.borrow().calls.len(), 1);
    }

    #[test]
    fn busy_retry_budget_matches_linux_maximum_and_returns_terminal_status() {
        let mut replies = [[SMC_BUSY, 0, 0, 0, 0, 0, 0, 0]; 22];
        replies[21][1] = 7;
        let fake = Fake::new(&replies);
        let mut transport = fake.clone();
        let mut delay = fake.clone();
        assert_eq!(
            invoke(&mut transport, &mut delay, original()).unwrap(),
            replies[21]
        );
        let state = fake.0.borrow();
        assert_eq!(state.calls.len(), 22);
        assert_eq!(state.waits, [30; 21]);
    }

    #[test]
    fn interrupted_retry_budget_is_bounded() {
        let replies = [[SMC_INTERRUPTED, 0, 0, 0, 0, 0, 0, 0]; 33];
        let fake = Fake::new(&replies);
        let mut transport = fake.clone();
        let mut delay = fake.clone();
        assert_eq!(
            invoke(&mut transport, &mut delay, original()).unwrap()[0],
            SMC_INTERRUPTED
        );
        assert_eq!(fake.0.borrow().calls.len(), 33);
    }

    #[test]
    fn transport_and_delay_failures_stop_without_another_monitor_call() {
        let fake = Fake::new(&[[SMC_BUSY, 0, 0, 0, 0, 0, 0, 0], [0; 8]]);
        fake.0.borrow_mut().fail_delay = true;
        let mut transport = fake.clone();
        let mut delay = fake.clone();
        assert_eq!(
            invoke(&mut transport, &mut delay, original()),
            Err(Fault::Uncertain)
        );
        assert_eq!(fake.0.borrow().calls.len(), 1);

        let failed = Fake::new(&[]);
        failed.0.borrow_mut().fail_transport = true;
        let mut transport = failed.clone();
        let mut delay = failed.clone();
        assert_eq!(
            invoke(&mut transport, &mut delay, original()),
            Err(Fault::Uncertain)
        );
        assert_eq!(failed.0.borrow().calls.len(), 1);
    }
}
