//! Offline WMI resource-config candidate for the pinned Sunfish QCACLD path.
//!
//! The field defaults and flag mapping are transcribed from LineageOS kernel
//! commit `28f9290ae067d3e7857126eb6312720dbe9c7c80`, chiefly
//! `core/wma/src/wma_main.c`, `core/mac/inc/wlan_tgt_def_config.h`,
//! `core/mac/inc/wma_internal.h`, and
//! `qca-wifi-host-cmn/wmi/src/wmi_unified_tlv.c`. The builder serializes data
//! only. It does not send WMI, authorize firmware, allocate DMA, or make the
//! profile suitable for a live target without comparison to a captured
//! service-ready event and the exact effective host/build configuration.

use crate::qcacld_wmi::{
    QcacldPeerLimit, QcacldWmaHostConfig, QcacldWmaPolicyError, SUNFISH_WMI_INIT_COMMAND_ID,
    ServiceAvailableView, ServiceReadyView, SunfishWmiStartupError, WmiInitEncodeError,
};
use crate::qcom_htc::{self, WmiTxPlan};

pub const RESOURCE_CONFIG_TAG: u16 = 75;
pub const RESOURCE_CONFIG_WORDS: usize = 68;
pub const RESOURCE_CONFIG_TLV_BYTES: usize = 4 + RESOURCE_CONFIG_WORDS * 4;
const RESOURCE_CONFIG_VALUE_BYTES: u16 = (RESOURCE_CONFIG_WORDS * 4) as u16;

const SERVICE_DEPRECATED_REPLACE: usize = 99;
const SERVICE_HTT_H2T_NO_HTC_HDR_LEN_IN_MSG_LEN: usize = 176;
const SERVICE_PEER_UNMAP_RESPONSE_SUPPORT: usize = 182;

const FLAG_MGMT_COMPLETION_BUNDLE: u32 = 1 << 9;
const FLAG_TX_MSDU_NEW_PARTITION: u32 = 1 << 10;
const FLAG_CCE_DISABLE: u32 = 1 << 12;
const FLAG_NEW_HTT_MESSAGE_FORMAT: u32 = 1 << 19;
const FLAG_PEER_UNMAP_RESPONSE: u32 = 1 << 20;

/// Build options that vary with the QCACLD compile configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishQcacldCompileOptions {
    pub high_latency_hif: bool,
    pub ip_header_alignment_workaround: bool,
    pub multicast_to_unicast: bool,
    pub twt_support: bool,
    pub num_msdu_descriptors: u32,
    pub twt_ap_pdev_count: u32,
    pub twt_ap_sta_count: u32,
}

impl SunfishQcacldCompileOptions {
    /// Values implied by the pinned Sunfish SNOC build and unoverridden
    /// `wlan_tgt_def_config.h` defaults. Keep these marked as a candidate until
    /// the loaded module's effective build/configuration is independently
    /// confirmed.
    pub const fn lineage_sunfish_candidate() -> Self {
        Self {
            high_latency_hif: false,
            ip_header_alignment_workaround: false,
            multicast_to_unicast: false,
            twt_support: false,
            num_msdu_descriptors: 1056,
            twt_ap_pdev_count: 0,
            twt_ap_sta_count: 0,
        }
    }
}

/// Explicit effective inputs used by `wma_set_default_tgt_config()` and its
/// later service-ready updates. No firmware service bit or runtime option is
/// guessed by this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishQcacldResourceInputs {
    pub host: QcacldWmaHostConfig,
    pub peer_limit: QcacldPeerLimit,
    pub compile: SunfishQcacldCompileOptions,
    pub monitor_mode: bool,
    pub enable_peer_unmap_confirmation: bool,
    /// Runtime-effective fields from `wma_update_fw_config()` or the selected
    /// target build. `None` leaves the source-default candidate values intact.
    pub effective_resources: Option<SunfishQcacldEffectiveResources>,
}

/// Values observed in LineageOS immediately before WMI INIT serialization.
/// Keep them explicit because chain masks, PMO limits, and build selection are
/// not derivable from generic Sunfish defaults alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishQcacldEffectiveResources {
    pub tx_chain_mask: u32,
    pub rx_chain_mask: u32,
    pub max_frag_entries: u32,
    pub num_wow_filters: u32,
    pub apf_instruction_size: u32,
    pub num_packet_filters: u32,
}

impl SunfishQcacldResourceInputs {
    /// Defaults visible in the pinned Sunfish Lineage INI/header sources.
    /// `peer_limit` remains explicit because WMA selects it from runtime
    /// target revision rather than the firmware build string.
    pub const fn lineage_sunfish_candidate(peer_limit: QcacldPeerLimit) -> Self {
        Self {
            host: QcacldWmaHostConfig::lineage_sunfish_defaults(),
            peer_limit,
            compile: SunfishQcacldCompileOptions::lineage_sunfish_candidate(),
            monitor_mode: false,
            enable_peer_unmap_confirmation: false,
            effective_resources: None,
        }
    }

    /// Observed effective settings from the 2026-10-24 Lineage WMI INIT
    /// capture. The SERVICE_READY bitmap is still parsed and supplied by the
    /// caller so the feature flags are not inferred from this profile.
    pub const fn observed_lineage_wmi_init_2026_10_24(peer_limit: QcacldPeerLimit) -> Self {
        Self {
            host: QcacldWmaHostConfig::lineage_sunfish_defaults(),
            peer_limit,
            compile: SunfishQcacldCompileOptions::lineage_sunfish_candidate(),
            monitor_mode: false,
            enable_peer_unmap_confirmation: true,
            effective_resources: Some(SunfishQcacldEffectiveResources {
                tx_chain_mask: 3,
                rx_chain_mask: 3,
                max_frag_entries: 2,
                num_wow_filters: 2,
                apf_instruction_size: 0,
                num_packet_filters: u32::MAX,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishQcacldResourceError {
    HostPolicy(QcacldWmaPolicyError),
    Startup(SunfishWmiStartupError),
    ServiceWordUnavailable { service_index: usize },
    TwtCountsRequireSupport,
    UnsupportedHighLatencyBuild,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishQcacldWmiFrameError {
    Resource(SunfishQcacldResourceError),
    InitEncode(WmiInitEncodeError),
    HtcEncode(qcom_htc::HtcError),
    ControlSessionNotBound,
    ControlEndpointMismatch { expected: u8, observed: u8 },
    StartupNotReady,
    StartupEvidenceUnavailable,
    WrongGeneration { expected: u64, observed: u64 },
}

/// Offline plan for one QCACLD WMI_INIT command inside its negotiated HTC
/// endpoint. It contains no CE descriptor and does not publish the frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishQcacldWmiFramePlan {
    pub resource: SunfishQcacldResourceCandidate,
    pub body_bytes: usize,
    pub htc: WmiTxPlan,
}

/// Build a source-derived candidate, encode QCACLD's WMI_INIT body, and frame
/// it using caller-supplied HTC endpoint/credit negotiation. The caller must
/// provide two independent bounded buffers. Success means bytes are encoded;
/// it does not authorize `plan_wmi_tx`, DMA, or hardware publication.
#[allow(clippy::too_many_arguments)]
pub fn encode_sunfish_qcacld_wmi_init_frame(
    service_ready: &ServiceReadyView<'_>,
    service_available: &ServiceAvailableView<'_>,
    inputs: SunfishQcacldResourceInputs,
    hardware_mode_index: Option<u32>,
    endpoint_id: u8,
    max_message_size: usize,
    credit_size: u16,
    available_credits: u16,
    wmi_body_output: &mut [u8],
    htc_frame_output: &mut [u8],
) -> Result<SunfishQcacldWmiFramePlan, SunfishQcacldWmiFrameError> {
    let resource = SunfishQcacldResourceCandidate::build(service_ready, service_available, inputs)
        .map_err(SunfishQcacldWmiFrameError::Resource)?;
    let startup = service_ready
        .require_sunfish_wmi_startup()
        .map_err(|error| {
            SunfishQcacldWmiFrameError::Resource(SunfishQcacldResourceError::Startup(error))
        })?;
    let resource_tlv = resource.encode_tlv();
    let body_bytes = startup
        .encode_init_body(&resource_tlv, hardware_mode_index, wmi_body_output)
        .map_err(SunfishQcacldWmiFrameError::InitEncode)?;
    let htc = qcom_htc::encode_wmi_command_frame(
        endpoint_id,
        max_message_size,
        credit_size,
        available_credits,
        SUNFISH_WMI_INIT_COMMAND_ID,
        &wmi_body_output[..body_bytes],
        htc_frame_output,
    )
    .map_err(SunfishQcacldWmiFrameError::HtcEncode)?;
    Ok(SunfishQcacldWmiFramePlan {
        resource,
        body_bytes,
        htc,
    })
}

/// A serialized QCACLD WMI resource-config TLV. This is protocol data only,
/// not a send-ready authorization token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishQcacldResourceCandidate {
    words: [u32; RESOURCE_CONFIG_WORDS],
}

impl SunfishQcacldResourceCandidate {
    pub fn build(
        service_ready: &ServiceReadyView<'_>,
        service_available: &ServiceAvailableView<'_>,
        inputs: SunfishQcacldResourceInputs,
    ) -> Result<Self, SunfishQcacldResourceError> {
        service_ready
            .require_sunfish_wmi_startup()
            .map_err(SunfishQcacldResourceError::Startup)?;

        // Sunfish uses the SNOC QCACLD path. The SDIO/USB high-latency
        // configuration has different vdev, WDS, AST, and TID defaults.
        if inputs.compile.high_latency_hif {
            return Err(SunfishQcacldResourceError::UnsupportedHighLatencyBuild);
        }
        if !inputs.compile.twt_support
            && (inputs.compile.twt_ap_pdev_count != 0 || inputs.compile.twt_ap_sta_count != 0)
        {
            return Err(SunfishQcacldResourceError::TwtCountsRequireSupport);
        }

        let counts = inputs
            .host
            .derive_resource_counts(inputs.peer_limit)
            .map_err(SunfishQcacldResourceError::HostPolicy)?;

        // These are raw bit positions in the pinned `wmi_services.h`. Base
        // services come from SERVICE_READY; IDs >= 128 come from the separate
        // SERVICE_AVAILABLE segment and fail closed if its offset is unknown.
        let base_service = |service_index| {
            service_ready
                .service_enabled(service_index)
                .ok_or(SunfishQcacldResourceError::ServiceWordUnavailable { service_index })
        };
        let extended_service = |service_index| {
            service_available
                .service_enabled(service_index)
                .ok_or(SunfishQcacldResourceError::ServiceWordUnavailable { service_index })
        };
        let use_pdev_id = base_service(SERVICE_DEPRECATED_REPLACE)?;
        let new_htt_message_format = extended_service(SERVICE_HTT_H2T_NO_HTC_HDR_LEN_IN_MSG_LEN)?;
        let peer_unmap_response = extended_service(SERVICE_PEER_UNMAP_RESPONSE_SUPPORT)?
            && inputs.enable_peer_unmap_confirmation;

        let mut words = [0u32; RESOURCE_CONFIG_WORDS];
        words[0] = u32::from(counts.num_vdevs);
        words[1] = u32::from(counts.num_peers);
        words[2] = u32::from(counts.num_offload_peers);
        words[3] = u32::from(counts.num_offload_reorder_buffers);
        words[4] = 2; // CFG_TGT_NUM_PEER_KEYS
        words[5] = u32::from(counts.num_tids);
        words[6] = 16; // CFG_TGT_AST_SKID_LIMIT
        words[7] = 0x7; // CFG_TGT_DEFAULT_TX_CHAIN_MASK
        words[8] = 0x7; // CFG_TGT_DEFAULT_RX_CHAIN_MASK
        words[9] = 100;
        words[10] = 100;
        words[11] = 100;
        words[12] = 40;
        words[13] = if inputs.monitor_mode {
            0 // CFG_TGT_RX_DECAP_MODE_RAW
        } else if inputs.compile.ip_header_alignment_workaround {
            1 // CFG_TGT_RX_DECAP_MODE_NWIFI
        } else {
            2 // CFG_TGT_RX_DECAP_MODE
        };
        words[14] = u32::from(counts.scan_max_pending_req);
        words[15] = 3; // CFG_TGT_DEFAULT_BMISS_OFFLOAD_MAX_VDEV
        words[16] = 3; // CFG_TGT_DEFAULT_ROAM_OFFLOAD_MAX_VDEV
        words[17] = 8; // CFG_TGT_DEFAULT_ROAM_OFFLOAD_MAX_PROFILES
        if inputs.compile.multicast_to_unicast {
            words[18] = 4; // CFG_TGT_DEFAULT_NUM_MCAST_GROUPS
            words[19] = 16; // CFG_TGT_DEFAULT_NUM_MCAST_TABLE_ELEMS
            words[20] = 2; // CFG_TGT_DEFAULT_MCAST2UCAST_MODE
        }
        words[21] = 1024; // CFG_TGT_DEFAULT_TX_DBG_LOG_SIZE
        words[22] = 0; // CFG_TGT_WDS_ENTRIES
        words[23] = 0; // CFG_TGT_DEFAULT_DMA_BURST_SIZE
        words[24] = 0; // CFG_TGT_DEFAULT_MAC_AGGR_DELIM
        words[25] = 0; // CFG_TGT_DEFAULT_RX_SKIP_DEFRAG_TIMEOUT_DUP_DETECTION_CHECK
        words[26] = 0; // CFG_TGT_DEFAULT_VOW_CONFIG
        words[27] = 3; // CFG_TGT_DEFAULT_GTK_OFFLOAD_MAX_VDEV
        words[28] = inputs.compile.num_msdu_descriptors;
        words[29] = 10; // CFG_TGT_MAX_FRAG_TABLE_ENTRIES
        words[30] = 1; // CFG_TGT_NUM_TDLS_VDEVS
        words[31] = 8; // CFG_TGT_NUM_TDLS_CONN_TABLE_ENTRIES
        words[32] = 3; // CFG_TGT_DEFAULT_BEACON_TX_OFFLOAD_MAX_VDEV
        words[33] = 32; // CFG_TGT_MAX_MULTICAST_FILTER_ENTRIES
        words[34] = 0; // num_wow_filters
        words[35] = 6; // WMA_MAXNUM_PERIODIC_TX_PTRNS (set after defaults)
        words[36] = 0; // keep_alive_pattern_size
        words[37] = 1; // CFG_TGT_NUM_TDLS_CONC_SLEEP_STAS
        words[38] = 1; // CFG_TGT_NUM_TDLS_CONC_BUFFER_STAS
        words[39] = 0; // wmi_send_separate
        words[40] = 1; // CFG_TGT_NUM_OCB_VDEVS
        words[41] = 2; // CFG_TGT_NUM_OCB_CHANNELS
        words[42] = 2; // CFG_TGT_NUM_OCB_SCHEDULES

        let mut flag1 = FLAG_MGMT_COMPLETION_BUNDLE | FLAG_TX_MSDU_NEW_PARTITION;
        if new_htt_message_format {
            flag1 |= FLAG_NEW_HTT_MESSAGE_FORMAT;
        }
        if peer_unmap_response {
            flag1 |= FLAG_PEER_UNMAP_RESPONSE;
        }
        // CCE-disable and ATF are zero in the qdf_mem_zero()'d target config.
        debug_assert_eq!(FLAG_CCE_DISABLE & flag1, 0);
        words[43] = flag1;
        words[53] = u32::from(use_pdev_id);
        words[54] = 7; // CDS_DBS_SCAN_CLIENTS_MAX
        if inputs.compile.twt_support {
            words[58] = inputs.compile.twt_ap_pdev_count;
            words[59] = inputs.compile.twt_ap_sta_count;
        }
        if let Some(effective) = inputs.effective_resources {
            words[7] = effective.tx_chain_mask;
            words[8] = effective.rx_chain_mask;
            words[29] = effective.max_frag_entries;
            words[34] = effective.num_wow_filters;
            words[51] = effective.apf_instruction_size;
            words[61] = effective.num_packet_filters;
        }

        // WMI buffers are cleared by wmi_buf_alloc(); unassigned resource
        // fields therefore remain zero, matching the pinned host path.
        Ok(Self { words })
    }

    pub const fn words(&self) -> &[u32; RESOURCE_CONFIG_WORDS] {
        &self.words
    }

    pub fn encode_tlv(&self) -> [u8; RESOURCE_CONFIG_TLV_BYTES] {
        let mut tlv = [0u8; RESOURCE_CONFIG_TLV_BYTES];
        let header =
            (u32::from(RESOURCE_CONFIG_TAG) << 16) | u32::from(RESOURCE_CONFIG_VALUE_BYTES);
        tlv[..4].copy_from_slice(&header.to_le_bytes());
        for (index, word) in self.words.iter().enumerate() {
            let start = 4 + index * 4;
            tlv[start..start + 4].copy_from_slice(&word.to_le_bytes());
        }
        tlv
    }
}
