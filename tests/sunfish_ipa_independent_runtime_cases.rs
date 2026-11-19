// Included in the owner harness only with the composed runtime profile.
use super::*;
use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

#[test]
fn runtime_independent_discovery_early_turns_and_receipts_preserve_event_identity() {
    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    for server_first in [true, false] {
        let mut r = LoRig::from_session_with_modem(0, false, lo_session(), false, false);
        r.owner.set_rmtfs_event_clock(clock);
        r.enqueue(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
        let request = lo_message(0, 0x20, 0x1234, &[(0x10, &[1])]);
        r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &request));
        r.owner.notified(8, &mut r.irqs).unwrap();
        r.now = clock();
        let discovered = r.owner.service_linux_ipa_step(r.now, &mut r.irqs).unwrap();
        assert_eq!(discovered, LP::Discovered);
        r.owner.continue_ipa_init_after_discovery(discovered, clock, &mut r.irqs).unwrap();
        let (init, transport) = r.owner.ipa_init_trace();
        assert_eq!(init.unwrap().extra_steps, 2);
        assert!(init.unwrap().prepared_at.is_some());
        assert!(transport.unwrap().queued_at.is_some());
        assert!(transport.unwrap().retired_at.is_none());
        assert!(r.owner.ipa_registration_trace().unwrap().prepared_at.is_none());

        r.now = clock();
        assert!(r.owner.service_ipa_registration_early(r.now, &mut r.irqs).unwrap());
        let prepared = r.owner.ipa_registration_trace().unwrap();
        assert!(prepared.prepared_at.is_some() && prepared.queued_at.is_none());
        let report = r.owner.linux_ipa_report().unwrap();
        assert!(report.client_pending_record && report.server_pending_record);
        r.now = clock();
        assert!(!r.owner.service_ipa_registration_early(r.now, &mut r.irqs).unwrap());
        assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs).unwrap(), LP::Queued);
        r.drive().unwrap();
        assert_eq!(r.sent.len(), 2);
        let init_wire = r.sent.iter().find(|(_, p)| p[32] == 0).unwrap().clone();
        let reg_wire = r.sent.iter().find(|(_, p)| p[32] == 2).unwrap().clone();
        r.sent.clear();
        assert_ne!(init_wire.0, reg_wire.0);
        assert_eq!(&reg_wire.1[32..37], &[2, 0x34, 0x12, 0x20, 0]);
        r.now = clock();
        r.init_response(0, 0, 0).unwrap();
        let response_at = r.owner.ipa_init_trace().0.unwrap().response_at.unwrap();
        assert!(r.owner.linux_ipa_report().unwrap().held_response);
        let reg_sent = r.owner.ipa_registration_trace().unwrap();
        assert!(reg_sent.prepared_at < reg_sent.queued_at && reg_sent.queued_at < reg_sent.published_at);
        assert!(reg_sent.completed_at.is_none());
        assert!(r.owner.ipa_init_trace().0.unwrap().committed_at.is_none());

        if server_first {
            r.ack(&reg_wire, true);
            assert!(r.owner.ipa_registration_trace().unwrap().completed_at.is_some());
            assert!(r.owner.ipa_init_trace().0.unwrap().committed_at.is_none(), "server commit cannot timestamp INIT");
            assert!(!r.owner.linux_ipa_report().unwrap().protocol.unwrap().modem_ready);
        }
        r.ack(&init_wire, true);
        let init_done = r.owner.ipa_init_trace().0.unwrap();
        assert!(init_done.committed_at.unwrap() > response_at);
        assert_eq!(init_done.response_at, Some(response_at));
        if !server_first { r.ack(&reg_wire, true); }
        assert_eq!(r.owner.ipa_init_trace().0.unwrap().committed_at, init_done.committed_at);
        let report = r.owner.linux_ipa_report().unwrap();
        assert_eq!((report.publications_committed, report.responses_accepted), (2, 1));
        assert!(!report.pending_record && !report.held_response);
        r.request(0x35, 0x5678);
        let driver = r.next(); r.ack(&driver, true);
        let indication = r.next(); r.ack(&indication, true);
        assert!(r.owner.linux_ipa_report().unwrap().protocol_complete);
    }
}

#[test]
fn runtime_independent_early_response_cannot_block_server_early_turn() {
    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let mut r = LoRig::new();
    r.owner.set_rmtfs_event_clock(|| CLOCK.fetch_add(1, Relaxed));
    let _init = r.next();
    r.init_response(0, 0, 0).unwrap();
    let request = lo_message(0, 0x20, 0x4321, &[(0x10, &[1])]);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &request));
    r.owner.notified(8, &mut r.irqs).unwrap();
    assert!(r.owner.ipa_registration_turn_times()[0].is_some());
    assert!(r.owner.service_ipa_registration_early(r.now, &mut r.irqs).unwrap());
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.held_response && report.client_pending_record && report.server_pending_record);
    assert_eq!(report.responses_accepted, 0);
    assert_eq!(report.publications_committed, 0);
    assert!(r.owner.ipa_registration_trace().unwrap().prepared_at.is_some());
}

#[test]
fn runtime_independent_alternates_admission_without_starving_initial_client() {
    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    let mut r = LoRig::from_session_with_modem(0, false, lo_session(), false, false);
    r.owner.set_rmtfs_event_clock(clock);
    r.enqueue(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
    let request = lo_message(0, 0x20, 0x4321, &[(0x10, &[1])]);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &request));
    r.owner.notified(8, &mut r.irqs).unwrap();
    assert_eq!(r.owner.service_linux_ipa_step(clock(), &mut r.irqs).unwrap(), LP::Discovered);
    assert!(r.owner.service_ipa_registration_early(clock(), &mut r.irqs).unwrap());
    assert!(r.owner.ipa_init_trace().0.unwrap().prepared_at.is_none(), "server prepare cannot timestamp INIT");
    assert!(!r.owner.service_ipa_registration_early(clock(), &mut r.irqs).unwrap());
    assert_eq!(r.owner.service_linux_ipa_step(clock(), &mut r.irqs).unwrap(), LP::Prepared);
    let init_prepared = r.owner.ipa_init_trace().0.unwrap().prepared_at;
    assert!(init_prepared.is_some());
    assert!(r.owner.service_ipa_registration_early(clock(), &mut r.irqs).unwrap());
    assert!(!r.owner.service_ipa_registration_early(clock(), &mut r.irqs).unwrap());
    assert_eq!(r.owner.service_linux_ipa_step(clock(), &mut r.irqs).unwrap(), LP::Queued);
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.client_record_queued && report.server_record_queued);
    assert_eq!(report.publications_committed, 0);
    assert_eq!(r.owner.ipa_init_trace().0.unwrap().prepared_at, init_prepared);
}

#[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
#[test]
fn cold_registration_handoff_admits_reply_once_with_either_arrival_order() {
    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    let announcement = owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]);
    let request = lo_message(0, 0x20, 0x1234, &[(0x10, &[1])]);
    let packet = lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &request);
    for endpoint_first in [true, false] {
        let mut r = LoRig::from_session_with_modem(0, false, lo_session(), false, false);
        r.owner.set_rmtfs_event_clock(clock);
        if endpoint_first { r.enqueue(&announcement); }
        r.enqueue(&packet);
        r.owner.notified(8, &mut r.irqs).unwrap();
        if !endpoint_first {
            assert!(!r.owner.service_cold_ipa_registration(clock, &mut r.irqs).unwrap());
            assert!(r.owner.ipa_registration_turn_times()[0].is_none());
            r.enqueue(&announcement);
            r.owner.notified(8, &mut r.irqs).unwrap();
        }
        assert!(r.owner.ipa_registration_turn_times()[0].is_some());
        assert!(r.owner.service_cold_ipa_registration(clock, &mut r.irqs).unwrap());
        assert!(!r.owner.service_cold_ipa_registration(clock, &mut r.irqs).unwrap());
        let registration = r.owner.ipa_registration_trace().unwrap();
        assert!(registration.routed_at < registration.prepared_at);
        assert!(registration.prepared_at < registration.first_queue_attempt_at);
        assert!(registration.first_queue_attempt_at < registration.queued_at);
        assert!(registration.published_at.is_none());
        assert!(r.owner.ipa_init_trace().1.is_none());
        assert_eq!(r.owner.service_linux_ipa_step(clock(), &mut r.irqs).unwrap(), LP::Prepared);
    }
}

#[cfg(all(feature = "sunfish-ipa-registration-cold-handoff",
          feature = "sunfish-wlan-first-server-check-early-turn"))]
#[test]
fn cold_registration_handoff_after_tftp_dispatch_queues_before_init() {
    use elf_blocker_irq::{Kernel, Router};
    use qcom_tftp_server_check::{REQUEST, SERVICE_PORT, ServerCheck};

    struct TestKernel;
    impl Kernel for TestKernel {
        fn acknowledge(&mut self, _: u8) -> Result<(), ()> { Ok(()) }
    }

    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    let mut session = lo_session();
    session.prepare_tftp_server(1, SERVICE_PORT).unwrap();
    session.enable_ipa_before_first_oack();
    #[cfg(all(feature = "sunfish-ipa-registration-rx-first", not(feature = "sunfish-ipa-reply-before-oack")))]
    session.observe_first_tftp_without_oack_hold();
    let mut r = LoRig::from_session_with_modem(0, false, session, false, false);
    r.owner.set_rmtfs_event_clock(clock);
    let mut router = Router::new(TestKernel, 1);
    let mut server = ServerCheck::new();
    let request = lo_message(0, 0x20, 0x1234, &[(0x10, &[1])]);
    if r.available.len() < 3 {
        r.owner.offer_receive().unwrap();
        r.drive().unwrap();
    }
    r.enqueue(&lo_packet(1, 0x1e, SERVICE_PORT, false, REQUEST));
    r.enqueue(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &request));
    router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
    router.dispatch(&mut r.owner).unwrap();

    // Mirror service_pass: early IPA check, TFTP's nested dispatch, then
    // a second handoff check before ordinary IPA discovery.
    assert!(!router.service_cold_ipa_registration(&mut r.owner, clock).unwrap());
    assert!(!r.owner.service_ipa_registration_early(clock(), &mut router).unwrap());
    assert_eq!(r.owner.service_tftp_server_check_step(&mut server, &mut router),
        Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
    assert!(router.dispatch_queued_server_check_oack(&mut r.owner, clock).unwrap());
    #[cfg(all(feature = "sunfish-ipa-registration-rx-first", not(feature = "sunfish-first-oack-owner-turn")))]
    {
        assert!(r.owner.first_oack_rx_passes() > 0);
        assert!(r.owner.server_check_oacks()[0].unwrap().oack_at.is_none());
    }
    #[cfg(all(feature = "sunfish-first-oack-owner-turn", not(feature = "sunfish-ipa-reply-before-oack")))]
    {
        let oack = r.owner.server_check_oacks()[0].unwrap();
        assert!(oack.wrq_done_at.unwrap() < oack.oack_at.unwrap());
    }
    assert!(r.owner.ipa_registration_turn_times()[0].is_some());

    assert!(router.service_cold_ipa_registration(&mut r.owner, clock).unwrap());
    assert!(!router.service_cold_ipa_registration(&mut r.owner, clock).unwrap());
    let registration = r.owner.ipa_registration_trace().unwrap();
    assert!(registration.queued_at.is_some());
    assert!(registration.first_queue_attempt_at < registration.queued_at);
    assert!(r.owner.ipa_init_trace().1.is_none());
    #[cfg(feature = "sunfish-ipa-reply-before-oack")]
    {
        let oack = r.owner.server_check_oacks()[0].unwrap();
        assert!(oack.wrq_done_at.unwrap() < registration.published_at.unwrap());
        assert!(registration.tx.request_done_at.unwrap() < registration.published_at.unwrap());
        assert!(registration.completed_at.is_none());
        assert!(oack.oack_at.is_none());
        r.drain();
        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
        {
            while r.available.len() < 2 {
                r.owner.offer_receive().unwrap();
                qcom_modem_owner::Interrupts::schedule(&mut router).unwrap();
                router.dispatch(&mut r.owner).unwrap();
                assert!(r.drain());
            }
            let wrq = lo_packet(1, 0x49, SERVICE_PORT, false, REQUEST);
            r.enqueue(&wrq);
            r.enqueue(&wrq);
            router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
            router.dispatch(&mut r.owner).unwrap();
            let coalesced = r.owner.tftp_wrq_coalesced();
            router.dispatch(&mut r.owner).unwrap();
            assert_eq!(r.owner.tftp_wrq_coalesced(), coalesced + 1);
            assert_eq!(r.owner.service_tftp_server_check_step(&mut server, &mut router),
                Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
            router.dispatch(&mut r.owner).unwrap();
            assert!(r.drain());
            if r.available.is_empty() {
                r.owner.offer_receive().unwrap();
                qcom_modem_owner::Interrupts::schedule(&mut router).unwrap();
                router.dispatch(&mut r.owner).unwrap();
                assert!(r.drain());
            }
            r.enqueue(&owner_control_frame(6, [0, 0x49, 0, 0]));
            router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
            router.dispatch(&mut r.owner).unwrap();
            let fifo = r.owner.registration_reply_fifo().unwrap();
            let close = fifo.headers.iter().flatten().find(|header|
                header.del_client == Some([0, 0x49])).unwrap();
            let request = close.tftp_request.unwrap();
            assert_eq!(request.server_result, 0xfe);
            let original = request.original.unwrap();
            assert_eq!((original.source, original.result, original.occupied, original.phases),
                (1, 0, 1, 7));
        }
        #[cfg(all(feature = "sunfish-ipa-tftp-dispatch-trace", not(feature = "sunfish-ipa-tftp-original-trace")))]
        {
            use qcom_tftp_server_check::{Address, Route, TRANSFER_COUNT};
            let mut reply = [0; 64];
            for port in 0x80..0x80 + TRANSFER_COUNT as u32 - 1 {
                server.receive(Route {
                    source: Address { node: 0, port },
                    destination: Address { node: 1, port: SERVICE_PORT },
                }, REQUEST, &mut reply).unwrap();
            }
            r.enqueue(&lo_packet(1, 0x49, SERVICE_PORT, false, REQUEST));
            router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
            router.dispatch(&mut r.owner).unwrap();
            assert!(matches!(r.owner.service_tftp_server_check_step(&mut server, &mut router),
                Ok(qcom_modem_owner::TftpServiceProgress::Ignored(_))));
            router.dispatch(&mut r.owner).unwrap();
            r.drain();
            r.enqueue(&owner_control_frame(6, [0, 0x49, 0, 0]));
            router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
            router.dispatch(&mut r.owner).unwrap();
            let fifo = r.owner.registration_reply_fifo().unwrap();
            let close = fifo.headers.iter().flatten().find(|header|
                header.del_client == Some([0, 0x49])).unwrap();
            let request = close.tftp_request.unwrap();
            assert_eq!((request.server_result, request.occupied, request.oack_queued),
                (5, TRANSFER_COUNT as u8, false));
        }
        let (intent, _) = r.sent.last().unwrap();
        r.peer.push(&control(14, 2, *intent));
        router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
        router.dispatch(&mut r.owner).unwrap();
        let completed = r.owner.ipa_registration_trace().unwrap().completed_at.unwrap();
        assert!(completed < r.owner.server_check_oacks()[0].unwrap().oack_at.unwrap());
        #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
        {
            let fifo = r.owner.registration_reply_fifo().unwrap();
            let visible = fifo.completion_visibility.unwrap();
            assert!(registration.published_at.unwrap() <= fifo.peer_tail_at.unwrap());
            assert!(visible.last_absent.unwrap() <= visible.first_visible.unwrap());
            assert!(visible.first_visible.unwrap() < completed);
            // The simulated peer has one earlier 8-byte control record.
            #[cfg(not(feature = "sunfish-ipa-tftp-dispatch-trace"))]
            assert_eq!(visible.bytes_ahead, Some(8));
            #[cfg(feature = "sunfish-ipa-reply-grant-headers")]
            {
                let opening = fifo.headers[0].unwrap();
                let completion = fifo.completion.unwrap();
                assert!(opening.tx && !completion.tx);
                assert_eq!(opening.qmi, Some((2, 0x1234, 0x20)));
                assert_eq!(completion.command, 14);
                assert_eq!(completion.parameter, opening.parameter);
                assert!(visible.first_visible.unwrap() <= completion.at.unwrap());
            }
        }
    }
    assert_eq!(r.owner.service_linux_ipa_step(clock(), &mut router).unwrap(), LP::Prepared);
    #[cfg(feature = "sunfish-ipa-reply-before-oack")]
    assert_eq!(r.owner.service_linux_ipa_step(clock(), &mut router).unwrap(), LP::Committed);
    assert_eq!(r.owner.service_linux_ipa_step(clock(), &mut router).unwrap(), LP::Queued);
    assert!(registration.queued_at < r.owner.ipa_init_trace().1.unwrap().queued_at);
}

#[cfg(feature = "sunfish-ipa-init-after-registration")]
include!("sunfish_ipa_post_registration_cases.rs");
