//! Host-side peer-unmap to association teardown. No hardware event or DMA owner.
#![forbid(unsafe_code)]

use crate::ath10k_htt::{HttPeerUnmap, HttSecurityIndication};
use crate::ath10k_key::{CcmpKeyInstallTracker, KeyInstallAction};
use crate::ath10k_peer::{PeerAction, PeerTracker};
use crate::ieee80211_join::{JoinPhase, Wpa2Join};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinPeerUnmap {
    Ignored,
    PeerRetained,
    AssociationLost,
}

/// Feed a security indication to the key tracker only when its peer ID is
/// currently mapped to the association and both trackers agree on identity.
/// The hardware owner must supply trusted generations and dispatch events in
/// order; software queueing alone still does not prove hardware publication.
pub fn observe_mapped_security(
    peer: &PeerTracker,
    join: &Wpa2Join<'_>,
    keys: &mut CcmpKeyInstallTracker,
    transport_generation: u64,
    association_generation: u32,
    vdev_id: u8,
    event: HttSecurityIndication,
) -> Option<KeyInstallAction> {
    let bssid = join.bssid();
    if join.association_generation() != association_generation
        || join.phase() == JoinPhase::Failed
        || !peer.matches_identity(transport_generation, vdev_id, bssid)
        || !peer.contains_peer_id(transport_generation, event.peer_id)
        || !keys.matches_peer(association_generation, vdev_id, event.peer_id, bssid)
    {
        return None;
    }
    Some(keys.observe_security_indication(association_generation, event))
}

/// Consume an unmap only for this join's BSSID, vdev and association epoch.
/// The caller must supply the actual transport epoch, enforce global peer-ID
/// uniqueness, and close/retire any already-published hardware traffic when
/// AssociationLost is returned.
pub fn unmap_join_peer(
    peer: &mut PeerTracker,
    join: &mut Wpa2Join<'_>,
    transport_generation: u64,
    association_generation: u32,
    vdev_id: u8,
    event: HttPeerUnmap,
) -> JoinPeerUnmap {
    let bssid = join.bssid();
    if join.association_generation() != association_generation
        || join.phase() == JoinPhase::Failed
        || !peer.matches_identity(transport_generation, vdev_id, bssid)
    {
        return JoinPeerUnmap::Ignored;
    }
    match peer.on_unmap(transport_generation, event) {
        PeerAction::Unmapped {
            remaining: true, ..
        } => JoinPeerUnmap::PeerRetained,
        PeerAction::Unmapped {
            remaining: false, ..
        } => {
            if join.peer_lost(association_generation, bssid) {
                JoinPeerUnmap::AssociationLost
            } else {
                JoinPeerUnmap::Ignored
            }
        }
        _ => JoinPeerUnmap::Ignored,
    }
}

#[cfg(test)]
mod tests {
    use super::{JoinPeerUnmap, observe_mapped_security, unmap_join_peer};
    use crate::ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use crate::ath10k_htt::{
        HTT_SECURITY_TYPE_AES_CCMP, HttPeerMap, HttPeerUnmap, HttSecurityIndication,
    };
    use crate::ath10k_key::{
        CcmpKeyInstall, CcmpKeyInstallTracker, KeyInstallAction, KeyKind, queue_ccmp_install_key,
    };
    use crate::ath10k_peer::{PeerTracker, queue_peer_create_tracked};
    use crate::ath10k_vdev::PeerType;
    use crate::ath10k_wmi_tx::WmiCommandTxQueue;
    use crate::ieee80211_join::{JoinConfig, JoinPhase, Wpa2Join};

    const BSSID: [u8; 6] = [2, 0, 0, 0, 0, 2];
    const RSN: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];

    fn fixture(mac: [u8; 6]) -> (PeerTracker, Wpa2Join<'static>) {
        let mut peer = PeerTracker::new(9, 7, mac).unwrap();
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
        let queued = queue_peer_create_tracked(
            &mut q,
            &mut peer,
            9,
            PeerType::Default,
            0x100000,
            12,
            0,
            &mut [0; 128],
        )
        .unwrap();
        peer.mark_published(9, queued.slot, 12).unwrap();
        for id in [3, 4] {
            peer.on_map(
                9,
                HttPeerMap {
                    vdev_id: 7,
                    peer_id: id,
                    mac,
                },
            );
        }
        let join = Wpa2Join::new(JoinConfig {
            association_generation: 5,
            station: [2, 0, 0, 0, 0, 1],
            bssid: BSSID,
            initial_sequence_number: 0,
            ssid: b"test-network",
            passphrase: b"correct horse battery",
            rsn_ie: &RSN,
            supported_rates: &[0x82, 0x84],
            extended_supported_rates: &[],
            additional_ies: &[],
            capability_info: 0x11,
            listen_interval: 10,
            snonce: [0x33; 32],
        })
        .unwrap();
        (peer, join)
    }

    #[test]
    fn only_last_matching_map_revokes_join() {
        let (mut peer, mut join) = fixture(BSSID);
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 8, 5, 7, HttPeerUnmap { peer_id: 3 }),
            JoinPeerUnmap::Ignored
        );
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 9, 4, 7, HttPeerUnmap { peer_id: 3 }),
            JoinPeerUnmap::Ignored
        );
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 9, 5, 8, HttPeerUnmap { peer_id: 3 }),
            JoinPeerUnmap::Ignored
        );
        assert!(peer.contains_peer_id(9, 3));
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 9, 5, 7, HttPeerUnmap { peer_id: 3 }),
            JoinPeerUnmap::PeerRetained
        );
        assert_eq!(join.phase(), JoinPhase::Ready);
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 9, 5, 7, HttPeerUnmap { peer_id: 4 }),
            JoinPeerUnmap::AssociationLost
        );
        assert_eq!(join.phase(), JoinPhase::Failed);
        assert!(!join.is_authorized());
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 9, 5, 7, HttPeerUnmap { peer_id: 4 }),
            JoinPeerUnmap::Ignored
        );
    }

    #[test]
    fn unrelated_peer_cannot_end_join_or_consume_its_map() {
        let (mut peer, mut join) = fixture([2, 0, 0, 0, 0, 9]);
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 9, 5, 7, HttPeerUnmap { peer_id: 3 }),
            JoinPeerUnmap::Ignored
        );
        assert!(peer.contains_peer_id(9, 3));
        assert_eq!(join.phase(), JoinPhase::Ready);
    }

    #[test]
    fn security_event_requires_live_mapping_and_matching_key_tracker() {
        let (mut peer, mut join) = fixture(BSSID);
        let mut keys = CcmpKeyInstallTracker::new(5, 7, 3, BSSID, 0, None).unwrap();
        let sec = HttSecurityIndication {
            peer_id: 3,
            security_type: HTT_SECURITY_TYPE_AES_CCMP,
            is_unicast: true,
        };
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut keys, 8, 5, 7, sec),
            None
        );
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut keys, 9, 4, 7, sec),
            None
        );
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut keys, 9, 5, 8, sec),
            None
        );
        assert_eq!(
            observe_mapped_security(
                &peer,
                &join,
                &mut keys,
                9,
                5,
                7,
                HttSecurityIndication { peer_id: 5, ..sec }
            ),
            None
        );
        let mut wrong_vdev = CcmpKeyInstallTracker::new(5, 8, 3, BSSID, 0, None).unwrap();
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut wrong_vdev, 9, 5, 7, sec),
            None
        );
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut keys, 9, 5, 7, sec),
            Some(KeyInstallAction::IgnoredBeforeQueue)
        );
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
        let install = CcmpKeyInstall {
            vdev_id: 7,
            peer_address: BSSID,
            key_index: 0,
            kind: KeyKind::Pairwise,
            temporal_key: &[0x42; 16],
        };
        let queued = queue_ccmp_install_key(
            &mut queue,
            install,
            0x100000,
            1,
            0,
            &mut [0; 108],
            &mut [0; 128],
        )
        .unwrap();
        keys.record_queued(5, queued).unwrap();
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut keys, 9, 5, 7, sec),
            Some(KeyInstallAction::IgnoredBeforePublication)
        );
        keys.mark_published(5, KeyKind::Pairwise, queued.transfer)
            .unwrap();
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut keys, 9, 5, 7, sec),
            Some(KeyInstallAction::Installed {
                kind: KeyKind::Pairwise,
                all_required_installed: true
            })
        );
        assert_eq!(
            unmap_join_peer(&mut peer, &mut join, 9, 5, 7, HttPeerUnmap { peer_id: 3 }),
            JoinPeerUnmap::PeerRetained
        );
        assert_eq!(
            observe_mapped_security(&peer, &join, &mut keys, 9, 5, 7, sec),
            None
        );
    }
}
