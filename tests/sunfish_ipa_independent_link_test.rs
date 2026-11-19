include!("sunfish_ipa_linux_qmi_link_test.rs");

#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[test]
fn independent_receipts_survive_both_retirement_orders_and_delayed_commit() {
    for reverse_queue in [false, true] {
        for reverse_done in [false, true] {
            let mut rig = LinuxRig::new();
            let mut core = rig.core();
            let init = core.prepare_init_driver(1, 17, 0x415).unwrap();
            let handle = rig.request(0x20, 0x1234, 1);
            let reply = rig.prepare_reply(&mut core, &handle);
            // A younger publication on the server socket can be admitted
            // before an older client publication delayed by backpressure.
            if reverse_queue {
                rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();
                rig.link.queue_linux_ipa(1, &init, None).unwrap();
            } else {
                rig.link.queue_linux_ipa(1, &init, None).unwrap();
                rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();
            }
            rig.pump();
            assert_eq!(rig.output.len(), 2);
            let init_wire = rig.output.iter().find(|p| p.1[32] == 0).unwrap().clone();
            let reply_wire = rig.output.iter().find(|p| p.1[32] == 2).unwrap().clone();
            assert_eq!(&reply_wire.1[32..32 + word(&reply_wire.1, 20) as usize], reply.bytes());
            assert_eq!(&init_wire.1[32..32 + word(&init_wire.1, 20) as usize], init.bytes());
            assert!(rig.link.take_linux_ipa_receipt_for(1, &init).unwrap().is_none());
            assert!(rig.link.take_linux_ipa_receipt_for(1, &reply).unwrap().is_none());
            let (first, second) = if reverse_done { (&reply_wire, &init_wire) } else { (&init_wire, &reply_wire) };
            rig.ack_one(first, true);
            rig.ack_one(second, true);
            // Both receipts coexist. Commit server first regardless of order.
            let server_receipt = rig.link.take_linux_ipa_receipt_for(1, &reply).unwrap().unwrap();
            rig.link.commit_linux_ipa(1, &mut core, &reply, server_receipt).unwrap();
            assert!(core.state().indication_requested && !core.state().modem_ready);
            assert!(rig.link.take_linux_ipa_receipt_for(1, &reply).unwrap().is_none());
            let client_receipt = rig.link.take_linux_ipa_receipt_for(1, &init).unwrap().unwrap();
            rig.link.commit_linux_ipa(1, &mut core, &init, client_receipt).unwrap();
            assert!(rig.link.take_linux_ipa_receipt_for(1, &init).unwrap().is_none());
            assert!(rig.link.queue_linux_ipa(1, &init, None).is_err(), "no replay after commit");
            assert!(rig.link.queue_linux_ipa(1, &reply, Some(&handle)).is_err());
            core.receive_init_response(1, init.peer(), &ipa_init_response(17, 0, 0)).unwrap();
            assert!(core.state().modem_ready);
        }
    }
}

#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[test]
fn independent_wrong_owner_and_crossed_receipts_never_commit() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let init = core.prepare_init_driver(1, 9, 0x415).unwrap();
    let initial = rig.queued(&init, None);
    let handle = rig.request(0x20, 3, 1);
    let mut foreign = rig.core();
    let foreign_reply = rig.prepare_reply(&mut foreign, &handle);
    assert!(rig.link.queue_linux_ipa(1, &foreign_reply, Some(&handle)).is_err());
    let reply = rig.prepare_reply(&mut core, &handle);
    rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();
    rig.pump();
    let reply_wire = rig.output.remove(0);
    rig.ack_one(&reply_wire, true);
    assert!(rig.link.take_linux_ipa_receipt_for(1, &foreign_reply).is_err());
    assert!(rig.link.take_linux_ipa_receipt_for(2, &reply).is_err());
    assert!(rig.link.take_linux_ipa_receipt_for(1, &init).unwrap().is_none());
    let receipt = rig.link.take_linux_ipa_receipt_for(1, &reply).unwrap().unwrap();
    assert!(rig.link.commit_linux_ipa(1, &mut core, &init, receipt).is_err());
    assert!(!core.state().indication_requested && !core.state().modem_ready);
    rig.ack_one(&initial, true);
    let receipt = rig.link.take_linux_ipa_receipt_for(1, &init).unwrap().unwrap();
    rig.link.commit_linux_ipa(1, &mut core, &init, receipt).unwrap();
    assert!(!core.state().indication_requested, "rejecting wrong receipt cannot fabricate a server commit");
}

#[cfg(feature = "sunfish-ipa-independent-server-experiment")]
#[test]
fn independent_second_server_record_is_bounded_and_uncertainty_fails_both() {
    let mut rig = LinuxRig::new();
    let mut core = rig.core();
    let init = core.prepare_init_driver(1, 0, 0x415).unwrap();
    let handle = rig.request(0x20, 1, 1);
    let reply = rig.prepare_reply(&mut core, &handle);
    let bytes = ipa_server_request(2, 0x35, &[(1, &[1])]);
    assert!(matches!(core.prepare_response(1, reply.peer(), &bytes), Err(linux::Error::Busy)));
    rig.link.queue_linux_ipa(1, &init, None).unwrap();
    rig.link.queue_linux_ipa(1, &reply, Some(&handle)).unwrap();
    assert!(rig.link.queue_linux_ipa(1, &reply, Some(&handle)).is_err());
    core.publication_uncertain(1, &reply).unwrap();
    assert!(core.commit_publication(1, &init).is_err());
    assert!(core.commit_publication(1, &reply).is_err());
    assert!(core.state().failed && !core.protocol_complete());
}
