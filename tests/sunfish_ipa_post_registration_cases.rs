#[test]
fn published_registration_admits_init_only_for_the_open_original_client() {
    use elf_blocker_irq::{Kernel, Router};
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};
    use qcom_tftp_server_check::{SERVICE_PORT, ServerCheck};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    struct K;
    impl Kernel for K {
        fn acknowledge(&mut self, _: u8) -> Result<(), ()> { Ok(()) }
    }
    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    for (peer, buffered_close, delayed) in [
        (0x1f, false, false), (0x1f, true, false), (0x21, false, false), (0x1f, false, true),
    ] {
        let mut session = lo_session();
        session.prepare_tftp_server(1, SERVICE_PORT).unwrap();
        session.enable_ipa_before_first_oack();
        let mut r = LoRig::from_session_with_intents(0, false, session, false, false,
            if delayed { 64 } else { 4096 });
        while r.available.len() < 4 {
            r.owner.offer_receive().unwrap();
            r.drive().unwrap();
        }
        r.owner.set_rmtfs_event_clock(clock);
        let mut router = Router::new(K, 1);
        let mut server = ServerCheck::new();
        let mut fw = Server::new_bounded(1, 0, SERVICE_PORT, 0x5100, 0x51ff, &[],
            AuthorizedWlanImage::test_fixture(b"fw")).unwrap();
        r.enqueue(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
        r.enqueue(&lo_packet(1, peer, SERVICE_PORT, false, qcom_tftp_server_check::REQUEST));
        r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true,
            &lo_message(0, 0x20, 0x1234, &[(0x10, &[1])])));
        if buffered_close { r.enqueue(&owner_control_frame(6, [0, peer, 0, 0])); }

        router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
        router.dispatch(&mut r.owner).unwrap();
        assert!(!router.service_cold_ipa_registration(&mut r.owner, clock).unwrap());
        assert!(!r.owner.service_ipa_registration_early(clock(), &mut router).unwrap());
        assert_eq!(r.owner.service_combined_tftp_step(&mut server, &mut fw, &mut router),
            Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
        assert!(router.dispatch_queued_server_check_oack(&mut r.owner, clock).unwrap());
        assert!(r.owner.ipa_init_trace().1.is_none());
        if delayed { router.capture(1 << 2).unwrap(); }
        assert!(router.service_cold_ipa_registration(&mut r.owner, clock).unwrap());
        if delayed {
            let reg = r.owner.ipa_registration_trace().unwrap();
            assert!(reg.queued_at.is_some() && reg.published_at.is_none());
            assert!(r.owner.ipa_init_trace().1.is_none());
            assert_eq!(router.take_other(), 1 << 2);
            router.dispatch(&mut r.owner).unwrap();
        }
        let reg = r.owner.ipa_registration_trace().unwrap();
        assert!(reg.published_at.is_some() && reg.completed_at.is_none());
        assert_eq!(r.owner.server_check_oacks()[0].unwrap().first_close_at.is_some(), buffered_close);
        let (owner_trace, init) = r.owner.ipa_init_trace();
        let expected_outcome = if peer != 0x1f { 1 } else if buffered_close { 2 } else { 5 };
        assert_eq!(owner_trace.unwrap().post_registration as u32, expected_outcome);
        if peer == 0x1f && !buffered_close {
            let init = init.expect("open first client gets INIT admission after registration publication");
            assert!(init.queued_at > reg.published_at && init.published_at.is_none());
        } else {
            assert!(init.is_none(), "close or a different original client skips the extra turn");
        }

        if peer == 0x1f && !buffered_close {
            r.enqueue(&owner_control_frame(6, [0, peer, 0, 0]));
            r.owner.notified(8, &mut r.irqs).unwrap();
            let close = r.owner.server_check_oacks()[0].unwrap().first_close_at.unwrap();
            assert!(init.unwrap().queued_at.unwrap() < close);
            if r.owner.pending_firmware_close().is_some() {
                r.owner.resolve_firmware_close(&mut fw).unwrap();
            }
        }
        r.now = clock();
        if delayed {
            r.drive().unwrap();
            assert!(r.owner.ipa_init_trace().1.unwrap().published_at.is_none());
            let mut offer = control(5, 2, 1);
            offer.extend(4096u32.to_le_bytes());
            offer.extend(59u32.to_le_bytes());
            r.peer.push(&offer);
            r.drive().unwrap();
            assert!(r.owner.ipa_init_trace().1.unwrap().published_at.is_none());
            r.peer.push(&control(8, 2, 1));
        }
        r.drive().unwrap();
        let [(reg_id, registration), (init_id, init)] = r.sent.as_slice() else {
            panic!("expected exactly one registration reply and one INIT");
        };
        assert_ne!(reg_id, init_id);
        assert_eq!((&registration[32..37], lo_word(registration,28)), (&[2,0x34,0x12,0x20,0][..],LO_PEER_CLIENT));
        assert_eq!((&init[32..37], lo_word(init,28)), (&[0,0,0,0x21,0][..],LO_MODEM));
        let (reg_id, init_id) = (*reg_id,*init_id);
        r.peer.push(&control(14,2,reg_id));
        r.drive().unwrap();
        assert!(r.owner.ipa_registration_trace().unwrap().completed_at.is_some());
        assert!(r.owner.ipa_init_trace().1.unwrap().retired_at.is_none());
        r.peer.push(&control(14,2,init_id));
        r.drive().unwrap();
        assert!(r.owner.ipa_init_trace().1.unwrap().retired_at.is_some());
        assert_eq!(r.sent.len(),2);
        assert_eq!(r.owner.ipa_init_trace().0.unwrap().post_registration as u32, expected_outcome);
    }
}

#[cfg(feature = "sunfish-ipa-reg-close-fifo")]
include!("sunfish_ipa_close_fifo_cases.rs");
