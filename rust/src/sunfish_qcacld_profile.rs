//! Source-derived WLAN_CFG profile for Sunfish's downstream QCACLD/ICNSS path.
//!
//! Values are transcribed from LineageOS kernel commit
//! `28f9290ae067d3e7857126eb6312720dbe9c7c80` and its pinned
//! `qca-wifi-host-cmn` commit `141c2acec4d77e80a7b6b2b872cdc440210f7349`:
//! `core/mac/inc/qwlan_version.h`, `hif/src/ce/ce_assignment.h`,
//! `hif/src/ce/ce_main.c`, and `fw-api/fw/htc_services.h`. The profile is
//! descriptive protocol data only; it does not authorize radio or DMA access.

use crate::wlfw_protocol::{
    CeServicePipeConfig, CeTargetPipeConfig, ShadowRegisterConfig, WlanConfig,
};

pub const HOST_VERSION: &str = "5.2.03.9H";
/// ICNSS's `ICNSS_QUIRKS_DEFAULT` enables FW rejuvenation.
pub const FW_REJUVENATE_ENABLED: bool = true;
/// `ICNSS_DEFAULT_FEATURE_MASK` in the pinned Lineage source.
pub const DEFAULT_DYNAMIC_FEATURE_MASK: u64 =
    crate::qcom_wlfw_client::ICNSS_DEFAULT_DYNAMIC_FEATURE_MASK;

const IN: u32 = 1;
const OUT: u32 = 2;
const INOUT: u32 = 3;
// htc_services.h defines PIPEDIR_INOUT_H2H as 4. QMI's generated enum lists
// only values 0..=3, but QCACLD forwards the CE7 value in its target table.
const INOUT_H2H: u32 = 4;
const DISABLE_INTERRUPT: u32 = 0x08;

pub static TARGET_PIPES: [CeTargetPipeConfig; 12] = [
    CeTargetPipeConfig {
        pipe_num: 0,
        pipe_dir: OUT,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 1,
        pipe_dir: IN,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 2,
        pipe_dir: IN,
        entries: 64,
        max_bytes: 2048,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 3,
        pipe_dir: OUT,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 4,
        pipe_dir: OUT,
        entries: 256,
        max_bytes: 256,
        flags: DISABLE_INTERRUPT,
    },
    CeTargetPipeConfig {
        pipe_num: 5,
        pipe_dir: OUT,
        entries: 1024,
        max_bytes: 64,
        flags: DISABLE_INTERRUPT,
    },
    CeTargetPipeConfig {
        pipe_num: 6,
        pipe_dir: INOUT,
        entries: 32,
        max_bytes: 16384,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 7,
        pipe_dir: INOUT_H2H,
        entries: 0,
        max_bytes: 0,
        flags: DISABLE_INTERRUPT,
    },
    CeTargetPipeConfig {
        pipe_num: 8,
        pipe_dir: IN,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 9,
        pipe_dir: IN,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 10,
        pipe_dir: IN,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    },
    CeTargetPipeConfig {
        pipe_num: 11,
        pipe_dir: IN,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    },
];

pub static SERVICE_PIPES: [CeServicePipeConfig; 20] = [
    CeServicePipeConfig {
        service_id: 0x0104,
        pipe_dir: OUT,
        pipe_num: 3,
    }, // WMI_DATA_VO
    CeServicePipeConfig {
        service_id: 0x0104,
        pipe_dir: IN,
        pipe_num: 2,
    },
    CeServicePipeConfig {
        service_id: 0x0102,
        pipe_dir: OUT,
        pipe_num: 3,
    }, // WMI_DATA_BK
    CeServicePipeConfig {
        service_id: 0x0102,
        pipe_dir: IN,
        pipe_num: 2,
    },
    CeServicePipeConfig {
        service_id: 0x0101,
        pipe_dir: OUT,
        pipe_num: 3,
    }, // WMI_DATA_BE
    CeServicePipeConfig {
        service_id: 0x0101,
        pipe_dir: IN,
        pipe_num: 2,
    },
    CeServicePipeConfig {
        service_id: 0x0103,
        pipe_dir: OUT,
        pipe_num: 3,
    }, // WMI_DATA_VI
    CeServicePipeConfig {
        service_id: 0x0103,
        pipe_dir: IN,
        pipe_num: 2,
    },
    CeServicePipeConfig {
        service_id: 0x0100,
        pipe_dir: OUT,
        pipe_num: 3,
    }, // WMI_CONTROL
    CeServicePipeConfig {
        service_id: 0x0100,
        pipe_dir: IN,
        pipe_num: 2,
    },
    CeServicePipeConfig {
        service_id: 0x0001,
        pipe_dir: OUT,
        pipe_num: 0,
    }, // HTC_CTRL_RSVD
    CeServicePipeConfig {
        service_id: 0x0001,
        pipe_dir: IN,
        pipe_num: 2,
    },
    CeServicePipeConfig {
        service_id: 0xfe00,
        pipe_dir: OUT,
        pipe_num: 0,
    }, // HTC_RAW_STREAMS
    CeServicePipeConfig {
        service_id: 0xfe00,
        pipe_dir: IN,
        pipe_num: 2,
    },
    CeServicePipeConfig {
        service_id: 0x0300,
        pipe_dir: OUT,
        pipe_num: 4,
    }, // HTT_DATA_MSG
    CeServicePipeConfig {
        service_id: 0x0300,
        pipe_dir: IN,
        pipe_num: 1,
    },
    CeServicePipeConfig {
        service_id: 0x0500,
        pipe_dir: OUT,
        pipe_num: 5,
    }, // WDI_IPA_TX
    CeServicePipeConfig {
        service_id: 0x0301,
        pipe_dir: IN,
        pipe_num: 9,
    }, // HTT_DATA2
    CeServicePipeConfig {
        service_id: 0x0302,
        pipe_dir: IN,
        pipe_num: 10,
    }, // HTT_DATA3
    CeServicePipeConfig {
        service_id: 0x0600,
        pipe_dir: IN,
        pipe_num: 11,
    }, // PACKET_LOG
];

pub static SHADOW_REGISTERS: [ShadowRegisterConfig; 12] = [
    ShadowRegisterConfig {
        id: 0,
        offset: 0x3c,
    },
    ShadowRegisterConfig {
        id: 3,
        offset: 0x3c,
    },
    ShadowRegisterConfig {
        id: 4,
        offset: 0x3c,
    },
    ShadowRegisterConfig {
        id: 5,
        offset: 0x3c,
    },
    ShadowRegisterConfig {
        id: 7,
        offset: 0x3c,
    },
    ShadowRegisterConfig {
        id: 1,
        offset: 0x40,
    },
    ShadowRegisterConfig {
        id: 2,
        offset: 0x40,
    },
    ShadowRegisterConfig {
        id: 7,
        offset: 0x40,
    },
    ShadowRegisterConfig {
        id: 8,
        offset: 0x40,
    },
    ShadowRegisterConfig {
        id: 9,
        offset: 0x40,
    },
    ShadowRegisterConfig {
        id: 10,
        offset: 0x40,
    },
    ShadowRegisterConfig {
        id: 11,
        offset: 0x40,
    },
];

/// Request shape passed by downstream ICNSS for its ordinary SNOC WLAN enable.
/// Shadow-reg-v2 is absent in that path; the older shadow table is present.
pub const fn wlan_config() -> WlanConfig<'static> {
    WlanConfig {
        host_version: Some(HOST_VERSION),
        target_pipes: &TARGET_PIPES,
        service_pipes: &SERVICE_PIPES,
        shadow_registers: &SHADOW_REGISTERS,
        shadow_registers_v2: None,
    }
}
