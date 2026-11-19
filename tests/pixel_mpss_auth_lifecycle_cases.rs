use super::*;
use crate::sunfish_mpss_linux_loader::tests::boot_handoff::with_auth_boundary;
use crate::sunfish_mpss_linux_smp2p::Lifecycle as L;
use native::{AuthGate, Gate as _};
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn setup() -> (std::sync::MutexGuard<'static, ()>, Gate) {
    let lock = SERIAL.lock().unwrap();
    crate::BAD_ROOT_MAP.store(false, Ordering::Release);
    COLD.store(1, Ordering::Release);
    (
        lock,
        Gate {
            maps: mappings().unwrap(),
            failed: false,
        },
    )
}
#[test]
fn root_auth_actual_request_consumes_cold_before_monitor_and_real_receipts_advance_once() {
    let (_lock, gate) = setup();
    let gate = std::cell::RefCell::new(gate);
    with_auth_boundary(
        |r| {
            assert!(gate.borrow_mut().observe().unwrap().modem_stopped);
            gate.borrow_mut().submit_mpss_auth(1, r).unwrap();
            assert!(!gate.borrow_mut().observe().unwrap().modem_stopped);
            assert!(!preboot_generation(1));
            assert_eq!(lifecycle(1), Some(L::AuthSubmitted));
        },
        |a| {
            auth_returned(&a).unwrap();
            assert_eq!(lifecycle(1), Some(L::AuthReturned));
            crate::with_link_memory(|c, p| {
                crate::GLINK_LIFE.set((1, L::AuthReturned, crate::smp_link::Power::Active));
                p.peer_smp(false);
                p.word(
                    0x11d000 + crate::sunfish_mpss_linux_smem::FINAL_FREE + 16 + 36,
                    2,
                );
                c.poll().unwrap();
                let ready = c.take_ready_observation().unwrap();
                running(&a, &ready).unwrap();
                assert_eq!(lifecycle(1), Some(L::Running));
                assert!(!gate.borrow_mut().observe().unwrap().initial_hlos);
                assert!(running(&a, &ready).is_err());
                assert_eq!(lifecycle(1), None);
            });
        },
    );
}
#[test]
fn root_auth_duplicate_wrong_generation_or_terminal_gate_never_reopens() {
    let (_lock, mut gate) = setup();
    with_auth_boundary(
        |r| {
            assert!(gate.submit_mpss_auth(2, r).is_err());
            assert!(gate.submit_mpss_auth(1, r).is_err());
        },
        |_| {},
    );
    assert_eq!(COLD.load(Ordering::Acquire), 2);
    COLD.store(1, Ordering::Release); // adversarial restoration cannot clear Gate's latch
    assert!(gate.observe().is_err());
    COLD.store(1, Ordering::Release);
    let mut gate = Gate {
        maps: mappings().unwrap(),
        failed: false,
    };
    with_auth_boundary(
        |r| {
            gate.submit_mpss_auth(1, r).unwrap();
            assert!(gate.submit_mpss_auth(1, r).is_err());
            assert_eq!(lifecycle(1), None);
        },
        |a| assert!(auth_returned(&a).is_err()),
    );
}
#[test]
fn root_auth_mapping_change_and_external_abort_are_terminal() {
    let (_lock, mut gate) = setup();
    crate::BAD_ROOT_MAP.store(true, Ordering::Release);
    assert!(gate.observe().is_err());
    crate::BAD_ROOT_MAP.store(false, Ordering::Release);
    assert!(!retained_generation(1));
    assert!(gate.observe().is_err());
    COLD.store(1, Ordering::Release);
    let mut gate = Gate {
        maps: mappings().unwrap(),
        failed: false,
    };
    with_auth_boundary(
        |r| {
            gate.submit_mpss_auth(1, r).unwrap();
            abort();
        },
        |a| {
            assert!(auth_returned(&a).is_err());
            assert_eq!(lifecycle(1), None);
        },
    );
}
#[test]
fn root_auth_receipt_before_submission_cannot_grant_runtime() {
    let (_lock, _) = setup();
    with_auth_boundary(
        |_| {},
        |a| {
            assert!(auth_returned(&a).is_err());
            assert_eq!(lifecycle(1), None);
        },
    );
}
#[test]
fn root_auth_factory_rejects_missing_or_redelegated_owners_before_extraction() {
    let (_lock, _) = setup();
    for phase in [Phase::New, Phase::Prepared, Phase::Delegated] {
        let mut f = Factory {
            storage: None,
            tftp: None,
            resources: None,
            server: None,
            session: None,
            unattached: None,
            phase,
            error: 0,
        };
        assert!(f.take_boot_parts().is_err());
        assert_eq!(f.phase(), Phase::Failed);
        assert_eq!(f.error(), 0x30);
        assert!(f.take_boot_parts().is_err());
    }
}
#[test]
fn root_auth_shared_badges_preserve_ipa_and_reject_watchdog_faults_and_unknown_bits() {
    use crate::pixel_mpss_auth_irq as irq;
    let all = (1 << irq::SMP_CHANNEL) | (1 << irq::WDOG_CHANNEL) | 12;
    let (own, rest) = irq::split(all);
    assert_eq!(own, 96);
    assert_eq!(rest, 12);
    assert!(!irq::monitor_healthy(rest, own));
    assert!(irq::monitor_healthy(12, 32));
    for b in [1, 2, 16, 128, 1u64 << 63] {
        assert!(!irq::monitor_healthy(b, 0));
    }
    assert!(!irq::ack_sample(0, crate::smp_link::Poll::Observed));
    assert!(!irq::ack_sample(32, crate::smp_link::Poll::Waiting));
    assert!(!irq::ack_sample(32, crate::smp_link::Poll::Unstable));
    assert!(irq::ack_sample(32, crate::smp_link::Poll::Observed));
    assert!(!irq::ack_sample(32, crate::smp_link::Poll::VersionChanged));
}
#[test]
fn root_auth_ready_requires_current_irq_sample_not_a_stale_ready_history() {
    crate::with_link_memory(|c, p| {
        crate::GLINK_LIFE.set((1, L::AuthReturned, crate::smp_link::Power::Active));
        p.peer_smp(false);
        let at = 0x11d000 + crate::sunfish_mpss_linux_smem::FINAL_FREE + 16 + 36;
        p.word(at, 2);
        c.poll().unwrap();
        assert!(crate::pixel_mpss_auth_irq::ready_sample(&c.report()));
        let mut old = c.report();
        old.peer.as_mut().unwrap().slave_kernel = Some(0);
        assert!(old.ready_seen);
        assert!(!crate::pixel_mpss_auth_irq::ready_sample(&old));
        p.word(at, 0);
        assert_eq!(c.poll(), Err(crate::smp_link::Error::PeerRestart));
    });
}

#[derive(Debug, Eq, PartialEq)]
struct HandoffReceipt(u64);
impl crate::sunfish_mpss_live_domain::ReceiptGeneration for HandoffReceipt {
    fn generation(&self) -> u64 {
        self.0
    }
}

#[test]
fn root_auth_runtime_handoff_requires_ready_receipts_and_is_one_use() {
    use crate::sunfish_mpss_live_domain::{HandoffError, ReceiptHandoff};

    let mut before_ready = ReceiptHandoff::new(1, Some(HandoffReceipt(1)), Some(HandoffReceipt(1)));
    assert_eq!(before_ready.take(false), Err(HandoffError::NotReady));
    assert!(before_ready.is_quarantined());
    assert_eq!(before_ready.retained(), (true, true));

    let mut missing_auth =
        ReceiptHandoff::<HandoffReceipt, HandoffReceipt>::new(1, None, Some(HandoffReceipt(1)));
    assert_eq!(missing_auth.take(true), Err(HandoffError::MissingReceipt));
    assert!(missing_auth.is_quarantined());
    assert_eq!(missing_auth.retained(), (false, true));

    let mut missing_ready =
        ReceiptHandoff::<HandoffReceipt, HandoffReceipt>::new(1, Some(HandoffReceipt(1)), None);
    assert_eq!(missing_ready.take(true), Err(HandoffError::MissingReceipt));
    assert!(missing_ready.is_quarantined());
    assert_eq!(missing_ready.retained(), (true, false));

    let mut wrong_generation =
        ReceiptHandoff::new(1, Some(HandoffReceipt(2)), Some(HandoffReceipt(1)));
    assert_eq!(
        wrong_generation.take(true),
        Err(HandoffError::WrongGeneration)
    );
    assert!(wrong_generation.is_quarantined());
    assert_eq!(wrong_generation.retained(), (true, true));

    let mut valid = ReceiptHandoff::new(1, Some(HandoffReceipt(1)), Some(HandoffReceipt(1)));
    assert_eq!(valid.take(true), Ok((HandoffReceipt(1), HandoffReceipt(1))));
    assert_eq!(valid.take(true), Err(HandoffError::Consumed));
    assert!(valid.is_quarantined());
}

#[test]
fn root_auth_shared_native_gate_scopes_access_and_quarantines_conflicts() {
    use crate::sunfish_mpss_live_domain::{GateError, NativeGate};
    use std::cell::Cell;
    use std::rc::Rc;

    let identity = Rc::new(Cell::new(0));
    let gate = NativeGate::new((7u32, Rc::clone(&identity)));
    assert_eq!(
        gate.try_borrow_mut(|(value, id)| {
            *value += 1;
            id.set(id.get() + 1);
            *value
        }),
        Ok(8)
    );
    assert_eq!(
        gate.try_borrow_mut(|(value, id)| {
            *value += 1;
            id.set(id.get() + 1);
            (*value, id.get())
        }),
        Ok((9, 2))
    );
    assert_eq!(identity.get(), 2);

    let nested = NativeGate::new(0u32);
    let outer = nested.try_borrow_mut(|value| {
        *value += 1;
        nested.try_borrow_mut(|inner| *inner += 1)
    });
    assert_eq!(outer, Err(GateError::Quarantined));
    assert!(nested.is_quarantined());
    assert_eq!(
        nested.try_borrow_mut(|value| *value),
        Err(GateError::Quarantined)
    );

    let failed = NativeGate::new(1u32);
    assert_eq!(
        failed.try_borrow_mut(|value| {
            if *value != 1 {
                failed.quarantine();
            }
            *value
        }),
        Ok(1)
    );
    failed.quarantine();
    assert!(failed.try_borrow_mut(|_| ()).is_err());
}

#[test]
fn root_auth_runtime_handoff_consumes_actual_auth_and_ready_receipts_once() {
    use crate::sunfish_mpss_linux_loader::AuthReturned;
    use crate::sunfish_mpss_linux_smp2p::ReadyObservation;
    use crate::sunfish_mpss_live_domain::ReceiptHandoff;

    with_auth_boundary(
        |_| {},
        |auth| {
            let mut ready = None;
            crate::with_link_memory(|controller, peer| {
                crate::GLINK_LIFE.set((
                    1,
                    crate::sunfish_mpss_linux_smp2p::Lifecycle::AuthReturned,
                    crate::smp_link::Power::Active,
                ));
                peer.peer_smp(false);
                let at = 0x11d000 + crate::sunfish_mpss_linux_smem::FINAL_FREE + 16 + 36;
                peer.word(at, 2);
                controller.poll().unwrap();
                ready = Some(controller.take_ready_observation().unwrap());
            });
            let mut handoff = ReceiptHandoff::new(1, Some(auth), ready);
            let (auth, ready): (AuthReturned, ReadyObservation) = handoff.take(true).unwrap();
            assert_eq!(auth.generation(), 1);
            assert_eq!(ready.generation(), 1);
            assert!(matches!(
                handoff.take(true),
                Err(crate::sunfish_mpss_live_domain::HandoffError::Consumed)
            ));
        },
    );
}
