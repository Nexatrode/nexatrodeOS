//! Exact router plus actual serialized Owner, with a CPU FIFO/kernel model.
include!("sunfish_modem_owner_test.rs");
extern crate self as personal_os_rust_server;
#[path = "../rust/src/pixel_mpss_runtime_irq.rs"]
mod pixel_mpss_runtime_irq;
mod native_irq {
    use super::*;
    use pixel_mpss_runtime_irq::{Error, Kernel, Router};
    use std::cell::{Cell, RefCell};
    #[derive(Clone, Default)]
    struct K {
        calls: Rc<RefCell<Vec<u8>>>,
        fail: Rc<Cell<bool>>,
        panic: Rc<Cell<bool>>,
    }
    impl Kernel for K {
        fn acknowledge(&mut self, c: u8) -> Result<(), ()> {
            self.calls.borrow_mut().push(c);
            assert!(!self.panic.get(), "uncertain kernel ACK");
            if self.fail.get() { Err(()) } else { Ok(()) }
        }
    }
    fn actor() -> (Owner<Fake>, Fake) {
        let peer = Fake::new(0, 0);
        (
            Owner::attach(Adapter::attach(peer.clone(), session(), 1).unwrap(), 1).unwrap(),
            peer,
        )
    }
    #[cfg(all(feature = "sunfish-tftp-early-dispatch-experiment", feature = "sunfish-tftp-endpoint-observer"))]
    #[test]
    fn queued_tftp_reply_can_use_one_router_continuation_without_skipping_irq_checks() {
        use qcom_modem_owner::TftpServiceProgress;
        let (peer, mut owner) = owner();
        owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
        let mut intents = control(5, 2, 8);
        for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
        peer.push(&intents);
        let mut setup = Irqs::default(); owner.notified(8, &mut setup).unwrap(); peer.take_tx();
        owner_control_peer_hello(&peer, &mut owner, &mut setup);
        take_owner_datagrams(&peer, &mut owner, &mut setup);
        let slot = owner.offer_receive().unwrap(); owner.notified(8, &mut setup).unwrap(); peer.take_tx();
        let kernel = K::default();
        let mut router = Router::new(kernel.clone(), 1);
        peer.push(&data(slot, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x56, 0x5000)));
        router.capture(1 << 4).unwrap();
        let received = router.dispatch(&mut owner).unwrap();
        assert!(received.deferred && owner.pending_tftp_request().is_some());
        let mut server = qcom_tftp_server_check::ServerCheck::new();
        assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut router), Ok(TftpServiceProgress::ResponseQueued));
        assert!(router.report().continuation_pending);
        assert_eq!(owner.tftp_endpoint_trace().unwrap().flags & 4, 0);
        router.dispatch(&mut owner).unwrap();
        assert_ne!(owner.tftp_endpoint_trace().unwrap().flags & 4, 0, "the queued reply is published on the next bounded turn");
        assert!(router.report().error.is_none());
        for _ in 0..8 {
            if router.report().pending_modem == 0 { break; }
            router.dispatch(&mut owner).unwrap();
        }
        assert_eq!(&*kernel.calls.borrow(), &[4], "the original physical IRQ is rearmed exactly once");
    }
    #[test]
    fn native_irq_actual_owner_maps_physical_four_five_and_preserves_other_owners() {
        let (mut owner, _) = actor();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(0x3c).unwrap();
        assert!(router.dispatch(&mut owner).unwrap().deferred);
        assert!(k.calls.borrow().is_empty());
        assert_eq!(router.take_other(), 12);
        assert_eq!(router.take_other(), 0);
        let p = router.dispatch(&mut owner).unwrap();
        assert_eq!(p.acknowledged, 3);
        assert_eq!(&*k.calls.borrow(), &[5, 4]);
        assert_eq!(router.report().pending_modem, 0);
        assert!(router.report().continuation_pending);
        router.dispatch(&mut owner).unwrap();
        assert!(!router.report().continuation_pending);
        assert_eq!(&*k.calls.borrow(), &[5, 4]);
    }
    #[test]
    fn software_wakeup_continues_pending_work_without_acknowledging_an_irq() {
        assert_eq!(pixel_mpss_runtime_irq::CONTINUATION_SEND_CHANNEL, 7);
        let (mut owner, _) = actor();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.schedule().unwrap();
        router.capture(1 << 8).unwrap();
        router.dispatch(&mut owner).unwrap();
        assert!(!router.report().continuation_pending);
        assert!(k.calls.borrow().is_empty());
    }
    #[test]
    fn software_wakeup_can_coexist_with_a_real_glink_irq() {
        let (mut owner, _) = actor();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.schedule().unwrap();
        router.capture((1 << 8) | (1 << 4)).unwrap();
        router.dispatch(&mut owner).unwrap();
        assert_eq!(&*k.calls.borrow(), &[4]);
    }
    #[test]
    fn native_irq_physical_gsi_is_not_a_modem_continuation_or_ack() {
        let (mut owner, _) = actor();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(8).unwrap();
        assert!(!router.report().continuation_pending);
        assert_eq!(router.take_other(), 8);
        let p = router.dispatch(&mut owner).unwrap();
        assert_eq!(p.acknowledged, 0);
        assert!(!router.report().continuation_pending);
        assert!(k.calls.borrow().is_empty());
    }
    #[test]
    fn native_irq_watchdog_unknown_and_duplicate_are_terminal_without_ack() {
        for badge in [1, 2, 64, 128, 1u64 << 63] {
            let k = K::default();
            let mut router = Router::new(k.clone(), 1);
            assert!(router.capture(badge | 12).is_err());
            assert_eq!(router.take_other(), 12 | (badge & 3));
            let (mut owner, _) = actor();
            assert!(router.dispatch(&mut owner).is_err());
            assert!(owner.error().is_some());
            assert!(router.capture(16).is_err());
            assert!(router.schedule().is_err());
            assert!(k.calls.borrow().is_empty());
        }
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(16).unwrap();
        assert_eq!(router.capture(16), Err(Error::DuplicateNotification));
        assert!(k.calls.borrow().is_empty());
    }
    #[test]
    fn native_irq_no_ack_before_delivery_or_after_rearm_and_no_continue_ack() {
        for channel in [0, 1, 3, 4, 5, 6] {
            let k = K::default();
            let mut router = Router::new(k.clone(), 1);
            assert!(router.acknowledge(channel).is_err());
            assert!(k.calls.borrow().is_empty());
        }
        let (mut owner, _) = actor();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(16).unwrap();
        router.dispatch(&mut owner).unwrap();
        assert!(router.acknowledge(0).is_err());
        assert_eq!(&*k.calls.borrow(), &[4]);
    }
    #[test]
    fn native_irq_fifo_fault_prevents_kernel_ack_and_stops_router() {
        let (mut owner, peer) = actor();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(16).unwrap();
        peer.0.borrow_mut().observation.ready = false;
        assert!(router.dispatch(&mut owner).is_err());
        assert!(k.calls.borrow().is_empty());
        peer.0.borrow_mut().observation.ready = true;
        assert!(router.dispatch(&mut owner).is_err());
        assert!(router.schedule().is_err());
    }
    #[test]
    fn native_irq_uncertain_ack_and_unwind_are_not_retried() {
        for unwind in [false, true] {
            let (mut owner, _) = actor();
            let k = K::default();
            let mut router = Router::new(k.clone(), 1);
            router.capture(16).unwrap();
            k.panic.set(unwind);
            k.fail.set(!unwind);
            if unwind {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || router.dispatch(&mut owner)
                    ))
                    .is_err()
                );
            } else {
                assert!(router.dispatch(&mut owner).is_err());
            }
            assert_eq!(&*k.calls.borrow(), &[4]);
            assert_eq!(router.report().pending_modem, 16);
            k.fail.set(false);
            k.panic.set(false);
            assert!(router.acknowledge(0).is_err());
            assert!(router.dispatch(&mut owner).is_err());
            assert!(router.capture(16).is_err());
            assert_eq!(&*k.calls.borrow(), &[4]);
        }
    }
    #[test]
    fn native_irq_identity_change_cannot_take_an_existing_binding() {
        let (mut a, _) = actor();
        let peer = Fake::new(0, 0);
        peer.0.borrow_mut().observation.identity += 1;
        let mut b = Owner::attach(Adapter::attach(peer, session(), 1).unwrap(), 1).unwrap();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.dispatch(&mut a).unwrap();
        assert_eq!(router.dispatch(&mut b), Err(Error::Identity));
        assert!(k.calls.borrow().is_empty());
        let mut zero = Router::new(K::default(), 0);
        assert_eq!(zero.capture(0), Err(Error::Identity));
    }
    #[test]
    fn native_irq_new_edge_after_ack_is_distinct_and_rearmed_once() {
        let (mut owner, _) = actor();
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        for _ in 0..2 {
            router.capture(16).unwrap();
            router.dispatch(&mut owner).unwrap();
        }
        assert_eq!(&*k.calls.borrow(), &[4, 4]);
        assert_eq!(router.report().acknowledgements, 2);
    }
    #[test]
    fn native_irq_boot_transport_ack_is_glink_only_and_one_shot() {
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        assert_eq!(router.acknowledge_boot_glink(), Err(Error::UnexpectedAck));

        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(1 << 4).unwrap();
        router.acknowledge_boot_glink().unwrap();
        assert_eq!(&*k.calls.borrow(), &[4]);
        assert_eq!(router.report().pending_modem, 0);
        assert_eq!(router.acknowledge_boot_glink(), Err(Error::UnexpectedAck));
        assert_eq!(&*k.calls.borrow(), &[4]);
    }
    #[test]
    fn native_irq_uncertain_boot_glink_ack_is_not_retried() {
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(1 << 4).unwrap();
        k.fail.set(true);
        assert_eq!(router.acknowledge_boot_glink(), Err(Error::Ack));
        k.fail.set(false);
        assert_eq!(router.acknowledge_boot_glink(), Err(Error::Ack));
        assert_eq!(&*k.calls.borrow(), &[4]);
    }

    #[test]
    fn native_irq_explicit_quarantine_is_terminal_without_ack() {
        let k = K::default();
        let mut router = Router::new(k.clone(), 1);
        router.capture(1 << 4).unwrap();
        router.quarantine();
        assert!(router.capture(1 << 5).is_err());
        assert!(router.acknowledge_boot_glink().is_err());
        assert!(router.schedule().is_err());
        assert_eq!(router.report().pending_modem, 1 << 4);
        assert_eq!(&*k.calls.borrow(), &[]);
    }

    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    #[test]
    fn close_handoff_keeps_irq_masked_until_owner_decision_then_resumes() {
        exercise_close_handoff_irq(false);
    }

    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    #[test]
    fn rejected_close_handoff_never_acknowledges_irq() {
        exercise_close_handoff_irq(true);
    }

    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    fn exercise_close_handoff_irq(reject: bool) {
        use qcom_glink_qmi::TftpReplyRoute;
        use qcom_modem_owner::TftpServiceProgress;
        use qcom_qmi_router::Address;
        let (mut owner, peer) = actor();
        owner.prepare_tftp_server(0x5000).unwrap(); owner.start_control().unwrap();
        let mut intents = control(5, 2, 8);
        for id in 0u32..8 { intents.extend(4096u32.to_le_bytes()); intents.extend(id.to_le_bytes()); }
        peer.push(&intents);
        let mut irqs = Irqs::default(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        owner_control_peer_hello(&peer, &mut owner, &mut irqs);
        take_owner_datagrams(&peer, &mut owner, &mut irqs);
        let mut server = qcom_tftp_server_check::ServerCheck::new();
        let mut first_records = Vec::new();
        for (iteration, expected) in [TftpServiceProgress::ResponseQueued, TftpServiceProgress::WaitingReply].into_iter().enumerate() {
            let slot = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
            let mut packet = tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000);
            packet[16..20].copy_from_slice(&0u32.to_le_bytes());
            peer.push(&data(slot, &packet)); owner.notified(1, &mut irqs).unwrap();
            assert_eq!(owner.service_tftp_server_check_step(&mut server, &mut irqs), Ok(expected));
            if iteration == 0 { owner.notified(8, &mut irqs).unwrap(); first_records = peer.take_tx(); }
        }
        acknowledge_outgoing_glink(&peer, &mut owner, &mut irqs, &first_records); peer.take_tx();
        let credit = owner.offer_receive().unwrap(); owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let body: Vec<u8> = [7u32, 0, 0x57, 0, 0].iter().flat_map(|word| word.to_le_bytes()).collect();
        peer.push(&data(credit, &owner_qrtr_packet(7, 0x57, 0x5001, false, &body)));
        owner.notified(1, &mut irqs).unwrap(); peer.take_tx();
        let deferred = owner.offer_receive().unwrap(); let close = owner.offer_receive().unwrap();
        owner.notified(8, &mut irqs).unwrap(); peer.take_tx();
        let mut saturated = false;
        for n in 0..16 {
            let result = owner.queue_tftp_response(TftpReplyRoute {
                source: Address { node: 1, port: 0x5100 + n },
                destination: Address { node: 0, port: 0x80 + n },
            }, qcom_tftp_server_check::OACK, &mut irqs);
            if result == Err(OwnerError::Transport(qcom_glink_fifo::Error::Session(qcom_glink_qmi::Error::NoCapacity))) { saturated = true; break; }
            result.unwrap();
        }
        assert!(saturated);
        peer.push(&data(deferred, &tftp_qrtr(qcom_tftp_server_check::REQUEST, 0x57, 0x5000)));
        owner.notified(1, &mut irqs).unwrap();
        peer.push(&data(close, &owner_control_frame(6, [0, 0x57, 0, 0])));
        let kernel = K::default(); let mut router = Router::new(kernel.clone(), 1);
        // Prior setup used Irqs; transfer its already-scheduled continuation
        // into the router that owns subsequent dispatches.
        router.schedule().unwrap();
        router.capture(1 << 4).unwrap();
        assert!(router.dispatch(&mut owner).unwrap().deferred);
        assert_eq!(owner.pending_tftp_close(), Some(Address { node: 0, port: 0x57 }));
        assert!(kernel.calls.borrow().is_empty(), "IRQ remains masked while the close is held");
        assert!(router.report().continuation_pending);
        if reject {
            let mut out = [0; 128];
            let route = qcom_tftp_server_check::Route {
                source: qcom_tftp_server_check::Address { node: 0, port: 0x57 },
                destination: qcom_tftp_server_check::Address { node: 1, port: 0x5001 },
            };
            server.receive(route, b"\0\x03\0\x01x", &mut out).unwrap();
            assert!(owner.resolve_pending_tftp_close(&mut server).is_err());
            assert!(router.dispatch(&mut owner).is_err());
            assert!(kernel.calls.borrow().is_empty(), "rejected cleanup must not unmask the IRQ");
            return;
        }
        owner.resolve_pending_tftp_close(&mut server).unwrap();
        router.dispatch(&mut owner).unwrap();
        assert_eq!(&*kernel.calls.borrow(), &[4]);
    }
}
