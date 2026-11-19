//! Bind WPA2 association key material and CE1 security events to QCACLD key
//! installation state. This is protocol glue only; it has no DMA or hardware
//! authority.
#![forbid(unsafe_code)]

use crate::ath10k_htt::HttSecurityIndication;
use crate::ieee80211_join::{JoinError, Wpa2Join};
use crate::ieee80211_supplicant::KeyInstallView;
use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_wmi_ce::{DmaPool, ServiceCeMapping, TxBufferWindow};
use crate::sunfish_qcacld_key::{
    self, CcmpInstallKey, KeyInstallAction, KeyKind, QcacldCcmpKeyInstallTracker, TrackerError,
};

const PAIRWISE_RSC_ZERO: [u8; 8] = [0; 8];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyInstallPrepareError {
    Join(JoinError),
    Tracker(TrackerError),
    Plan(sunfish_qcacld_key::Error),
    AssociationMismatch,
    PeerMismatch,
    PairwiseKeyIndexMismatch {
        tracker: u8,
        wpa2: u8,
    },
    GroupKeyIndexMismatch {
        tracker: Option<u8>,
        wpa2: Option<u8>,
    },
}

/// Fixed CE3/HTC inputs for one association-owned key command. The endpoint,
/// credit values, ring cursor, and memory windows are snapshots; the native
/// publisher revalidates mutable ownership and reserves the actual credit and
/// sensitive-buffer ledgers before committing the descriptor.
pub struct KeyInstallCeTarget<'a> {
    pub generation: u64,
    pub endpoint_id: u8,
    pub max_message_size: usize,
    pub credit_size: u16,
    pub available_credits: u16,
    pub mappings: &'a [ServiceCeMapping],
    pub transfer_id: u16,
    pub dma_pool: DmaPool,
    pub buffer: TxBufferWindow,
    pub profile: Profile,
    pub rings: &'a RingPlan,
    pub cursor: RingCursor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeySecurityDispatchError {
    Join(JoinError),
}

/// Select the next key owned by an active WPA2 join and bind it to the
/// association's QCACLD tracker. Pairwise is emitted first; an optional GTK
/// follows with the message-3 RSC. No key bytes are copied by this helper.
pub fn next_wpa2_key_install_request<'a>(
    join: &'a Wpa2Join<'_>,
    tracker: &QcacldCcmpKeyInstallTracker,
) -> Result<Option<CcmpInstallKey<'a>>, KeyInstallPrepareError> {
    let view = join
        .key_install_view()
        .map_err(KeyInstallPrepareError::Join)?;
    next_wpa2_key_install_request_from_view(
        view,
        join.association_generation(),
        join.bssid(),
        tracker,
    )
}

/// Key-view variant for owners that already borrowed keys from the active
/// supplicant. It is also the deterministic selection boundary used by
/// tests without modeling CE, DMA, or hardware state.
pub fn next_wpa2_key_install_request_from_view<'a>(
    view: KeyInstallView<'a>,
    generation: u32,
    peer_address: [u8; 6],
    tracker: &QcacldCcmpKeyInstallTracker,
) -> Result<Option<CcmpInstallKey<'a>>, KeyInstallPrepareError> {
    if generation != tracker.generation() {
        return Err(KeyInstallPrepareError::AssociationMismatch);
    }
    if peer_address != tracker.peer_address() {
        return Err(KeyInstallPrepareError::PeerMismatch);
    }
    let Some(kind) = tracker
        .next_unqueued_kind(generation)
        .map_err(KeyInstallPrepareError::Tracker)?
    else {
        return Ok(None);
    };
    match kind {
        KeyKind::Pairwise => {
            const WPA2_PAIRWISE_KEY_INDEX: u8 = 0;
            if tracker.pairwise_key_index() != WPA2_PAIRWISE_KEY_INDEX {
                return Err(KeyInstallPrepareError::PairwiseKeyIndexMismatch {
                    tracker: tracker.pairwise_key_index(),
                    wpa2: WPA2_PAIRWISE_KEY_INDEX,
                });
            }
            Ok(Some(CcmpInstallKey {
                vdev_id: tracker.vdev_id(),
                peer_address,
                key_index: WPA2_PAIRWISE_KEY_INDEX,
                kind,
                temporal_key: view.pairwise_temporal_key(),
                group_receive_sequence_counter: &PAIRWISE_RSC_ZERO,
            }))
        }
        KeyKind::Group => {
            let group = view.group_key();
            let wpa2_index = group.map(|key| key.key_id);
            if tracker.group_key_index() != wpa2_index {
                return Err(KeyInstallPrepareError::GroupKeyIndexMismatch {
                    tracker: tracker.group_key_index(),
                    wpa2: wpa2_index,
                });
            }
            let Some(group) = group else {
                return Err(KeyInstallPrepareError::GroupKeyIndexMismatch {
                    tracker: tracker.group_key_index(),
                    wpa2: None,
                });
            };
            Ok(Some(CcmpInstallKey {
                vdev_id: tracker.vdev_id(),
                peer_address,
                key_index: group.key_id,
                kind,
                temporal_key: group.temporal_key,
                group_receive_sequence_counter: view.receive_sequence_counter(),
            }))
        }
    }
}

/// Build the next CE3 key plan directly from the active WPA2 association.
/// Pairwise-before-group ordering and association identity are checked before
/// the key is serialized. The planner erases the WMI body after framing; the
/// returned native plan owns the only staging frame copy and must be passed
/// immediately to `publish_qcacld_key_install`, which scrubs it on every path.
pub fn plan_next_wpa2_key_install_ce_tx<'a>(
    join: &Wpa2Join<'_>,
    tracker: &QcacldCcmpKeyInstallTracker,
    target: KeyInstallCeTarget<'a>,
    wmi_body_output: &mut [u8],
    htc_frame_output: &'a mut [u8],
) -> Result<Option<sunfish_qcacld_key::QcacldKeyInstallCePlan<'a>>, KeyInstallPrepareError> {
    let Some(request) = next_wpa2_key_install_request(join, tracker)? else {
        return Ok(None);
    };
    sunfish_qcacld_key::plan_qcacld_key_install_ce_tx(
        target.generation,
        request,
        target.endpoint_id,
        target.max_message_size,
        target.credit_size,
        target.available_credits,
        target.mappings,
        target.transfer_id,
        target.dma_pool,
        target.buffer,
        target.profile,
        target.rings,
        target.cursor,
        wmi_body_output,
        htc_frame_output,
    )
    .map(Some)
    .map_err(KeyInstallPrepareError::Plan)
}

/// Route one CE1 `SEC_IND` through the association tracker. The controlled
/// port is confirmed only when this indication makes every queued key
/// mandatory for the association installed. Since `SEC_IND` carries no
/// association generation, the owner must drain prior-association CE1 events
/// before dispatching an indication with the current generation.
pub fn dispatch_wpa2_security_indication(
    join: &mut Wpa2Join<'_>,
    tracker: &mut QcacldCcmpKeyInstallTracker,
    event_generation: u32,
    indication: HttSecurityIndication,
) -> Result<KeyInstallAction, KeySecurityDispatchError> {
    let action = tracker.observe_security_indication(event_generation, indication);
    if matches!(
        action,
        KeyInstallAction::Installed {
            all_required_installed: true,
            ..
        }
    ) {
        join.confirm_keys_installed(tracker)
            .map_err(KeySecurityDispatchError::Join)?;
    }
    Ok(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ieee80211_join::{JoinConfig, JoinPhase};
    use crate::qcom_ce_v3::RingCursor;
    use crate::qcom_ce_v3_profile::{Profile, plan_rings};
    use crate::qcom_wmi_ce::{Direction, DmaPool, ServiceCeMapping, TxBufferWindow};
    use crate::sunfish_qcacld_key::{CommandLayout, KeyKind, WMI_VDEV_INSTALL_KEY_BODY_BYTES};

    const STATION: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
    const BSSID: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
    const RSN_IE: [u8; 22] = [
        48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];

    fn join() -> Wpa2Join<'static> {
        Wpa2Join::new(JoinConfig {
            association_generation: 9,
            station: STATION,
            bssid: BSSID,
            initial_sequence_number: 0,
            ssid: b"test-network",
            passphrase: b"correct horse battery",
            rsn_ie: &RSN_IE,
            supported_rates: &[0x82, 0x84, 0x8b, 0x96],
            extended_supported_rates: &[],
            additional_ies: &[],
            capability_info: 0x0011,
            listen_interval: 10,
            snonce: [0x33; 32],
        })
        .unwrap()
    }

    fn queued_pairwise_tracker(generation: u32) -> QcacldCcmpKeyInstallTracker {
        let mut tracker =
            QcacldCcmpKeyInstallTracker::new(generation, 1, 0x42, BSSID, 0, None).unwrap();
        tracker
            .record_queued(
                generation,
                CommandLayout {
                    command_id: crate::sunfish_qcacld_key::WMI_VDEV_INSTALL_KEY_COMMAND_ID,
                    body_bytes: WMI_VDEV_INSTALL_KEY_BODY_BYTES,
                    vdev_id: 1,
                    peer_address: BSSID,
                    key_index: 0,
                    kind: KeyKind::Pairwise,
                },
            )
            .unwrap();
        tracker
    }

    fn pairwise_indication() -> HttSecurityIndication {
        HttSecurityIndication {
            peer_id: 0x42,
            security_type: crate::ath10k_htt::HTT_SECURITY_TYPE_AES_CCMP,
            is_unicast: true,
        }
    }

    #[test]
    fn stale_security_event_cannot_confirm_or_open_a_join() {
        let mut join = join();
        let mut tracker = queued_pairwise_tracker(9);
        assert_eq!(
            dispatch_wpa2_security_indication(&mut join, &mut tracker, 8, pairwise_indication()),
            Ok(KeyInstallAction::IgnoredStaleGeneration)
        );
        assert!(!tracker.all_required_installed());
        assert!(!join.is_authorized());
    }

    #[test]
    fn completed_key_event_during_wrong_join_phase_fails_closed() {
        let mut join = join();
        let mut tracker = queued_pairwise_tracker(9);
        assert_eq!(
            dispatch_wpa2_security_indication(&mut join, &mut tracker, 9, pairwise_indication()),
            Err(KeySecurityDispatchError::Join(JoinError::UnexpectedPhase))
        );
        assert!(tracker.all_required_installed());
        assert!(!join.is_authorized());
        assert_eq!(join.phase(), JoinPhase::Ready);
    }

    #[test]
    fn ce_key_plan_requires_keys_pending_before_serializing_material() {
        let join = join();
        let tracker = QcacldCcmpKeyInstallTracker::new(9, 1, 0x42, BSSID, 0, None).unwrap();
        let mappings = [ServiceCeMapping {
            service_id: 1,
            direction: Direction::HostToTarget,
            engine: 3,
        }];
        let rings = plan_rings(Profile::Mission, 0x8000_0000, 0x4_0000).unwrap();
        let target = KeyInstallCeTarget {
            generation: 1,
            endpoint_id: 7,
            max_message_size: 512,
            credit_size: 256,
            available_credits: 8,
            mappings: &mappings,
            transfer_id: 3,
            dma_pool: DmaPool::new(0x8000_0000, 0x4_0000).unwrap(),
            buffer: TxBufferWindow {
                iova: 0x8000_1000,
                capacity: 512,
            },
            profile: Profile::Mission,
            rings: &rings,
            cursor: RingCursor::new(32).unwrap(),
        };
        let mut body = [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES];
        let mut frame = [0x5a; 512];

        assert!(matches!(
            plan_next_wpa2_key_install_ce_tx(&join, &tracker, target, &mut body, &mut frame,),
            Err(KeyInstallPrepareError::Join(JoinError::UnexpectedPhase))
        ));
        assert_eq!(body, [0xa5; WMI_VDEV_INSTALL_KEY_BODY_BYTES]);
        assert_eq!(frame, [0x5a; 512]);
        assert!(!tracker.publication_uncertain());
    }
}
