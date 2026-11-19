#[test]
fn replay_original_grant() {
    run_grant_schedule(false, 15..=60);
}

#[cfg(feature = "sunfish-grant-ad-writer-trace")]
#[test]
fn trace_ordinary_ad_after_registration() {
    run_grant_schedule(true, 20..=60);
}

#[cfg(feature = "sunfish-grant-ad-writer-trace")]
#[test]
fn trace_registration_ad_at_close_boundary() {
    run_grant_schedule(false, 4..=9);
}

fn run_grant_schedule(late_grant: bool, deadlines: std::ops::RangeInclusive<u32>) {
    use elf_blocker_irq::{Kernel, Router};
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};
    use qcom_tftp_server_check::{SERVICE_PORT, ServerCheck};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    struct K;
    impl Kernel for K {
        fn acknowledge(&mut self, _: u8) -> Result<(), ()> {
            Ok(())
        }
    }
    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    for deadline in deadlines {
        COUNT.set(None);
        ANCHOR.set(None);
        PHASES.take();
        DEADLINE.set(deadline);
        let mut session = lo_session();
        session.prepare_tftp_server(1, SERVICE_PORT).unwrap();
        session.enable_ipa_before_first_oack();
        session.enable_first_oack_after_ipa_publish();
        let capacity = 4096;
        let mut r = LoRig::from_session_with_intents(0, false, session, false, false, capacity);
        while r.available.len() < 5 {
            r.owner.offer_receive().unwrap();
            r.drive().unwrap();
        }
        r.owner.set_rmtfs_event_clock(clock);
        let mut router = Router::new(K, 1);
        let mut server = ServerCheck::new();
        let mut fw = Server::new_bounded(
            1,
            0,
            SERVICE_PORT,
            0x5100,
            0x51ff,
            &[],
            AuthorizedWlanImage::test_fixture(b"fw"),
        )
        .unwrap();
        r.enqueue(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
        r.enqueue(&lo_packet(
            1,
            0x1f,
            SERVICE_PORT,
            false,
            qcom_tftp_server_check::REQUEST,
        ));
        if !late_grant {
            GRANT_AFTER_DONE.set(Some(r.available[0]));
        }
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        GRANT_AFTER_REGISTRATION.set(late_grant);
        r.enqueue(&lo_packet(
            1,
            LO_PEER_CLIENT,
            LO_SERVER,
            true,
            &lo_message(0, 0x20, 0x1234, &[(0x10, &[1])]),
        ));
        let close_record = data(
            r.available.remove(0),
            &owner_control_frame(6, [0, 0x1f, 0, 0]),
        );
        let start = r.peer.log().len();
        COUNT.set(Some(0));
        FRAME.replace(Some(close_record));
        router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
        router.dispatch(&mut r.owner).unwrap();
        assert!(
            !router
                .service_cold_ipa_registration(&mut r.owner, clock)
                .unwrap()
        );
        assert!(
            !r.owner
                .service_ipa_registration_early(clock(), &mut router)
                .unwrap()
        );
        assert_eq!(
            r.owner
                .service_combined_tftp_step(&mut server, &mut fw, &mut router),
            Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued)
        );
        mark("oack-queued");
        assert!(
            router
                .dispatch_queued_server_check_oack(&mut r.owner, clock)
                .unwrap()
        );
        assert!(
            router
                .service_cold_ipa_registration(&mut r.owner, clock)
                .unwrap()
        );
        let transfer = r.owner.server_check_oacks()[0].unwrap();
        assert_eq!(transfer.peer.port, 0x1f);
        if r.owner
            .ipa_init_trace()
            .1
            .is_some_and(|t| t.queued_at.is_some())
        {
            mark("init-admitted-by-return");
        }
        for _ in 0..120 {
            router
                .capture(1 << elf_blocker_irq::CONTINUATION_RECEIVE_CHANNEL)
                .unwrap();
            router.dispatch(&mut r.owner).unwrap();
            let first = r.owner.server_check_oacks()[0].unwrap();
            if first.first_close_at.is_some() {
                break;
            }
        }
        assert!(FRAME.take().is_none(), "injection missed");
        let end = r.owner.server_check_oacks()[0].unwrap();
        let close = end.first_close_at.unwrap();
        let grant = r.owner.first_grant_trace();
        let mask = r.owner.first_oack_eligibility().close_mask;
        let success = end.oack_at.is_some();
        assert!(end.first_data_at.is_none());
        let log = r.peer.log();
        // A staged write that yields at readback never crosses the TX head.
        let mut staged = None;
        let mut writes = Vec::new();
        for op in &log[start..] {
            match op {
                Op::Write(offset, b) => staged = Some((offset, b)),
                Op::Head(head) => {
                    let (offset, b) = staged.take().unwrap();
                    assert_eq!(*head as usize, (*offset + b.len()) % FIFO_BYTES);
                    writes.push(b);
                }
                _ => {}
            }
        }
        let commands: Vec<_> = writes.iter().map(|r| r[0]).collect();
        let phases = PHASES.take();
        COUNT.set(None);
        println!(
            "deadline={deadline} success={success} outcome={} mask={mask:?} commands={commands:?} phases={phases:?}",
            grant.outcome
        );
        let ad = writes.iter().position(|b| b[0] == 5).unwrap();
        let ack = writes.iter().position(|b| b[0] == 8).unwrap();
        assert!(ad < ack);
        assert!(lo_word(writes[ad], 8) >= 52 && lo_word(writes[ack], 4) == 1);
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        {
            use qcom_glink_qmi::GrantAdWriter;
            let captured = grant.backing;
            let published = r.owner.ipa_registration_trace().unwrap().published_at.unwrap();
            assert_eq!(captured.requested, 52);
            assert_eq!(captured.intent, Some(lo_word(writes[ad], 12)));
            let reserved = captured.reserved_at.unwrap();
            assert_eq!(reserved > published, late_grant);
            let fallback = !late_grant && deadline <= 5;
            let expected = if late_grant || fallback { GrantAdWriter::Ordinary } else { GrantAdWriter::Registration };
            assert_eq!(captured.writer, Some(expected));
            let advertised = captured.advertised_at.unwrap();
            assert!(advertised > reserved && advertised > published);
            if fallback {
                assert!(grant.advertisement_at.is_none());
                let injected = phases.iter().position(|p| p.0 == "injected").unwrap();
                let ad_head = phases.iter().position(|p| p.0 == "ad-head").unwrap();
                assert!(ad_head > injected);
            } else {
                assert!(advertised <= grant.advertisement_at.unwrap());
            }
        }
        let sends: Vec<_> = writes
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                b[0] == 9 && lo_word(b, 44) == transfer.peer.port && b.get(48..50) == Some(&[0, 6])
            })
            .collect();
        assert_eq!(sends.len(), usize::from(success));
        if success {
            assert!(grant.advertisement_at < grant.ack_at && grant.ack_at < end.oack_at);
            let oack = phases.iter().position(|p| p.0 == "oack-head").unwrap();
            let close = phases.iter().position(|p| p.0 == "injected").unwrap();
            assert!(oack < close);
        }
        if let Some((i, b)) = sends.first() {
            assert!(ack < *i);
            assert_eq!(lo_word(b, 28), transfer.transfer_port);
        }
        assert_eq!(end.oack_at.is_some_and(|t| t < close), success);
    }
}
