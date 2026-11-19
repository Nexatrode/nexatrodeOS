#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/ath10k_htt.rs"]
mod ath10k_htt;
#[path = "../rust/src/ath10k_key.rs"]
mod ath10k_key;
#[path = "../rust/src/ath10k_wmi_tx.rs"]
mod ath10k_wmi_tx;
#[path = "../rust/src/wlan_key_confirmation.rs"]
mod wlan_key_confirmation;

use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
use ath10k_htt::{HTT_SECURITY_TYPE_AES_CCMP, HttSecurityIndication};
use ath10k_key::{
    CCMP128_KEY_BYTES, CcmpKeyInstall, CcmpKeyInstallTracker, Error, KeyInstallAction, KeyKind,
    QueueError, TrackerError, VDEV_INSTALL_KEY_FIXED_BYTES, WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID,
    encode_ccmp_install_key, queue_ccmp_install_key, security_indication_confirms_ccmp_install,
};
use ath10k_wmi_tx::WmiCommandTxQueue;

fn request<'a>(key: &'a [u8; CCMP128_KEY_BYTES], kind: KeyKind) -> CcmpKeyInstall<'a> {
    CcmpKeyInstall {
        vdev_id: 7,
        peer_address: [0x02, 1, 2, 3, 4, 5],
        key_index: 2,
        kind,
        temporal_key: key,
    }
}

fn queue() -> WmiCommandTxQueue<4> {
    WmiCommandTxQueue::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 128,
        },
        HtcServicePolicy {
            receive_allocation: 16,
            disable_credit_flow: false,
            credit_size: 16,
            initial_credits: 16,
        },
        false,
        9,
    )
    .unwrap()
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[test]
fn encodes_pairwise_ccmp_with_linux_zeroed_counters() {
    let key = [0x5a; CCMP128_KEY_BYTES];
    let mut body = [0xa5; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES + 4];
    let command = encode_ccmp_install_key(request(&key, KeyKind::Pairwise), &mut body).unwrap();

    assert_eq!(command.command_id, WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID);
    assert_eq!(command.body_bytes, 108);
    assert_eq!(word(&body, 0), 7);
    assert_eq!(&body[4..10], &[0x02, 1, 2, 3, 4, 5]);
    assert_eq!(&body[10..12], &[0, 0]);
    assert_eq!(word(&body, 12), 2);
    assert_eq!(word(&body, 16), 0); // WMI_KEY_PAIRWISE
    assert_eq!(word(&body, 20), 4); // WMI_CIPHER_AES_CCM
    assert!(body[24..80].iter().all(|byte| *byte == 0));
    assert_eq!(word(&body, 80), 16);
    assert_eq!(word(&body, 84), 0);
    assert_eq!(word(&body, 88), 0);
    assert_eq!(&body[92..108], &key);
    assert_eq!(&body[108..], &[0xa5; 4]);
}

#[test]
fn encodes_group_key_index_and_group_flag() {
    let key = [0x33; CCMP128_KEY_BYTES];
    let mut body = [0; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES];
    let command = encode_ccmp_install_key(request(&key, KeyKind::Group), &mut body).unwrap();

    assert_eq!(command.kind, KeyKind::Group);
    assert_eq!(command.key_index, 2);
    assert_eq!(word(&body, 16), 1); // WMI_KEY_GROUP
    assert_eq!(&body[92..108], &key);
}

#[test]
fn rejects_bad_index_or_short_output_without_mutation() {
    let key = [0x7c; CCMP128_KEY_BYTES];
    let mut body = [0xa5; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES];
    let mut bad_index = request(&key, KeyKind::Pairwise);
    bad_index.key_index = 4;
    assert_eq!(
        encode_ccmp_install_key(bad_index, &mut body),
        Err(Error::KeyIndexOutOfRange)
    );
    assert!(body.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_ccmp_install_key(request(&key, KeyKind::Pairwise), &mut body[..107]),
        Err(Error::OutputTooSmall)
    );
    assert!(body.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn queues_key_body_on_wmi_control_and_preserves_wire_bytes() {
    let key = [0x96; CCMP128_KEY_BYTES];
    let mut queue = queue();
    let mut body = [0xa5; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES];
    let mut frame = [0xa5; 128];
    let queued = queue_ccmp_install_key(
        &mut queue,
        request(&key, KeyKind::Pairwise),
        0x8800_1000,
        7,
        0x1234,
        &mut body,
        &mut frame,
    )
    .unwrap();

    assert_eq!(
        queued.command.command_id,
        WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID
    );
    assert_eq!(queued.transfer.slot, 0);
    assert_eq!(word(&frame, 8), WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID);
    assert_eq!(word(&frame, 12), 7);
    assert_eq!(&body[92..108], &key);
    assert_eq!(&frame[8 + 4 + 92..8 + 4 + 108], &key);
    assert_eq!(queue.ring_indices(), (1, 0));
}

#[test]
fn refuses_an_unbound_generic_queue() {
    let key = [0x11; CCMP128_KEY_BYTES];
    let mut queue = WmiCommandTxQueue::<4>::new(3, 128, 16, 16, true, false, 9).unwrap();
    let mut body = [0xa5; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES];
    let mut frame = [0xa5; 128];
    assert_eq!(
        queue_ccmp_install_key(
            &mut queue,
            request(&key, KeyKind::Group),
            0x8800_1000,
            7,
            0x1234,
            &mut body,
            &mut frame,
        ),
        Err(QueueError::NotWmiControlEndpoint)
    );
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn key_completion_matcher_requires_peer_ccmp_and_pairwise_or_group_cast() {
    let event = HttSecurityIndication {
        peer_id: 0x1234,
        security_type: HTT_SECURITY_TYPE_AES_CCMP,
        is_unicast: true,
    };
    assert!(security_indication_confirms_ccmp_install(
        event,
        0x1234,
        KeyKind::Pairwise
    ));
    assert!(!security_indication_confirms_ccmp_install(
        event,
        0x1235,
        KeyKind::Pairwise
    ));
    assert!(!security_indication_confirms_ccmp_install(
        event,
        0x1234,
        KeyKind::Group
    ));
    assert!(!security_indication_confirms_ccmp_install(
        HttSecurityIndication {
            security_type: 4,
            ..event
        },
        0x1234,
        KeyKind::Pairwise
    ));
}

#[test]
fn group_key_completion_requires_nonunicast_ccmp_indication() {
    assert!(security_indication_confirms_ccmp_install(
        HttSecurityIndication {
            peer_id: 9,
            security_type: HTT_SECURITY_TYPE_AES_CCMP,
            is_unicast: false,
        },
        9,
        KeyKind::Group
    ));
}

fn queued_install(kind: KeyKind, key_index: u8) -> ath10k_key::QueuedKeyInstall {
    let key = [0x42; CCMP128_KEY_BYTES];
    let mut install = request(&key, kind);
    install.key_index = key_index;
    let mut queue = queue();
    let mut body = [0; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES];
    let mut frame = [0; 128];
    queue_ccmp_install_key(
        &mut queue,
        install,
        0x8800_1000,
        7,
        0x1234,
        &mut body,
        &mut frame,
    )
    .unwrap()
}

fn indication(peer_id: u16, is_unicast: bool) -> HttSecurityIndication {
    HttSecurityIndication {
        peer_id,
        security_type: HTT_SECURITY_TYPE_AES_CCMP,
        is_unicast,
    }
}

#[test]
fn key_tracker_requires_queued_pairwise_and_group_confirmations_for_generation() {
    let mut tracker =
        CcmpKeyInstallTracker::new(12, 7, 0x1234, [0x02, 1, 2, 3, 4, 5], 2, Some(1)).unwrap();
    let pairwise = queued_install(KeyKind::Pairwise, 2);
    let group = queued_install(KeyKind::Group, 1);

    assert_eq!(
        tracker.observe_security_indication(12, indication(0x1234, true)),
        KeyInstallAction::IgnoredBeforeQueue
    );
    tracker.record_queued(12, pairwise).unwrap();
    tracker.record_queued(12, group).unwrap();
    tracker
        .mark_published(12, KeyKind::Pairwise, pairwise.transfer)
        .unwrap();
    assert_eq!(
        tracker.observe_security_indication(12, indication(0x1234, false)),
        KeyInstallAction::IgnoredBeforePublication
    );
    tracker
        .mark_published(12, KeyKind::Group, group.transfer)
        .unwrap();
    assert_eq!(
        tracker.observe_security_indication(11, indication(0x1234, true)),
        KeyInstallAction::IgnoredStaleGeneration
    );
    assert_eq!(
        tracker.observe_security_indication(12, indication(0x1235, true)),
        KeyInstallAction::IgnoredMismatch
    );
    assert_eq!(
        tracker.observe_security_indication(
            12,
            HttSecurityIndication {
                security_type: 0,
                ..indication(0x1234, true)
            }
        ),
        KeyInstallAction::IgnoredMismatch
    );

    assert_eq!(
        tracker.observe_security_indication(12, indication(0x1234, true)),
        KeyInstallAction::Installed {
            kind: KeyKind::Pairwise,
            all_required_installed: false,
        }
    );
    assert_eq!(
        tracker.observe_security_indication(12, indication(0x1234, true)),
        KeyInstallAction::Duplicate {
            kind: KeyKind::Pairwise
        }
    );
    assert!(!tracker.all_required_installed());
    assert_eq!(
        tracker.observe_security_indication(12, indication(0x1234, false)),
        KeyInstallAction::Installed {
            kind: KeyKind::Group,
            all_required_installed: true,
        }
    );
    assert!(tracker.all_required_installed());
    assert!(tracker.confirms_keys_for_association(12, [0x02, 1, 2, 3, 4, 5], 2, Some(1),));
    assert!(!tracker.confirms_keys_for_association(11, [0x02, 1, 2, 3, 4, 5], 2, Some(1),));
    assert!(!tracker.confirms_keys_for_association(12, [0x02, 1, 2, 3, 4, 6], 2, Some(1),));
    assert!(!tracker.confirms_keys_for_association(12, [0x02, 1, 2, 3, 4, 5], 1, Some(1),));
    assert!(!tracker.confirms_keys_for_association(12, [0x02, 1, 2, 3, 4, 5], 2, Some(0),));
}

#[test]
fn key_indication_before_descriptor_publication_cannot_confirm_install() {
    let mut tracker =
        CcmpKeyInstallTracker::new(3, 7, 0x1234, [0x02, 1, 2, 3, 4, 5], 2, None).unwrap();
    let queued = queued_install(KeyKind::Pairwise, 2);
    tracker.record_queued(3, queued).unwrap();
    assert_eq!(
        tracker.mark_published(3, KeyKind::Group, queued.transfer),
        Err(TrackerError::NotQueuedForPublication)
    );
    assert_eq!(
        tracker.observe_security_indication(3, indication(0x1234, true)),
        KeyInstallAction::IgnoredBeforePublication
    );
    assert!(!tracker.all_required_installed());
    assert_eq!(
        tracker.mark_published(2, KeyKind::Pairwise, queued.transfer),
        Err(TrackerError::StaleGeneration)
    );
    let mut wrong = queued.transfer;
    wrong.slot += 1;
    assert_eq!(
        tracker.mark_published(3, KeyKind::Pairwise, wrong),
        Err(TrackerError::PublicationMismatch)
    );
    wrong = queued.transfer;
    wrong.transfer.descriptor =
        ath10k_ce::Wcn3990Descriptor::encode(0x9000_0000, 112, 7, false, false).unwrap();
    assert_eq!(
        tracker.mark_published(3, KeyKind::Pairwise, wrong),
        Err(TrackerError::PublicationMismatch)
    );
    tracker
        .mark_published(3, KeyKind::Pairwise, queued.transfer)
        .unwrap();
    assert_eq!(
        tracker.mark_published(3, KeyKind::Pairwise, queued.transfer),
        Err(TrackerError::DuplicatePublication)
    );
    assert_eq!(
        tracker.observe_security_indication(3, indication(0x1234, true)),
        KeyInstallAction::Installed {
            kind: KeyKind::Pairwise,
            all_required_installed: true
        }
    );
}

#[test]
fn key_tracker_pairwise_only_completes_and_ignores_group_indications() {
    let mut tracker =
        CcmpKeyInstallTracker::new(3, 7, 0x1234, [0x02, 1, 2, 3, 4, 5], 2, None).unwrap();
    let queued = queued_install(KeyKind::Pairwise, 2);
    tracker.record_queued(3, queued).unwrap();
    tracker
        .mark_published(3, KeyKind::Pairwise, queued.transfer)
        .unwrap();
    assert_eq!(
        tracker.observe_security_indication(3, indication(0x1234, false)),
        KeyInstallAction::IgnoredUnexpectedKind
    );
    assert!(!tracker.all_required_installed());
    assert_eq!(
        tracker.observe_security_indication(3, indication(0x1234, true)),
        KeyInstallAction::Installed {
            kind: KeyKind::Pairwise,
            all_required_installed: true,
        }
    );
    assert!(tracker.all_required_installed());
}

#[test]
fn key_tracker_rejects_stale_mismatched_and_duplicate_queue_records() {
    let mut tracker =
        CcmpKeyInstallTracker::new(5, 7, 0x1234, [0x02, 1, 2, 3, 4, 5], 2, Some(1)).unwrap();
    let pairwise = queued_install(KeyKind::Pairwise, 2);
    assert_eq!(
        tracker.record_queued(4, pairwise),
        Err(TrackerError::StaleGeneration)
    );
    assert_eq!(
        tracker.record_queued(
            5,
            ath10k_key::QueuedKeyInstall {
                command: ath10k_key::CommandLayout {
                    command_id: 0,
                    ..pairwise.command
                },
                ..pairwise
            }
        ),
        Err(TrackerError::CommandEncodingMismatch)
    );
    assert_eq!(
        tracker.record_queued(
            5,
            ath10k_key::QueuedKeyInstall {
                command: ath10k_key::CommandLayout {
                    vdev_id: 8,
                    ..pairwise.command
                },
                ..pairwise
            }
        ),
        Err(TrackerError::CommandIdentityMismatch)
    );
    assert_eq!(
        tracker.record_queued(
            5,
            ath10k_key::QueuedKeyInstall {
                command: ath10k_key::CommandLayout {
                    key_index: 1,
                    ..pairwise.command
                },
                ..pairwise
            }
        ),
        Err(TrackerError::KeyIndexMismatch)
    );
    tracker.record_queued(5, pairwise).unwrap();
    assert_eq!(
        tracker.record_queued(5, pairwise),
        Err(TrackerError::DuplicateQueuedCommand)
    );
    assert_eq!(
        tracker.record_queued(5, queued_install(KeyKind::Group, 0)),
        Err(TrackerError::KeyIndexMismatch)
    );
    assert!(!tracker.all_required_installed());
}
