#![allow(dead_code)]
#![forbid(unsafe_code)]
#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/ath10k_htt.rs"]
mod ath10k_htt;
#[path = "../rust/src/ath10k_peer.rs"]
mod ath10k_peer;
#[path = "../rust/src/ath10k_vdev.rs"]
mod ath10k_vdev;
#[path = "../rust/src/ath10k_wmi_tx.rs"]
mod ath10k_wmi_tx;
use ath10k_htt::*;
use ath10k_peer::*;

#[test]
fn peer_map_unmap_decode_packed_little_endian_and_bound_lengths() {
    let map = [3, 7, 0x34, 0x02, 2, 1, 2, 3, 4, 5, 0xaa, 0xbb];
    assert_eq!(
        HttPeerMap::parse(&map),
        Ok(HttPeerMap {
            vdev_id: 7,
            peer_id: 0x234,
            mac: [2, 1, 2, 3, 4, 5]
        })
    );
    assert!(HttPeerMap::parse(&map[..11]).is_err());
    assert!(HttPeerMap::parse(&[4; 12]).is_err());
    assert_eq!(
        HttPeerUnmap::parse(&[4, 0xff, 0x34, 0x02]),
        Ok(HttPeerUnmap { peer_id: 0x234 })
    );
    assert!(HttPeerUnmap::parse(&[4, 0, 0]).is_err());
}

#[test]
fn peer_tracker_requires_published_create_and_tracks_multiple_peer_ids() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::WmiCommandTxQueue;
    let mac = [2, 1, 2, 3, 4, 5];
    let mut t = PeerTracker::new(9, 7, mac).unwrap();
    let mut q = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
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
    .unwrap();
    let mut frame = [0; 128];
    let queued = queue_peer_create_tracked(
        &mut q,
        &mut t,
        9,
        ath10k_vdev::PeerType::Default,
        0x100000,
        12,
        5,
        &mut frame,
    )
    .unwrap();
    let map = HttPeerMap {
        vdev_id: 7,
        peer_id: 0x234,
        mac,
    };
    assert_eq!(t.on_map(9, map), PeerAction::Ignored);
    assert!(!t.contains_peer_id(9, 0x234));
    assert_eq!(
        t.mark_published(9, queued.slot, 13),
        Err(PeerError::WrongTransfer)
    );
    t.mark_published(9, queued.slot, 12).unwrap();
    assert_eq!(t.timeout(8), Err(PeerError::WrongGeneration));
    assert_eq!(t.on_map(8, map), PeerAction::Ignored);
    assert_eq!(
        t.on_map(9, HttPeerMap { vdev_id: 8, ..map }),
        PeerAction::Ignored
    );
    assert_eq!(
        t.on_map(9, map),
        PeerAction::Mapped {
            peer_id: 0x234,
            first: true
        }
    );
    assert!(t.contains_peer_id(9, 0x234));
    assert_eq!(t.timeout(9), Err(PeerError::NotQueued));
    assert!(!t.contains_peer_id(8, 0x234));
    assert!(!t.contains_peer_id(9, 2048));
    assert_eq!(t.on_map(9, map), PeerAction::Duplicate { peer_id: 0x234 });
    assert_eq!(
        t.on_map(
            9,
            HttPeerMap {
                peer_id: 0x235,
                ..map
            }
        ),
        PeerAction::Mapped {
            peer_id: 0x235,
            first: false
        }
    );
    assert_eq!(
        t.on_unmap(9, HttPeerUnmap { peer_id: 0x234 }),
        PeerAction::Unmapped {
            peer_id: 0x234,
            remaining: true
        }
    );
    assert!(!t.contains_peer_id(9, 0x234));
    assert!(t.contains_peer_id(9, 0x235));
    assert_eq!(
        t.on_unmap(9, HttPeerUnmap { peer_id: 0x235 }),
        PeerAction::Unmapped {
            peer_id: 0x235,
            remaining: false
        }
    );
    assert_eq!(t.on_map(9, map), PeerAction::Ignored);
}

#[test]
fn peer_creation_timeout_rejects_late_maps() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::WmiCommandTxQueue;
    let mac = [2, 1, 2, 3, 4, 5];
    let mut tracker = PeerTracker::new(9, 7, mac).unwrap();
    let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
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
    .unwrap();
    let mut frame = [0; 128];
    let entry = queue_peer_create_tracked(
        &mut queue,
        &mut tracker,
        9,
        ath10k_vdev::PeerType::Default,
        0x100000,
        12,
        0,
        &mut frame,
    )
    .unwrap();
    tracker.mark_published(9, entry.slot, 12).unwrap();
    tracker.timeout(9).unwrap();
    assert_eq!(
        tracker.on_map(
            9,
            HttPeerMap {
                vdev_id: 7,
                peer_id: 1,
                mac
            }
        ),
        PeerAction::Ignored
    );
    assert!(!tracker.contains_peer_id(9, 1));
}

#[test]
fn peer_id_outside_linux_bound_fails_closed() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::WmiCommandTxQueue;
    let mac = [2, 1, 2, 3, 4, 5];
    let mut tracker = PeerTracker::new(1, 0, mac).unwrap();
    let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
        },
        HtcServicePolicy {
            receive_allocation: 16,
            disable_credit_flow: false,
            credit_size: 16,
            initial_credits: 16,
        },
        false,
        0,
    )
    .unwrap();
    let mut frame = [0; 128];
    let entry = queue_peer_create_tracked(
        &mut queue,
        &mut tracker,
        1,
        ath10k_vdev::PeerType::Default,
        0x100000,
        3,
        0,
        &mut frame,
    )
    .unwrap();
    tracker.mark_published(1, entry.slot, 3).unwrap();
    let invalid = HttPeerMap {
        vdev_id: 0,
        peer_id: 2048,
        mac,
    };
    assert_eq!(tracker.on_map(1, invalid), PeerAction::InvalidPeerId(2048));
    assert_eq!(
        tracker.on_map(
            1,
            HttPeerMap {
                peer_id: 2,
                ..invalid
            }
        ),
        PeerAction::Ignored
    );
}
