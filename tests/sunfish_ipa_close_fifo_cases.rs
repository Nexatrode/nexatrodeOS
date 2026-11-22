#[test]
fn exact_first_client_close_at_registration_tx_head() {
    use elf_blocker_irq::{Kernel, Router};
    use qcom_qrtr_tftp::{AuthorizedWlanImage, Server};
    use qcom_tftp_server_check::{SERVICE_PORT, ServerCheck};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    struct K;
    impl Kernel for K { fn acknowledge(&mut self, _: u8) -> Result<(), ()> { Ok(()) } }
    static CLOCK: AtomicU64 = AtomicU64::new(100);
    let clock = || CLOCK.fetch_add(1, Relaxed);
    for (stage, earlier_bytes, later, expected) in [
        #[cfg(feature = "sunfish-ipa-init-empty-rx")]
        ("after_fresh", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-empty-rx")]
        ("after_writer", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        ("grant_then_close", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        ("grant_at_registration", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        ("init_empty", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        ("init_full", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        ("init_before_store", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        ("init_partial", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        ("init_other", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        ("init_fault", 0, false, 2),
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        ("small_at_queue", 0, false, 2),
        ("already", 0, false, 4), ("write_tx", 72, false, 1), ("head", 72, false, 3),
        ("notify", 72, false, 2), ("write_tx", 64, false, 2), ("later", 0, false, 2),
        ("write_tx", 72, true, 2), ("write_tx", 144, true, 1), ("fault", 0, false, 0),
    ] {
        let mut session = lo_session();
        session.prepare_tftp_server(1, SERVICE_PORT).unwrap();
        session.enable_ipa_before_first_oack();
        let capacity = if stage == "small_at_queue" { 64 } else { 4096 };
        let mut r = LoRig::from_session_with_intents(0, false, session, false, false, capacity);
        while r.available.len() < 5 {
            r.owner.offer_receive().unwrap();
            r.drive().unwrap();
        }
        r.owner.set_rmtfs_event_clock(clock);
        let mut router = Router::new(K, 1);
        let mut server = ServerCheck::new();
        let mut fw = Server::new_bounded(1, 0, SERVICE_PORT, 0x5100, 0x51ff, &[],
            AuthorizedWlanImage::test_fixture(b"fw")).unwrap();
        r.enqueue(&owner_control_frame(4, [0x31, 0x201, 0, LO_MODEM]));
        r.enqueue(&lo_packet(1, 0x1f, SERVICE_PORT, false, qcom_tftp_server_check::REQUEST));
        if stage == "grant_at_registration" { GRANT_AFTER_DONE.set(Some(r.available[0])); }
        r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, true, &lo_message(0, 0x20, 0x1234, &[(0x10, &[1])])));
        let mut bytes = Vec::new();
        if later { bytes.extend(data(r.available.remove(0), &owner_control_frame(6, [0, 0x70, 0, 0]))); }
        bytes.extend(data(r.available.remove(0), &owner_control_frame(6, [0, 0x1f, 0, 0])));
        assert_eq!(bytes.len(), if later {144} else {72});
        #[cfg(feature = "sunfish-ipa-init-empty-rx")]
        if stage == "after_fresh" { CLOSE_ARRIVAL.replace(Some((stage, bytes.clone()))); }
        if stage == "already" { r.peer.push(&bytes); }
        if earlier_bytes != 0 || stage == "fault" {
            CLOSE_ARRIVAL.replace(Some((stage, bytes[..earlier_bytes].to_vec())));
        }
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        if stage.starts_with("init_") {
            let other = data(r.available.remove(0), &owner_control_frame(6, [0, 0x70, 0, 0]));
            let (first, later) = match stage {
                "init_partial" => (bytes[..64].to_vec(), bytes[64..].to_vec()),
                "init_other" => (other, bytes.clone()),
                "init_full" | "init_before_store" => (bytes.clone(), Vec::new()),
                _ => (Vec::new(), bytes.clone()),
            };
            INIT_SAMPLE_STAGE.set(if stage == "init_before_store" { 4 } else { 0 });
            INIT_ARRIVAL.replace(Some((first, later, stage == "init_fault")));
        }
        router.capture(1 << elf_blocker_irq::GLINK_CHANNEL).unwrap();
        router.dispatch(&mut r.owner).unwrap();
        assert!(!router.service_cold_ipa_registration(&mut r.owner, clock).unwrap());
        assert!(!r.owner.service_ipa_registration_early(clock(), &mut router).unwrap());
        assert_eq!(r.owner.service_combined_tftp_step(&mut server, &mut fw, &mut router),
            Ok(qcom_modem_owner::TftpServiceProgress::ResponseQueued));
        assert!(router.dispatch_queued_server_check_oack(&mut r.owner, clock).unwrap());
        let handoff = router.service_cold_ipa_registration(&mut r.owner, clock);
        if stage == "fault" {
            assert!(handoff.is_err());
            assert!(CLOSE_ARRIVAL.take().is_none());
            assert!(r.owner.registration_close_fifo().is_none());
            assert!(r.owner.ipa_registration_trace().unwrap().published_at.is_some());
            continue;
        }
        assert!(handoff.unwrap());
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        if stage.starts_with("init_") {
            let result = router.dispatch(&mut r.owner);
            if stage == "init_fault" {
                assert!(result.is_err());
                assert!(r.owner.ipa_init_trace().1.unwrap().published_at.is_some());
                assert!(r.owner.init_close_fifo().fresh_unread.is_none());
                println!("init_fault post-INIT cursor error propagated, sample unavailable");
                INIT_ARRIVAL.take();
                continue;
            }
            result.unwrap();
            for _ in 0..16 {
                if r.owner.server_check_oacks()[0].unwrap().first_close_at.is_some() { break; }
                router.capture(1 << elf_blocker_irq::CONTINUATION_RECEIVE_CHANNEL).unwrap();
                router.dispatch(&mut r.owner).unwrap();
            }
            let t = r.owner.init_close_fifo();
            let close = r.owner.server_check_oacks()[0].unwrap().first_close_at.unwrap();
            let published = r.owner.ipa_init_trace().1.unwrap().published_at.unwrap();
            assert_eq!(t.fresh_unread, Some(match stage { "init_full" | "init_other" | "init_before_store" => 72, "init_partial" => 64, _ => 0 }), "{stage}");
            assert_eq!(t.visible_at_sample(), Some(matches!(stage, "init_full" | "init_before_store")));
            let bounds = t.bounds.unwrap();
            let absent = bounds.last_absent.unwrap();
            let visible = bounds.first_visible.unwrap();
            assert!(published <= t.sample_start.unwrap() && t.sample_start <= t.sample_end);
            assert!(absent <= visible && visible <= close);
            if stage == "init_before_store" { assert!(visible <= published); }
            else if stage == "init_full" { assert!(visible <= t.sample_end.unwrap()); }
            else { assert!(absent >= t.sample_start.unwrap() && visible >= t.sample_end.unwrap()); }
            println!("{stage} fresh={:?} at_sample={:?} absent={absent} visible={visible} close={close}",t.fresh_unread,t.visible_at_sample());
            continue;
        }
        #[cfg(feature = "sunfish-ipa-init-empty-rx")]
        if matches!(stage, "after_fresh" | "after_writer") {
            let reg = r.owner.ipa_registration_trace().unwrap().published_at.unwrap();
            let init = r.owner.ipa_init_trace().1.expect("empty publication hands off before the next RX poll");
            assert!(init.queued_at.unwrap() > reg && init.published_at.is_none());
            if stage == "after_writer" {
                // Give the ordinary continuation one dispatch with a fitting
                // remote intent, then deliver the close regardless of TX state.
                router.dispatch(&mut r.owner).unwrap();
                assert!(r.owner.ipa_init_trace().1.unwrap().published_at.is_some());
                assert!(r.peer.log().iter().any(|op| matches!(op, Op::Write(_, bytes)
                    if bytes.first() == Some(&9) && bytes.get(48..53) == Some(&[0, 0, 0, 0x21, 0])
                        && lo_word(bytes, 44) == LO_MODEM)));
            }
        }
        #[cfg(feature = "sunfish-ipa-init-empty-rx")]
        if stage == "write_tx" && earlier_bytes == 64 {
            assert!(r.owner.ipa_init_trace().1.is_none(), "partial close must keep the early turn closed");
            assert_eq!(r.owner.ipa_init_trace().0.unwrap().post_registration as u32, 6);
        }
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        match stage {
            "grant_at_registration" => {
                let init = r.owner.ipa_init_trace().1.unwrap();
                assert!(GRANT_AFTER_DONE.get().is_none());
                assert_eq!(init.tx.queue_state, 0xc7);
                // Deliver DEL_CLIENT at the first record write, with the same
                // input timing under both schedules. Dispatch may keep polling.
                CLOSE_AFTER_NEXT_WRITE.replace(Some(bytes.clone()));
                let log_start = r.peer.log().len();
                router.dispatch(&mut r.owner).unwrap();
                let log = r.peer.log();
                let writes: Vec<_> = log[log_start..].iter().filter_map(|op| {
                    if let Op::Write(_, bytes) = op { Some(bytes) } else { None }
                }).collect();
                assert!(CLOSE_AFTER_NEXT_WRITE.take().is_none());
                if cfg!(feature = "sunfish-ipa-init-before-ad") {
                    assert_eq!(writes[0][0], 9);
                    assert_eq!(writes[0].get(48..53), Some(&[0, 0, 0, 0x21, 0][..]));
                    assert_eq!(lo_word(writes[0], 44), LO_MODEM);
                } else {
                    assert_eq!(writes[0][0], 5);
                }
            }
            "grant_then_close" => r.peer.push(&control(7, 2, 52)),
            "small_at_queue" => { router.dispatch(&mut r.owner).unwrap(); }
            _ => {}
        }
        if !matches!(stage, "already" | "after_fresh" | "grant_at_registration") && earlier_bytes < bytes.len() {
            assert_eq!(r.owner.registration_close_fifo().unwrap().status(), 0);
            r.peer.push(&bytes[earlier_bytes..]);
        }
        for _ in 0..16 {
            if r.owner.server_check_oacks()[0].unwrap().first_close_at.is_some() { break; }
            router.capture(1 << elf_blocker_irq::CONTINUATION_RECEIVE_CHANNEL).unwrap();
            router.dispatch(&mut r.owner).unwrap();
        }
        assert!(CLOSE_ARRIVAL.take().is_none(), "{stage}: injection was never delivered");
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        {
            let prefix = r.owner.wrq_prefix();
            let init = r.owner.ipa_init_trace().1.and_then(|t| t.published_at);
            let oack = r.owner.server_check_oacks()[0].and_then(|t| t.oack_at);
            for (head, publication) in [(prefix.init_head, init), (prefix.oack_head, oack)] {
                assert_eq!(head.is_some(), publication.is_some(), "{stage}");
                if let Some((head, publication)) = head.zip(publication) {
                    assert!(head < publication, "{stage}: timestamp must precede publication bookkeeping");
                }
            }
        }
        #[cfg(feature = "sunfish-ipa-init-empty-rx")]
        if matches!(stage, "after_fresh" | "after_writer") {
            let close = r.owner.server_check_oacks()[0].unwrap().first_close_at.unwrap();
            let init = r.owner.ipa_init_trace().1.unwrap();
            assert!(init.queued_at.unwrap() < close);
            assert_eq!(init.published_at.is_some_and(|at| at < close), stage == "after_writer");
            #[cfg(feature = "sunfish-ipa-init-open-writer")]
            {
                assert_eq!(init.tx.queue_state & 0x83, 0x83, "original open client with fitting, ready intent");
                let [passes, nofit, controls, data, gates] = init.tx.at_first_close.unwrap();
                assert_eq!((nofit, controls, data), (0, 0, 0));
                assert_eq!(passes, u32::from(stage == "after_writer"));
                assert_eq!(gates, if stage == "after_writer" { 0x80 } else { 0 });
            }
        }
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        if matches!(stage, "after_writer" | "after_fresh" | "grant_then_close" | "small_at_queue") {
            let init = r.owner.ipa_init_trace().1.unwrap();
            let [passes, nofit, controls, _, gates] = init.tx.at_first_close.unwrap();
            println!("{stage}: queue={:#04x}, before_close={:?}, first_pass={:?}, selected={:?}, published={:?}, close={:?}",
                init.tx.queue_state, init.tx.at_first_close, init.tx.first_pass_at, init.tx.selected_at,
                init.published_at, r.owner.server_check_oacks()[0].unwrap().first_close_at);
            if stage == "grant_then_close" {
                assert_eq!(init.tx.queue_state & 0x83, 0x83);
                assert!(passes > 0 && controls > 0 && gates & 1 != 0 && nofit == 0);
                assert!(!init.published_at.is_some_and(|at| at < r.owner.server_check_oacks()[0].unwrap().first_close_at.unwrap()));
            }
            if stage == "small_at_queue" {
                assert_eq!(init.tx.queue_state & 0x83, 0x80);
                assert!(passes > 0 && nofit == passes && gates & 0x20 != 0);
                assert!(init.published_at.is_none());
            }
        }
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        if stage == "grant_at_registration" {
            let close = r.owner.server_check_oacks()[0].unwrap().first_close_at.unwrap();
            let init = r.owner.ipa_init_trace().1.unwrap();
            let early = init.published_at.is_some_and(|t| t < close);
            assert_eq!(early, cfg!(feature = "sunfish-ipa-init-before-ad"));
            println!("grant_at_registration: queue={:#04x}, INIT before close={early}", init.tx.queue_state);
        }
        let trace = r.owner.registration_close_fifo().unwrap();
        assert_eq!(trace.status(), expected, "{stage} {earlier_bytes} {later}: {trace:?}");
        if stage != "already" {
            assert_eq!(trace.end, Some(bytes.len() as u32 + if stage == "grant_then_close" { 8 } else { 0 }));
            assert_eq!(trace.record_bytes, Some(72));
            assert_eq!(trace.fresh, Some(earlier_bytes as u32));
        }
    }
}
