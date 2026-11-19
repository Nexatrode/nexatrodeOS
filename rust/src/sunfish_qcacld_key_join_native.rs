//! Native composition of WPA2 key selection with the guarded QCACLD CE3
//! publisher. This module adds no hardware authority: the underlying native
//! publisher still validates WMI READY, endpoint ownership, credits, CE state,
//! sensitive DMA lifetime, and association tracker identity.
#![forbid(unsafe_code)]

use crate::ieee80211_join::Wpa2Join;
use crate::qcom_ce_v3::RingCursor;
use crate::qcom_ce_v3_native_io::{Gate, NativeIo};
use crate::qcom_ce_v3_profile::{Profile, RingPlan};
use crate::qcom_htc::HtcCreditLedger;
use crate::qcom_wmi_ce::{DmaPool, SensitiveTxBufferLedger, ServiceCeMapping, TxBufferWindow};
use crate::sunfish_qcacld_htt_event::Event;
use crate::sunfish_qcacld_key::{KeyInstallAction, QcacldCcmpKeyInstallTracker};
use crate::sunfish_qcacld_key_join::{
    KeyInstallCeTarget, KeyInstallPrepareError, KeySecurityDispatchError,
    plan_next_wpa2_key_install_ce_tx,
};
use crate::sunfish_qcacld_key_native::{self, KeyInstallPublishError};
use crate::sunfish_qcacld_wmi_rx::{SunfishWmiStartupSession, WmiStartupPhase};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyInstallOwnerError {
    StartupNotReady { phase: WmiStartupPhase },
    TransportGenerationMismatch { active: u64, startup: u64 },
    ControlEndpointNotBound,
    Prepare(KeyInstallPrepareError),
    Publish(KeyInstallPublishError),
}

/// Result of routing a CE1 completion through association security policy.
/// Ordinary RX indications remain borrowed and are returned to the owner for
/// packet delivery; only SEC_IND mutates key-install state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wpa2HttDispatch<'a> {
    RxIndication(crate::sunfish_qcacld_rx_ind::HttRxIndication<'a>),
    SecurityIndication(KeyInstallAction),
}

/// Dispatch one decoded CE1 completion to the active association. The caller
/// must drain old CE1 work before associating a new generation, because the
/// firmware SEC_IND itself does not carry an association generation.
pub fn dispatch_wpa2_event<'a>(
    join: &mut Wpa2Join<'_>,
    tracker: &mut QcacldCcmpKeyInstallTracker,
    event_generation: u32,
    event: Event<'a>,
) -> Result<Wpa2HttDispatch<'a>, KeySecurityDispatchError> {
    match event {
        Event::RxIndication(indication) => Ok(Wpa2HttDispatch::RxIndication(indication)),
        Event::SecurityIndication(indication) => {
            let action = crate::sunfish_qcacld_key_join::dispatch_wpa2_security_indication(
                join,
                tracker,
                event_generation,
                indication,
            )?;
            Ok(Wpa2HttDispatch::SecurityIndication(action))
        }
    }
}

/// Publish the next WPA2 pairwise key or optional GTK from the active join.
/// Returns `Ok(false)` after all required keys have already been queued.
/// WMI READY and its control endpoint are checked before key material is
/// borrowed or serialized; publication then goes through the shared guarded
/// publisher, which records tracker state only after CE3 commit.
#[allow(clippy::too_many_arguments)]
pub fn publish_next_wpa2_key_install<G: Gate, const N: usize>(
    io: &mut NativeIo<G>,
    join: &Wpa2Join<'_>,
    tracker: &mut QcacldCcmpKeyInstallTracker,
    active_generation: u64,
    max_message_size: usize,
    transfer_id: u16,
    mappings: &[ServiceCeMapping],
    dma_pool: DmaPool,
    buffer: TxBufferWindow,
    startup: &SunfishWmiStartupSession,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
    credits: &mut HtcCreditLedger,
    sensitive: &mut SensitiveTxBufferLedger<N>,
    wmi_body_output: &mut [u8],
    htc_frame_output: &mut [u8],
) -> Result<bool, KeyInstallOwnerError> {
    if startup.phase() != WmiStartupPhase::Ready {
        return Err(KeyInstallOwnerError::StartupNotReady {
            phase: startup.phase(),
        });
    }
    if startup.generation() != active_generation {
        return Err(KeyInstallOwnerError::TransportGenerationMismatch {
            active: active_generation,
            startup: startup.generation(),
        });
    }
    let endpoint_id = startup
        .control_endpoint_id()
        .ok_or(KeyInstallOwnerError::ControlEndpointNotBound)?;
    let target = KeyInstallCeTarget {
        generation: active_generation,
        endpoint_id,
        max_message_size,
        credit_size: credits.credit_size(),
        available_credits: credits.available(),
        mappings,
        transfer_id,
        dma_pool,
        buffer,
        profile,
        rings,
        cursor: *cursor,
    };
    let Some(plan) =
        plan_next_wpa2_key_install_ce_tx(join, tracker, target, wmi_body_output, htc_frame_output)
            .map_err(KeyInstallOwnerError::Prepare)?
    else {
        return Ok(false);
    };
    sunfish_qcacld_key_native::publish_qcacld_key_install(
        io,
        plan,
        active_generation,
        join.association_generation(),
        startup,
        profile,
        rings,
        cursor,
        credits,
        sensitive,
        tracker,
    )
    .map_err(KeyInstallOwnerError::Publish)?;
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wpa2HttServiceDispatchError<Owner> {
    Security(KeySecurityDispatchError),
    Owner(Owner),
}

/// Compose the native CE1 drain with WPA2 SEC_IND routing. Each completion is
/// correlated against the active join before the caller receives it; the
/// original completion remains available so the owner can account HTC credits
/// and synchronously deliver any borrowed RX indication. Since SEC_IND has no
/// association generation, the owner must drain old CE1 work before replacing
/// the join or key tracker.
#[allow(clippy::too_many_arguments)]
pub fn service_native_wpa2_htt_ce1_irq<G: Gate, Owner>(
    io: &mut NativeIo<G>,
    generation: u64,
    profile: Profile,
    rings: &RingPlan,
    cursor: &mut RingCursor,
    rx_ledger: &mut crate::qcom_wmi_ce::RxBufferLedger,
    endpoint: crate::sunfish_qcacld_htc::HttEndpoint,
    scratch: &mut [u8],
    max_completions: usize,
    join: &mut Wpa2Join<'_>,
    tracker: &mut QcacldCcmpKeyInstallTracker,
    association_generation: u32,
    mut owner: impl FnMut(
        &crate::sunfish_qcacld_htt_event::CompletedEvent<'_>,
        Wpa2HttDispatch<'_>,
    ) -> Result<(), Owner>,
) -> Result<
    crate::sunfish_qcacld_htt_event::HttCe1DrainReport,
    crate::sunfish_qcacld_htt_event_native::NativeHttCe1ServiceError<
        Wpa2HttServiceDispatchError<Owner>,
    >,
> {
    crate::sunfish_qcacld_htt_event_native::service_native_htt_ce1_irq(
        io,
        generation,
        profile,
        rings,
        cursor,
        rx_ledger,
        endpoint,
        scratch,
        max_completions,
        |completed| {
            let routed = dispatch_wpa2_event(
                join,
                tracker,
                association_generation,
                completed.decoded.event,
            )
            .map_err(Wpa2HttServiceDispatchError::Security)?;
            owner(completed, routed).map_err(Wpa2HttServiceDispatchError::Owner)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ieee80211_join::JoinConfig;
    use crate::sunfish_qcacld_key::{
        CommandLayout, KeyKind, WMI_VDEV_INSTALL_KEY_BODY_BYTES, WMI_VDEV_INSTALL_KEY_COMMAND_ID,
    };

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

    #[test]
    fn stale_ce1_sec_ind_is_routed_but_cannot_change_association_state() {
        let mut join = join();
        let mut tracker = QcacldCcmpKeyInstallTracker::new(9, 1, 0x42, BSSID, 0, None).unwrap();
        tracker
            .record_queued(
                9,
                CommandLayout {
                    command_id: WMI_VDEV_INSTALL_KEY_COMMAND_ID,
                    body_bytes: WMI_VDEV_INSTALL_KEY_BODY_BYTES,
                    vdev_id: 1,
                    peer_address: BSSID,
                    key_index: 0,
                    kind: KeyKind::Pairwise,
                },
            )
            .unwrap();
        let indication = crate::ath10k_htt::HttSecurityIndication {
            peer_id: 0x42,
            security_type: crate::ath10k_htt::HTT_SECURITY_TYPE_AES_CCMP,
            is_unicast: true,
        };

        assert_eq!(
            dispatch_wpa2_event(
                &mut join,
                &mut tracker,
                8,
                Event::SecurityIndication(indication),
            ),
            Ok(Wpa2HttDispatch::SecurityIndication(
                KeyInstallAction::IgnoredStaleGeneration
            ))
        );
        assert!(!tracker.all_required_installed());
        assert!(!join.is_authorized());
    }
}
