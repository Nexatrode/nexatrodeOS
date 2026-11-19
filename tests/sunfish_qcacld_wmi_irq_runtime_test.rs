#![allow(dead_code)]

#[path = "../rust/src/sunfish_qcacld_wmi_irq_runtime.rs"]
mod sunfish_qcacld_wmi_irq_runtime;

use std::cell::RefCell;
use sunfish_qcacld_wmi_irq_runtime::{
    DrainProgress, NotificationOutcome, RuntimeError, WmiCe2IrqRuntime,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServiceError {
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AckError {
    Failed,
}

#[test]
fn budget_exhaustion_reschedules_without_ack_then_final_slice_acks_once() {
    let mut runtime = WmiCe2IrqRuntime::new();
    let calls = RefCell::new(Vec::new());

    let first = runtime
        .notified(
            true,
            false,
            || {
                calls.borrow_mut().push("service");
                Ok::<_, ServiceError>(DrainProgress::Deferred)
            },
            || calls.borrow_mut().push("continuation"),
            || {
                calls.borrow_mut().push("gic_ack");
                Ok::<_, AckError>(())
            },
        )
        .unwrap();

    assert_eq!(first, NotificationOutcome::Deferred);
    assert!(runtime.irq_pending());
    assert_eq!(*calls.borrow(), ["service", "continuation"]);

    calls.borrow_mut().clear();
    let second = runtime
        .notified(
            false,
            true,
            || {
                calls.borrow_mut().push("service");
                Ok::<_, ServiceError>(DrainProgress::Complete)
            },
            || calls.borrow_mut().push("continuation"),
            || {
                calls.borrow_mut().push("gic_ack");
                Ok::<_, AckError>(())
            },
        )
        .unwrap();

    assert_eq!(second, NotificationOutcome::Complete);
    assert!(!runtime.irq_pending());
    assert_eq!(*calls.borrow(), ["service", "gic_ack"]);
}

#[test]
fn simultaneous_irq_and_continuation_run_one_slice() {
    let mut runtime = WmiCe2IrqRuntime::new();
    let mut service_calls = 0;
    let mut ack_calls = 0;
    let outcome = runtime
        .notified(
            true,
            true,
            || {
                service_calls += 1;
                Ok::<_, ServiceError>(DrainProgress::Complete)
            },
            || panic!("complete drain must not schedule continuation"),
            || {
                ack_calls += 1;
                Ok::<_, AckError>(())
            },
        )
        .unwrap();

    assert_eq!(outcome, NotificationOutcome::Complete);
    assert_eq!(service_calls, 1);
    assert_eq!(ack_calls, 1);
    assert!(!runtime.irq_pending());
}

#[test]
fn stale_continuation_after_ack_is_inert() {
    let mut runtime = WmiCe2IrqRuntime::new();
    let mut called = false;
    let outcome = runtime
        .notified(
            false,
            true,
            || {
                called = true;
                Ok::<_, ServiceError>(DrainProgress::Complete)
            },
            || panic!("stale continuation must not reschedule"),
            || -> Result<(), AckError> { panic!("stale continuation must not ACK") },
        )
        .unwrap();

    assert_eq!(outcome, NotificationOutcome::StaleContinuation);
    assert!(!called);
    assert!(!runtime.irq_pending());
}

#[test]
fn service_failure_keeps_irq_pending_and_does_not_ack() {
    let mut runtime = WmiCe2IrqRuntime::new();
    let result = runtime.notified(
        true,
        false,
        || Err::<DrainProgress, _>(ServiceError::Failed),
        || panic!("failed service must not reschedule"),
        || -> Result<(), AckError> { panic!("failed service must not ACK") },
    );

    assert_eq!(
        result,
        Err(RuntimeError::<ServiceError, AckError>::Service(
            ServiceError::Failed
        ))
    );
    assert!(runtime.irq_pending());
}

#[test]
fn failed_gic_ack_keeps_pending_state_for_fail_closed_recovery() {
    let mut runtime = WmiCe2IrqRuntime::new();
    let result = runtime.notified(
        true,
        false,
        || Ok::<_, ServiceError>(DrainProgress::Complete),
        || panic!("complete drain must not reschedule"),
        || Err::<(), _>(AckError::Failed),
    );

    assert_eq!(
        result,
        Err(RuntimeError::InterruptAcknowledge(AckError::Failed))
    );
    assert!(runtime.irq_pending());
}

#[test]
fn empty_notification_set_is_rejected_without_work() {
    let mut runtime = WmiCe2IrqRuntime::new();
    let mut called = false;
    let result = runtime.notified(
        false,
        false,
        || {
            called = true;
            Ok::<_, ServiceError>(DrainProgress::Complete)
        },
        || panic!("empty notification must not schedule"),
        || -> Result<(), AckError> { panic!("empty notification must not ACK") },
    );

    assert_eq!(
        result,
        Err(RuntimeError::<ServiceError, AckError>::UnexpectedNotification)
    );
    assert!(!called);
    assert!(!runtime.irq_pending());
}

#[test]
fn native_report_flags_must_agree_before_irq_ack_policy_is_selected() {
    assert_eq!(
        DrainProgress::from_report_flags(true, false),
        Some(DrainProgress::Complete)
    );
    assert_eq!(
        DrainProgress::from_report_flags(false, true),
        Some(DrainProgress::Deferred)
    );
    assert_eq!(DrainProgress::from_report_flags(false, false), None);
    assert_eq!(DrainProgress::from_report_flags(true, true), None);
}
