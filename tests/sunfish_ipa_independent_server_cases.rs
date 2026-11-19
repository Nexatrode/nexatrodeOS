#[test]
fn independent_comparison_registration_during_uncompleted_init() {
    for server_first in [false, true] {
        let mut r = LoRig::new();
        let init = r.next();
        r.init_response(0, 0, 0).unwrap();
        r.request(0x20, 0x1234);
        let report = r.owner.linux_ipa_report().unwrap();
        assert!(report.held_response && !report.protocol.unwrap().modem_ready);
        assert!(!report.protocol.unwrap().indication_requested);
        let independent = cfg!(feature = "sunfish-ipa-independent-server-experiment");
        assert_eq!(r.sent.len(), usize::from(independent));
        println!("independent={independent} registration published before INIT receipt={}", !r.sent.is_empty());
        let mut registration = if independent { Some(r.sent.remove(0)) } else { None };
        if let Some(packet) = &registration {
            assert_eq!(&packet.1[32..37], &[2, 0x34, 0x12, 0x20, 0]);
            assert_ne!(packet.0, init.0, "simultaneous records own distinct remote intents");
        }
        if independent && server_first {
            r.ack(registration.as_ref().unwrap(), true);
            let report = r.owner.linux_ipa_report().unwrap();
            assert!(report.protocol.unwrap().indication_requested);
            assert!(!report.protocol.unwrap().modem_ready);
            assert!(report.held_response && report.pending_record);
            assert_eq!(report.publications_committed, 1, "only the server receipt committed");
        }
        r.ack(&init, true);
        let report = r.owner.linux_ipa_report().unwrap();
        assert!(report.protocol.unwrap().modem_ready && !report.held_response);
        if !independent { registration = Some(r.next()); }
        if !independent || !server_first { r.ack(registration.as_ref().unwrap(), true); }
        assert!(r.owner.linux_ipa_report().unwrap().protocol.unwrap().indication_requested);
        r.request(0x35, 0x5678);
        let driver = r.next();
        r.ack(&driver, true);
        let indication = r.next();
        r.ack(&indication, true);
        assert!(r.owner.linux_ipa_report().unwrap().protocol_complete);
        assert_eq!(r.owner.linux_ipa_report().unwrap().publications_committed, 4);
    }
}

#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[test]
fn independent_server_progress_never_hides_init_timeout_or_replays_io() {
    let mut r = LoRig::new();
    let _init = r.next();
    r.request(0x20, 7);
    let registration = r.next();
    r.ack(&registration, true);
    assert!(r.owner.linux_ipa_report().unwrap().protocol.unwrap().indication_requested);
    r.now = 60_000;
    assert!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs).is_err());
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record && !report.protocol_complete);
    r.peer.clear();
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::Stopped));
    assert_eq!(r.owner.notified(8, &mut r.irqs), Err(OwnerError::Stopped));
    assert!(r.peer.log().is_empty());
}

#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[test]
fn independent_failure_with_two_publications_retains_both_and_stops() {
    let mut r = LoRig::new();
    let _init = r.next();
    let request = lo_message(0, 0x20, 11, &[(0x10, &[1])]);
    r.enqueue(&lo_packet(1, LO_PEER_CLIENT, LO_SERVER, false, &request));
    r.owner.notified(8, &mut r.irqs).unwrap();
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Ok(LP::Prepared));
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Ok(LP::Queued));
    r.peer.0.borrow_mut().fail_after_store = true;
    assert!(r.owner.notified(8, &mut r.irqs).is_err());
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record && report.server_pending_record && report.server_record_queued);
    assert_eq!(report.publications_committed, 0);
    assert!(!report.protocol.unwrap().indication_requested);
    r.peer.clear();
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::Stopped));
    assert!(r.peer.log().is_empty());
}

#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[test]
fn independent_server_deadline_survives_client_completion() {
    let mut r = LoRig::new();
    let init = r.next();
    r.request(0x20, 8);
    let _registration = r.next();
    r.ack(&init, true);
    r.init_response(0, 0, 0).unwrap();
    let before = r.owner.linux_ipa_report().unwrap();
    assert!(before.protocol.unwrap().modem_ready && before.server_pending_record);
    assert_eq!(before.publications_committed, 1);
    r.now = before.next_deadline.unwrap();
    assert!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs).is_err());
    let after = r.owner.linux_ipa_report().unwrap();
    assert!(after.failed && after.server_pending_record && !after.protocol_complete);
    assert_eq!(after.timeout_causes, 1, "server publication still has its own deadline");
}

#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[test]
fn independent_generation_loss_invalidates_both_publications() {
    let mut r = LoRig::new();
    let _init = r.next();
    r.request(0x20, 9);
    let _registration = r.next();
    r.peer.0.borrow_mut().observation.generation = 2;
    assert!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs).is_err());
    let report = r.owner.linux_ipa_report().unwrap();
    assert!(report.failed && report.pending_record && report.server_pending_record);
    assert_eq!(report.publications_committed, 0);
    assert!(!report.protocol_complete);
    r.peer.clear();
    assert_eq!(r.owner.service_linux_ipa_step(r.now, &mut r.irqs), Err(OwnerError::Stopped));
    assert_eq!(r.owner.notified(8, &mut r.irqs), Err(OwnerError::Stopped));
    assert!(r.peer.log().is_empty());
}
