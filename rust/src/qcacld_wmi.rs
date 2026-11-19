//! Borrowed framing for the QCACLD WMI service-ready memory-request TLVs.
//!
//! This wire layout is based on the public LineageOS `sunfish` kernel snapshot
//! at `28f9290ae067`. It is deliberately separate from `ath10k_wmi`: both use
//! WMI TLVs, but their service-ready fixed structures are not interchangeable.
//! This module only validates and exposes firmware-provided bytes. It never
//! allocates DMA memory, trusts a requested size, or touches hardware.

const TLV_HEADER_SIZE: usize = 4;
const TAG_ARRAY_UINT32: u16 = 16;
const TAG_ARRAY_STRUCT: u16 = 18;
const TAG_ARRAY_FIXED_STRUCT: u16 = 19;
const TAG_SERVICE_READY: u16 = 32;
const TAG_HAL_REG_CAPABILITIES: u16 = 33;
const TAG_HOST_MEM_REQ: u16 = 34;
const TAG_READY_EVENT: u16 = 35;
const TAG_SERVICE_AVAILABLE: u16 = 559;

// `wmi_service_ready_event_fixed_param` is 33 u32 words (132 bytes),
// including its four-byte TLV header. `num_mem_reqs` is word 19, so within the
// TLV value (which excludes its header) the field begins at byte 72.
const SERVICE_READY_VALUE_SIZE: usize = 128;
const NUM_MEM_REQS_OFFSET: usize = 72;
const NUM_RF_CHAINS_OFFSET: usize = 36;
const MAX_FRAGMENT_ENTRIES_OFFSET: usize = 32;
const MAX_SCAN_CHANNELS_OFFSET: usize = 76;
const MAX_SUPPORTED_MACS_OFFSET: usize = 104;
const NUM_DBS_HW_MODES_OFFSET: usize = 112;
const TX_RX_CHAINMASK_OFFSET: usize = 116;
const HAL_REG_CAPABILITIES_VALUE_SIZE: usize = 36;
// WMI_SERVICE_BM_SIZE in the pinned wmi_services.h evaluates to 32 words.
const SERVICE_BITMAP_SIZE: usize = 32 * 4;
const SERVICE_MAX_BASE_ID: usize = 128;
const SERVICE_BASE_INDEX_STRIDE: usize = core::mem::size_of::<u32>();
const SERVICE_AVAILABLE_WORDS: usize = 4;
const SERVICE_AVAILABLE_BITMAP_SIZE: usize = SERVICE_AVAILABLE_WORDS * 4;
const SERVICE_AVAILABLE_VALUE_SIZE: usize = 4 + SERVICE_AVAILABLE_BITMAP_SIZE;
const SERVICE_AVAILABLE_SEGMENT_OFFSET: u32 = 128;
const MAX_MEM_REQS: u32 = 16;
const HOST_MEM_REQ_VALUE_SIZE: usize = 16;
pub const SUNFISH_WCN3998_FIRMWARE_BUILD: u32 = 0x3226_822e;
pub const SUNFISH_WMI_INIT_COMMAND_ID: u32 = 1;

// The pinned Sunfish QCACLD WMI headers define these TLV tags and ABI
// version. `wmi_init_cmd_send_tlv()` sends these as little-endian WMITLV
// headers; the command ID itself is carried separately by WMI transport.
const TAG_INIT_FIXED: u16 = 74;
const TAG_RESOURCE_CONFIG: u16 = 75;
const TAG_HW_MODE_FIXED: u16 = 515;
const SUNFISH_WMI_HOST_ABI: [u32; 6] = [0x0100_0000, 762, 0x5f41_4351, 0x0000_4c4d, 0, 0];
const WMI_INIT_FIXED_SIZE: usize = 32;
const WMI_RESOURCE_CONFIG_SIZE: usize = 276;
const WMI_RESOURCE_CONFIG_VALUE_SIZE: u16 = 272;
const WMI_RESOURCE_CONFIG_WORDS: usize = 68;
const WMI_INIT_EMPTY_MEM_ARRAY_SIZE: usize = 4;
const WMI_INIT_HW_MODE_SIZE: usize = 20;
const WMI_READY_EVENT_VALUE_SIZE: usize = 60;
const WMI_READY_STATUS_OFFSET: usize = 32;
const WMI_READY_MAC_OFFSET: usize = 24;
const WMI_READY_EXTRA_MAC_COUNT_OFFSET: usize = 40;
const WMI_MAC_ADDRESS_SIZE: usize = 8;
const MAX_READY_EXTRA_MACS: u32 = 2; // WMI_HOST_MAX_PDEV (3) minus the primary MAC

const QCACLD_NUM_VDEVS: u8 = 4;
const QCACLD_MIN_MAX_STATION: u8 = 1;
const QCACLD_MAX_MAX_STATION: u8 = 32;
const QCACLD_MIN_OFFLOAD_PEERS: u8 = 2;
const QCACLD_MAX_OFFLOAD_PEERS: u8 = 5;
const QCACLD_MAX_OFFLOAD_REORDER_BUFFERS: u8 = 3;
const QCACLD_MIN_SCAN_COUNT: u8 = 1;
const QCACLD_MAX_SCAN_COUNT: u8 = 8;

/// Target-specific peer cap used by the pinned Lineage QCACLD WMA path.
/// The caller must identify this from trusted target information; this module
/// does not infer it from the firmware build string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QcacldPeerLimit {
    Qca6174Revision1_1,
    OtherTarget32,
}

/// Host settings consumed by `wma_set_default_tgt_config()`. The Lineage
/// Sunfish INI hash-matches the public device-tree config, which omits these
/// four keys, so `lineage_sunfish_defaults()` reflects the pinned driver's
/// defaults. Callers may supply observed effective settings instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QcacldWmaHostConfig {
    pub max_station: u8,
    pub ap_max_offload_peers: u8,
    pub ap_max_offload_reorder_buffers: u8,
    pub max_scan: u8,
}

impl QcacldWmaHostConfig {
    pub const fn lineage_sunfish_defaults() -> Self {
        Self {
            max_station: 32,
            ap_max_offload_peers: 2,
            ap_max_offload_reorder_buffers: 2,
            max_scan: 4,
        }
    }

    /// Derive only the fields that pinned WMA recomputes from target and host
    /// settings. This does not populate a WMI resource-config TLV or validate
    /// the remaining resource words against firmware capabilities.
    pub fn derive_resource_counts(
        self,
        peer_limit: QcacldPeerLimit,
    ) -> Result<QcacldWmaResourceCounts, QcacldWmaPolicyError> {
        if !(QCACLD_MIN_MAX_STATION..=QCACLD_MAX_MAX_STATION).contains(&self.max_station) {
            return Err(QcacldWmaPolicyError::MaxStation);
        }
        if !(QCACLD_MIN_OFFLOAD_PEERS..=QCACLD_MAX_OFFLOAD_PEERS)
            .contains(&self.ap_max_offload_peers)
        {
            return Err(QcacldWmaPolicyError::OffloadPeers);
        }
        if self.ap_max_offload_reorder_buffers > QCACLD_MAX_OFFLOAD_REORDER_BUFFERS {
            return Err(QcacldWmaPolicyError::OffloadReorderBuffers);
        }
        if !(QCACLD_MIN_SCAN_COUNT..=QCACLD_MAX_SCAN_COUNT).contains(&self.max_scan) {
            return Err(QcacldWmaPolicyError::MaxScan);
        }

        let target_peer_cap = match peer_limit {
            QcacldPeerLimit::Qca6174Revision1_1 => 14,
            QcacldPeerLimit::OtherTarget32 => 32,
        };
        let supported_peers = self.max_station.min(target_peer_cap);
        let peers_plus_vdevs = u16::from(supported_peers) + u16::from(QCACLD_NUM_VDEVS) + 2;
        Ok(QcacldWmaResourceCounts {
            num_vdevs: QCACLD_NUM_VDEVS,
            supported_peers,
            num_peers: peers_plus_vdevs,
            num_offload_peers: self.ap_max_offload_peers + 1,
            num_offload_reorder_buffers: self.ap_max_offload_reorder_buffers + 1,
            num_tids: peers_plus_vdevs * 2,
            scan_max_pending_req: self.max_scan,
        })
    }
}

/// Partial candidate values for QCACLD's WMI resource-config TLV.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QcacldWmaResourceCounts {
    pub num_vdevs: u8,
    pub supported_peers: u8,
    pub num_peers: u16,
    pub num_offload_peers: u8,
    pub num_offload_reorder_buffers: u8,
    pub num_tids: u16,
    pub scan_max_pending_req: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QcacldWmaPolicyError {
    MaxStation,
    OffloadPeers,
    OffloadReorderBuffers,
    MaxScan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlvError {
    HeaderTruncated,
    ValueTruncated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TlvRecord<'a> {
    pub tag: u16,
    pub value: &'a [u8],
}

pub struct TlvIter<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceReadyError {
    Tlv(TlvError),
    ServiceReadyMissing,
    ServiceReadyDuplicate,
    ServiceReadyWrongLength,
    HalCapabilitiesMissing,
    HalCapabilitiesDuplicate,
    HalCapabilitiesWrongLength,
    ServiceBitmapMissing,
    ServiceBitmapMalformed,
    MemoryRequestsMissing,
    MemoryRequestsDuplicate,
    MemoryRequestTagUnexpected,
    MemoryRequestWrongLength,
    TooManyMemoryRequests,
    MemoryRequestCountMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceAvailableError {
    Tlv(TlvError),
    FixedParameterMissing,
    FixedParameterDuplicate,
    FixedParameterWrongLength,
    UnexpectedSegmentOffset { observed: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmptyMemoryRequestError {
    FirmwareBuildMismatch { expected: u32, observed: u32 },
    RequestsPresent { count: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishWmiStartupError {
    MemoryRequests(EmptyMemoryRequestError),
    AbiMismatch {
        word_index: usize,
        expected: u32,
        observed: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiInitEncodeError {
    ResourceConfigWrongSize { expected: usize, observed: usize },
    ResourceConfigWrongTag { observed: u16 },
    ResourceConfigWrongLength { expected: u16, observed: u16 },
    UnsupportedHardwareMode { observed: u32 },
    OutputTooSmall { required: usize, available: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmiReadyError {
    Tlv(TlvError),
    FixedParameterMissing,
    FixedParameterDuplicate,
    FixedParameterWrongLength,
    MacListDuplicate,
    MacListMissing {
        count: u32,
    },
    MacListWrongLength {
        expected: usize,
        observed: usize,
    },
    TooManyMacAddresses {
        count: u32,
    },
    AbiMismatch {
        word_index: usize,
        expected: u32,
        observed: u32,
    },
    InitFailed {
        status: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiReadyEvent<'a> {
    /// The fixed-parameter MAC as its two little-endian WMI words.
    pub mac_address_words: [u32; 2],
    /// Borrowed packed list of additional eight-byte WMI MAC records.
    pub extra_mac_addresses: &'a [u8],
    pub total_peers: u32,
    pub extra_peers: u32,
}

/// Structurally validated QCACLD resource-config TLV. Values remain policy
/// inputs; this view only decodes the pinned wire fields and does not certify
/// that a particular resource profile is suitable for Sunfish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiResourceConfig<'a> {
    tlv: &'a [u8],
}

impl<'a> WmiResourceConfig<'a> {
    pub fn parse(tlv: &'a [u8]) -> Result<Self, WmiInitEncodeError> {
        if tlv.len() != WMI_RESOURCE_CONFIG_SIZE {
            return Err(WmiInitEncodeError::ResourceConfigWrongSize {
                expected: WMI_RESOURCE_CONFIG_SIZE,
                observed: tlv.len(),
            });
        }
        let header = read_u32_at(tlv, 0);
        let observed_tag = (header >> 16) as u16;
        if observed_tag != TAG_RESOURCE_CONFIG {
            return Err(WmiInitEncodeError::ResourceConfigWrongTag {
                observed: observed_tag,
            });
        }
        let observed_len = (header & 0xffff) as u16;
        if observed_len != WMI_RESOURCE_CONFIG_VALUE_SIZE {
            return Err(WmiInitEncodeError::ResourceConfigWrongLength {
                expected: WMI_RESOURCE_CONFIG_VALUE_SIZE,
                observed: observed_len,
            });
        }
        debug_assert_eq!(tlv.len(), TLV_HEADER_SIZE + WMI_RESOURCE_CONFIG_WORDS * 4);
        Ok(Self { tlv })
    }

    fn word(&self, index: usize) -> u32 {
        debug_assert!(index < WMI_RESOURCE_CONFIG_WORDS);
        read_u32_at(self.tlv, TLV_HEADER_SIZE + index * 4)
    }

    pub const fn as_tlv(self) -> &'a [u8] {
        self.tlv
    }

    pub fn num_vdevs(&self) -> u32 {
        self.word(0)
    }

    pub fn num_peers(&self) -> u32 {
        self.word(1)
    }

    pub fn num_offload_peers(&self) -> u32 {
        self.word(2)
    }

    pub fn num_offload_reorder_buffers(&self) -> u32 {
        self.word(3)
    }

    pub fn num_peer_keys(&self) -> u32 {
        self.word(4)
    }

    pub fn num_tids(&self) -> u32 {
        self.word(5)
    }

    pub fn tx_chain_mask(&self) -> u32 {
        self.word(7)
    }

    pub fn rx_chain_mask(&self) -> u32 {
        self.word(8)
    }

    pub fn num_msdu_descriptors(&self) -> u32 {
        self.word(28)
    }

    pub fn max_fragment_entries(&self) -> u32 {
        self.word(29)
    }

    pub fn feature_flags(&self) -> u32 {
        self.word(43)
    }

    pub fn num_packet_filters(&self) -> u32 {
        self.word(61)
    }

    pub fn num_max_sta_vdevs(&self) -> u32 {
        self.word(62)
    }
}

/// Evidence that one exact firmware build requested no dynamic WMI memory.
/// This carries no DMA authority and is valid only for the parsed event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmptyMemoryRequestSet<'a> {
    firmware_build_version: u32,
    _event: &'a [u8],
}

/// A source-bound authorization to format a no-chunk WMI INIT body.
///
/// Construction requires the exact captured Sunfish firmware build, no
/// dynamic WMI memory requests, and a compatible QCACLD WMI ABI. This is only
/// protocol evidence: it does not authorize sending WMI, DMA, or MMIO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishWmiStartup<'a> {
    _memory_requests: EmptyMemoryRequestSet<'a>,
}

impl EmptyMemoryRequestSet<'_> {
    pub const fn firmware_build_version(self) -> u32 {
        self.firmware_build_version
    }
}

/// A borrowed service-ready view following the QCACLD WMITLV event schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceReadyView<'a> {
    pub fixed_event: &'a [u8],
    pub hal_reg_capabilities: &'a [u8],
    pub service_bitmap: &'a [u8],
    memory_requests: &'a [u8],
    pub declared_memory_request_count: u32,
}

/// Owned, compact copy of the accepted Sunfish SERVICE_READY fields needed
/// after the CE receive scratch buffer is reused. It can only be created from
/// a view that passed the exact build/ABI/empty-memory-request startup policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Used by the WMI RX session in the production library composition.
pub struct ServiceReadySnapshot {
    fixed_event: [u8; SERVICE_READY_VALUE_SIZE],
    hal_reg_capabilities: [u8; HAL_REG_CAPABILITIES_VALUE_SIZE],
    service_bitmap: [u8; SERVICE_BITMAP_SIZE],
}

/// Borrowed WMI_SERVICE_AVAILABLE_EVENTID extension bitmap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceAvailableView<'a> {
    segment_offset: u32,
    segment_bitmap: &'a [u8],
}

/// Owned copy of the validated four-word SERVICE_AVAILABLE extension bitmap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Used by the WMI RX session in the production library composition.
pub struct ServiceAvailableSnapshot {
    segment_offset: u32,
    segment_bitmap: [u8; SERVICE_AVAILABLE_BITMAP_SIZE],
}

/// One decoded, untrusted QCACLD host-memory request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostMemoryRequest {
    pub request_id: u32,
    pub unit_size: u32,
    pub unit_info: u32,
    pub unit_count: u32,
}

impl<'a> TlvIter<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
}

impl<'a> Iterator for TlvIter<'a> {
    type Item = Result<TlvRecord<'a>, TlvError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.bytes.len() {
            return None;
        }
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if remaining < TLV_HEADER_SIZE {
            self.offset = self.bytes.len();
            return Some(Err(TlvError::HeaderTruncated));
        }
        let start = self.offset;
        let len = usize::from(u16::from_le_bytes([
            self.bytes[start],
            self.bytes[start + 1],
        ]));
        let value_start = start + TLV_HEADER_SIZE;
        let value_end = match value_start.checked_add(len) {
            Some(end) if end <= self.bytes.len() => end,
            _ => {
                self.offset = self.bytes.len();
                return Some(Err(TlvError::ValueTruncated));
            }
        };
        let tag = u16::from_le_bytes([self.bytes[start + 2], self.bytes[start + 3]]);
        self.offset = value_end;
        Some(Ok(TlvRecord {
            tag,
            value: &self.bytes[value_start..value_end],
        }))
    }
}

impl HostMemoryRequest {
    pub fn parse(record: TlvRecord<'_>) -> Result<Self, ServiceReadyError> {
        if record.tag != TAG_HOST_MEM_REQ {
            return Err(ServiceReadyError::MemoryRequestTagUnexpected);
        }
        if record.value.len() != HOST_MEM_REQ_VALUE_SIZE {
            return Err(ServiceReadyError::MemoryRequestWrongLength);
        }
        let word = |offset: usize| {
            u32::from_le_bytes([
                record.value[offset],
                record.value[offset + 1],
                record.value[offset + 2],
                record.value[offset + 3],
            ])
        };
        Ok(Self {
            request_id: word(0),
            unit_size: word(4),
            unit_info: word(8),
            unit_count: word(12),
        })
    }
}

impl<'a> ServiceReadyView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ServiceReadyError> {
        let mut fixed_event = None;
        let mut hal_reg_capabilities = None;
        let mut service_bitmap = None;
        let mut memory_requests = None;

        for record in TlvIter::new(bytes) {
            let record = record.map_err(ServiceReadyError::Tlv)?;
            match record.tag {
                TAG_SERVICE_READY => {
                    if fixed_event.replace(record.value).is_some() {
                        return Err(ServiceReadyError::ServiceReadyDuplicate);
                    }
                    if record.value.len() != SERVICE_READY_VALUE_SIZE {
                        return Err(ServiceReadyError::ServiceReadyWrongLength);
                    }
                }
                TAG_HAL_REG_CAPABILITIES => {
                    if hal_reg_capabilities.replace(record.value).is_some() {
                        return Err(ServiceReadyError::HalCapabilitiesDuplicate);
                    }
                    if record.value.len() != HAL_REG_CAPABILITIES_VALUE_SIZE {
                        return Err(ServiceReadyError::HalCapabilitiesWrongLength);
                    }
                }
                TAG_ARRAY_UINT32 if service_bitmap.is_none() => {
                    if record.value.len() != SERVICE_BITMAP_SIZE {
                        return Err(ServiceReadyError::ServiceBitmapMalformed);
                    }
                    service_bitmap = Some(record.value);
                }
                TAG_ARRAY_UINT32 => {
                    // Later ARRAY_UINT32 records can carry optional capability
                    // arrays; they are not part of the service bitmap.
                }
                TAG_ARRAY_STRUCT => {
                    if memory_requests.replace(record.value).is_some() {
                        return Err(ServiceReadyError::MemoryRequestsDuplicate);
                    }
                }
                _ => {}
            }
        }

        let fixed_event = fixed_event.ok_or(ServiceReadyError::ServiceReadyMissing)?;
        let hal_reg_capabilities =
            hal_reg_capabilities.ok_or(ServiceReadyError::HalCapabilitiesMissing)?;
        let service_bitmap = service_bitmap.ok_or(ServiceReadyError::ServiceBitmapMissing)?;
        let declared_memory_request_count = u32::from_le_bytes([
            fixed_event[NUM_MEM_REQS_OFFSET],
            fixed_event[NUM_MEM_REQS_OFFSET + 1],
            fixed_event[NUM_MEM_REQS_OFFSET + 2],
            fixed_event[NUM_MEM_REQS_OFFSET + 3],
        ]);
        if declared_memory_request_count > MAX_MEM_REQS {
            return Err(ServiceReadyError::TooManyMemoryRequests);
        }
        let memory_requests = match memory_requests {
            Some(requests) => requests,
            None if declared_memory_request_count == 0 => &[],
            None => return Err(ServiceReadyError::MemoryRequestsMissing),
        };

        let mut observed = 0u32;
        for request in TlvIter::new(memory_requests) {
            let request = request.map_err(ServiceReadyError::Tlv)?;
            HostMemoryRequest::parse(request)?;
            observed = observed
                .checked_add(1)
                .ok_or(ServiceReadyError::MemoryRequestCountMismatch)?;
            if observed > MAX_MEM_REQS {
                return Err(ServiceReadyError::TooManyMemoryRequests);
            }
        }
        if observed != declared_memory_request_count {
            return Err(ServiceReadyError::MemoryRequestCountMismatch);
        }

        Ok(Self {
            fixed_event,
            hal_reg_capabilities,
            service_bitmap,
            memory_requests,
            declared_memory_request_count,
        })
    }

    pub fn service_word(&self, index: usize) -> Option<u32> {
        let start = index.checked_mul(4)?;
        let word = self.service_bitmap.get(start..start.checked_add(4)?)?;
        Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    }

    /// Return one raw WMI service capability by its source enum index.
    /// `None` means the complete containing word was not present; callers
    /// must not treat a missing bit as either enabled or disabled.
    pub fn service_enabled(&self, service_index: usize) -> Option<bool> {
        // Match the pinned QCACLD WMI_SERVICE_IS_ENABLED macro exactly. That
        // downstream macro divides and takes the modulus by sizeof(A_UINT32)
        // (4), rather than 32. This unusual packing is part of this target's
        // observed driver ABI. Extended IDs are supplied by SERVICE_AVAILABLE.
        if service_index >= SERVICE_MAX_BASE_ID {
            return None;
        }
        let word = self.service_word(service_index / SERVICE_BASE_INDEX_STRIDE)?;
        Some((word & (1u32 << (service_index % SERVICE_BASE_INDEX_STRIDE))) != 0)
    }

    /// Copy the small set of service-ready fields needed to build WMI_INIT
    /// after SERVICE_AVAILABLE arrives in a later CE2 completion. The accepted
    /// empty memory-request set is represented directly instead of retaining
    /// any receive-buffer borrow.
    #[allow(dead_code)] // Consumed by the generation-bound WMI RX session.
    pub fn sunfish_startup_snapshot(&self) -> Result<ServiceReadySnapshot, SunfishWmiStartupError> {
        self.require_sunfish_wmi_startup()?;
        let mut fixed_event = [0u8; SERVICE_READY_VALUE_SIZE];
        let mut hal_reg_capabilities = [0u8; HAL_REG_CAPABILITIES_VALUE_SIZE];
        let mut service_bitmap = [0u8; SERVICE_BITMAP_SIZE];
        fixed_event.copy_from_slice(self.fixed_event);
        hal_reg_capabilities.copy_from_slice(self.hal_reg_capabilities);
        service_bitmap.copy_from_slice(self.service_bitmap);
        Ok(ServiceReadySnapshot {
            fixed_event,
            hal_reg_capabilities,
            service_bitmap,
        })
    }

    /// Accept only the captured Sunfish WCN3998 build whose service-ready
    /// event reports zero dynamic host-memory requests. This never allocates,
    /// maps, or authorizes DMA; a changed build or nonempty list needs a
    /// separately validated memory policy.
    pub fn require_empty_sunfish_memory_requests(
        &self,
    ) -> Result<EmptyMemoryRequestSet<'a>, EmptyMemoryRequestError> {
        let firmware_build_version = u32::from_le_bytes([
            self.fixed_event[0],
            self.fixed_event[1],
            self.fixed_event[2],
            self.fixed_event[3],
        ]);
        if firmware_build_version != SUNFISH_WCN3998_FIRMWARE_BUILD {
            return Err(EmptyMemoryRequestError::FirmwareBuildMismatch {
                expected: SUNFISH_WCN3998_FIRMWARE_BUILD,
                observed: firmware_build_version,
            });
        }
        if self.declared_memory_request_count != 0 {
            return Err(EmptyMemoryRequestError::RequestsPresent {
                count: self.declared_memory_request_count,
            });
        }
        Ok(EmptyMemoryRequestSet {
            firmware_build_version,
            _event: self.fixed_event,
        })
    }

    /// Validate the ABI comparison used before QCACLD sends WMI_INIT.
    ///
    /// The pinned host's version whitelist contains only a dummy major-zero
    /// entry. Its WMI version helper therefore requires the same major/minor
    /// and all four namespace words; the ABI revision word is intentionally
    /// ignored by `wmi_versions_are_compatible()`.
    pub fn require_sunfish_wmi_startup(
        &self,
    ) -> Result<SunfishWmiStartup<'a>, SunfishWmiStartupError> {
        let memory_requests = self
            .require_empty_sunfish_memory_requests()
            .map_err(SunfishWmiStartupError::MemoryRequests)?;
        const ABI_OFFSET: usize = 4;
        for (word_index, expected) in SUNFISH_WMI_HOST_ABI.iter().copied().enumerate() {
            if word_index == 1 {
                // QCACLD compares ABI major/minor and namespaces, not revision.
                continue;
            }
            let start = ABI_OFFSET + word_index * 4;
            let observed = u32::from_le_bytes([
                self.fixed_event[start],
                self.fixed_event[start + 1],
                self.fixed_event[start + 2],
                self.fixed_event[start + 3],
            ]);
            if observed != expected {
                return Err(SunfishWmiStartupError::AbiMismatch {
                    word_index,
                    expected,
                    observed,
                });
            }
        }
        Ok(SunfishWmiStartup {
            _memory_requests: memory_requests,
        })
    }

    pub fn memory_requests(&self) -> TlvIter<'a> {
        TlvIter::new(self.memory_requests)
    }
}

impl<'a> ServiceAvailableView<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ServiceAvailableError> {
        let mut fixed = None;
        for record in TlvIter::new(bytes) {
            let record = record.map_err(ServiceAvailableError::Tlv)?;
            if record.tag != TAG_SERVICE_AVAILABLE {
                continue;
            }
            if fixed.replace(record.value).is_some() {
                return Err(ServiceAvailableError::FixedParameterDuplicate);
            }
            if record.value.len() != SERVICE_AVAILABLE_VALUE_SIZE {
                return Err(ServiceAvailableError::FixedParameterWrongLength);
            }
        }
        let fixed = fixed.ok_or(ServiceAvailableError::FixedParameterMissing)?;
        let segment_offset = read_u32_at(fixed, 0);
        if segment_offset != SERVICE_AVAILABLE_SEGMENT_OFFSET {
            return Err(ServiceAvailableError::UnexpectedSegmentOffset {
                observed: segment_offset,
            });
        }
        Ok(Self {
            segment_offset,
            segment_bitmap: &fixed[4..],
        })
    }

    pub const fn segment_offset(&self) -> u32 {
        self.segment_offset
    }

    /// Return one extended service bit using the pinned QCACLD mapping.
    /// `None` means the ID is outside the captured four-word segment.
    pub fn service_enabled(&self, service_index: usize) -> Option<bool> {
        let segment_offset = usize::try_from(self.segment_offset).ok()?;
        let relative = service_index.checked_sub(segment_offset)?;
        let word_index = relative / 32;
        if word_index >= SERVICE_AVAILABLE_WORDS {
            return None;
        }
        let word = read_u32_at(self.segment_bitmap, word_index * 4);
        Some((word & (1u32 << (service_index & 0x1f))) != 0)
    }

    /// Capture the source-defined SERVICE_AVAILABLE segment for later WMI_INIT
    /// resource construction without retaining the CE scratch-buffer borrow.
    #[allow(dead_code)] // Consumed by the generation-bound WMI RX session.
    pub fn snapshot(&self) -> ServiceAvailableSnapshot {
        let mut segment_bitmap = [0u8; SERVICE_AVAILABLE_BITMAP_SIZE];
        segment_bitmap.copy_from_slice(self.segment_bitmap);
        ServiceAvailableSnapshot {
            segment_offset: self.segment_offset,
            segment_bitmap,
        }
    }
}

impl ServiceReadySnapshot {
    /// Reconstruct the same validated view over owned bytes. Since construction
    /// requires a successful Sunfish startup policy check, the empty memory
    /// request list remains exact and is not inferred from untrusted input.
    #[allow(dead_code)] // Consumed by the WMI_INIT resource builder.
    pub const fn view(&self) -> ServiceReadyView<'_> {
        ServiceReadyView {
            fixed_event: &self.fixed_event,
            hal_reg_capabilities: &self.hal_reg_capabilities,
            service_bitmap: &self.service_bitmap,
            memory_requests: &[],
            declared_memory_request_count: 0,
        }
    }
}

impl ServiceAvailableSnapshot {
    /// Reconstruct the source-validated extension view over the owned bitmap.
    #[allow(dead_code)] // Consumed by the WMI_INIT resource builder.
    pub const fn view(&self) -> ServiceAvailableView<'_> {
        ServiceAvailableView {
            segment_offset: self.segment_offset,
            segment_bitmap: &self.segment_bitmap,
        }
    }
}

impl SunfishWmiStartup<'_> {
    /// Radio and resource limits reported by this parsed service-ready event.
    /// These values remain untrusted firmware input; callers must apply a
    /// bounded Sunfish policy before using them in WMI configuration.
    pub fn firmware_num_rf_chains(&self) -> u32 {
        read_u32_at(self._memory_requests._event, NUM_RF_CHAINS_OFFSET)
    }

    pub fn firmware_max_fragment_entries(&self) -> u32 {
        read_u32_at(self._memory_requests._event, MAX_FRAGMENT_ENTRIES_OFFSET)
    }

    pub fn firmware_max_scan_channels(&self) -> u32 {
        read_u32_at(self._memory_requests._event, MAX_SCAN_CHANNELS_OFFSET)
    }

    pub fn firmware_max_supported_macs(&self) -> u32 {
        read_u32_at(self._memory_requests._event, MAX_SUPPORTED_MACS_OFFSET)
    }

    pub fn firmware_num_dbs_hw_modes(&self) -> u32 {
        read_u32_at(self._memory_requests._event, NUM_DBS_HW_MODES_OFFSET)
    }

    pub fn firmware_txrx_chainmask(&self) -> u32 {
        read_u32_at(self._memory_requests._event, TX_RX_CHAINMASK_OFFSET)
    }

    /// Format the pinned QCACLD WMI_INIT TLVs for an already policy-validated
    /// resource-config TLV, with zero host-memory chunks. An optional hardware
    /// mode emits the fixed mode record with no band mappings. The caller must
    /// still keep this byte slice offline until transport and DMA ownership
    /// have independent validation.
    pub fn encode_init_body(
        &self,
        resource_config_tlv: &[u8],
        hardware_mode_index: Option<u32>,
        output: &mut [u8],
    ) -> Result<usize, WmiInitEncodeError> {
        if let Some(index) = hardware_mode_index {
            if index != 0 {
                return Err(WmiInitEncodeError::UnsupportedHardwareMode { observed: index });
            }
        }
        let resource_config = WmiResourceConfig::parse(resource_config_tlv)?;

        let required = WMI_INIT_FIXED_SIZE
            + WMI_RESOURCE_CONFIG_SIZE
            + WMI_INIT_EMPTY_MEM_ARRAY_SIZE
            + if hardware_mode_index.is_some() {
                WMI_INIT_HW_MODE_SIZE
            } else {
                0
            };
        if output.len() < required {
            return Err(WmiInitEncodeError::OutputTooSmall {
                required,
                available: output.len(),
            });
        }

        let mut offset = 0;
        put_u32(output, &mut offset, tlv_header(TAG_INIT_FIXED, 28));
        for word in SUNFISH_WMI_HOST_ABI {
            put_u32(output, &mut offset, word);
        }
        put_u32(output, &mut offset, 0); // num_host_mem_chunks
        output[offset..offset + WMI_RESOURCE_CONFIG_SIZE].copy_from_slice(resource_config.as_tlv());
        offset += WMI_RESOURCE_CONFIG_SIZE;
        put_u32(output, &mut offset, tlv_header(TAG_ARRAY_STRUCT, 0));

        if let Some(hw_mode_index) = hardware_mode_index {
            put_u32(output, &mut offset, tlv_header(TAG_HW_MODE_FIXED, 12));
            put_u32(output, &mut offset, 0); // pdev_id, zeroed by QCACLD
            put_u32(output, &mut offset, hw_mode_index);
            put_u32(output, &mut offset, 0); // num_band_to_mac
            put_u32(output, &mut offset, tlv_header(TAG_ARRAY_STRUCT, 0));
        }
        debug_assert_eq!(offset, required);
        Ok(offset)
    }

    /// Parse the QCACLD WMI_READY_EVENTID after a prepared WMI INIT. This
    /// validates only the event framing, ABI, MAC-list geometry, and zero
    /// status; it does not claim that the message was sent on a live transport.
    pub fn decode_ready_event<'a>(
        &self,
        bytes: &'a [u8],
    ) -> Result<WmiReadyEvent<'a>, WmiReadyError> {
        WmiReadyEvent::parse(bytes)
    }
}

impl<'a> WmiReadyEvent<'a> {
    /// Parse the complete QCACLD READY fixed structure and optional MAC list.
    /// The firmware ABI and status are checked against the pinned Sunfish
    /// host contract, so a short ath10k-compatible prefix is not accepted.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, WmiReadyError> {
        let mut fixed = None;
        let mut mac_list = None;
        for record in TlvIter::new(bytes) {
            let record = record.map_err(WmiReadyError::Tlv)?;
            match record.tag {
                TAG_READY_EVENT => {
                    if fixed.replace(record.value).is_some() {
                        return Err(WmiReadyError::FixedParameterDuplicate);
                    }
                    if record.value.len() != WMI_READY_EVENT_VALUE_SIZE {
                        return Err(WmiReadyError::FixedParameterWrongLength);
                    }
                }
                TAG_ARRAY_FIXED_STRUCT => {
                    if mac_list.replace(record.value).is_some() {
                        return Err(WmiReadyError::MacListDuplicate);
                    }
                }
                _ => {}
            }
        }

        let fixed = fixed.ok_or(WmiReadyError::FixedParameterMissing)?;
        for (word_index, expected) in SUNFISH_WMI_HOST_ABI.iter().copied().enumerate() {
            if word_index == 1 {
                continue;
            }
            let start = word_index * 4;
            let observed = read_u32_at(fixed, start);
            if observed != expected {
                return Err(WmiReadyError::AbiMismatch {
                    word_index,
                    expected,
                    observed,
                });
            }
        }

        let status = read_u32_at(fixed, WMI_READY_STATUS_OFFSET);
        if status != 0 {
            return Err(WmiReadyError::InitFailed { status });
        }
        let extra_mac_count = read_u32_at(fixed, WMI_READY_EXTRA_MAC_COUNT_OFFSET);
        if extra_mac_count > MAX_READY_EXTRA_MACS {
            return Err(WmiReadyError::TooManyMacAddresses {
                count: extra_mac_count,
            });
        }
        let expected_mac_bytes = usize::try_from(extra_mac_count)
            .ok()
            .and_then(|count| count.checked_mul(WMI_MAC_ADDRESS_SIZE))
            .ok_or(WmiReadyError::TooManyMacAddresses {
                count: extra_mac_count,
            })?;
        let extra_mac_addresses = match (mac_list, extra_mac_count) {
            (Some(list), _) if list.len() != expected_mac_bytes => {
                return Err(WmiReadyError::MacListWrongLength {
                    expected: expected_mac_bytes,
                    observed: list.len(),
                });
            }
            (Some(list), _) => list,
            (None, 0) => &[],
            (None, count) => return Err(WmiReadyError::MacListMissing { count }),
        };

        Ok(Self {
            mac_address_words: [
                read_u32_at(fixed, WMI_READY_MAC_OFFSET),
                read_u32_at(fixed, WMI_READY_MAC_OFFSET + 4),
            ],
            extra_mac_addresses,
            total_peers: read_u32_at(fixed, 44),
            extra_peers: read_u32_at(fixed, 48),
        })
    }
}

fn tlv_header(tag: u16, value_len: u16) -> u32 {
    (u32::from(tag) << 16) | u32::from(value_len)
}

fn put_u32(output: &mut [u8], offset: &mut usize, value: u32) {
    output[*offset..*offset + 4].copy_from_slice(&value.to_le_bytes());
    *offset += 4;
}

fn read_u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn append_tlv(bytes: &mut Vec<u8>, tag: u16, value: &[u8]) {
        bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(value);
    }

    fn fixture(requests: &[u8], declared_count: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut fixed = [0u8; SERVICE_READY_VALUE_SIZE];
        fixed[NUM_MEM_REQS_OFFSET..NUM_MEM_REQS_OFFSET + 4]
            .copy_from_slice(&declared_count.to_le_bytes());
        append_tlv(&mut bytes, TAG_SERVICE_READY, &fixed);
        append_tlv(
            &mut bytes,
            TAG_HAL_REG_CAPABILITIES,
            &[0; HAL_REG_CAPABILITIES_VALUE_SIZE],
        );
        let mut service_bitmap = [0u8; SERVICE_BITMAP_SIZE];
        service_bitmap[..4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        append_tlv(&mut bytes, TAG_ARRAY_UINT32, &service_bitmap);
        append_tlv(&mut bytes, TAG_ARRAY_STRUCT, requests);
        bytes
    }

    fn service_available_fixture(segment_offset: u32, words: [u32; 4]) -> Vec<u8> {
        let mut fixed = [0u8; SERVICE_AVAILABLE_VALUE_SIZE];
        fixed[..4].copy_from_slice(&segment_offset.to_le_bytes());
        for (index, word) in words.into_iter().enumerate() {
            fixed[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        let mut bytes = Vec::new();
        append_tlv(&mut bytes, TAG_SERVICE_AVAILABLE, &fixed);
        bytes
    }

    fn set_sunfish_startup_identity(bytes: &mut [u8], revision: u32) {
        bytes[4..8].copy_from_slice(&SUNFISH_WCN3998_FIRMWARE_BUILD.to_le_bytes());
        bytes[36..40].copy_from_slice(&12u32.to_le_bytes());
        bytes[40..44].copy_from_slice(&2u32.to_le_bytes());
        bytes[80..84].copy_from_slice(&59u32.to_le_bytes());
        bytes[108..112].copy_from_slice(&2u32.to_le_bytes());
        bytes[116..120].copy_from_slice(&1u32.to_le_bytes());
        bytes[120..124].copy_from_slice(&3u32.to_le_bytes());
        let mut abi = SUNFISH_WMI_HOST_ABI;
        abi[1] = revision;
        for (index, word) in abi.into_iter().enumerate() {
            let start = 8 + index * 4;
            bytes[start..start + 4].copy_from_slice(&word.to_le_bytes());
        }
    }

    fn empty_resource_config_tlv() -> Vec<u8> {
        let mut tlv = vec![0; WMI_RESOURCE_CONFIG_SIZE];
        tlv[..4].copy_from_slice(
            &tlv_header(TAG_RESOURCE_CONFIG, WMI_RESOURCE_CONFIG_VALUE_SIZE).to_le_bytes(),
        );
        tlv
    }

    fn startup_evidence(bytes: &[u8]) -> SunfishWmiStartup<'_> {
        ServiceReadyView::parse(bytes)
            .unwrap()
            .require_sunfish_wmi_startup()
            .unwrap()
    }

    #[test]
    fn lineage_sunfish_defaults_derive_only_wma_dynamic_resource_counts() {
        let counts = QcacldWmaHostConfig::lineage_sunfish_defaults()
            .derive_resource_counts(QcacldPeerLimit::OtherTarget32)
            .unwrap();

        assert_eq!(
            counts,
            QcacldWmaResourceCounts {
                num_vdevs: 4,
                supported_peers: 32,
                num_peers: 38,
                num_offload_peers: 3,
                num_offload_reorder_buffers: 3,
                num_tids: 76,
                scan_max_pending_req: 4,
            }
        );
    }

    #[test]
    fn qca6174_revision_one_one_cap_is_applied_before_peer_and_tid_counts() {
        let counts = QcacldWmaHostConfig::lineage_sunfish_defaults()
            .derive_resource_counts(QcacldPeerLimit::Qca6174Revision1_1)
            .unwrap();

        assert_eq!(counts.supported_peers, 14);
        assert_eq!(counts.num_peers, 20);
        assert_eq!(counts.num_tids, 40);
    }

    #[test]
    fn observed_host_overrides_are_bounded_and_offload_counts_add_one() {
        let counts = QcacldWmaHostConfig {
            max_station: 8,
            ap_max_offload_peers: 5,
            ap_max_offload_reorder_buffers: 0,
            max_scan: 8,
        }
        .derive_resource_counts(QcacldPeerLimit::OtherTarget32)
        .unwrap();

        assert_eq!(counts.num_peers, 14);
        assert_eq!(counts.num_tids, 28);
        assert_eq!(counts.num_offload_peers, 6);
        assert_eq!(counts.num_offload_reorder_buffers, 1);
        assert_eq!(counts.scan_max_pending_req, 8);
    }

    #[test]
    fn wma_resource_count_policy_rejects_out_of_range_host_values() {
        let defaults = QcacldWmaHostConfig::lineage_sunfish_defaults();
        assert_eq!(
            QcacldWmaHostConfig {
                max_station: 0,
                ..defaults
            }
            .derive_resource_counts(QcacldPeerLimit::OtherTarget32),
            Err(QcacldWmaPolicyError::MaxStation)
        );
        assert_eq!(
            QcacldWmaHostConfig {
                ap_max_offload_peers: 1,
                ..defaults
            }
            .derive_resource_counts(QcacldPeerLimit::OtherTarget32),
            Err(QcacldWmaPolicyError::OffloadPeers)
        );
        assert_eq!(
            QcacldWmaHostConfig {
                ap_max_offload_reorder_buffers: 4,
                ..defaults
            }
            .derive_resource_counts(QcacldPeerLimit::OtherTarget32),
            Err(QcacldWmaPolicyError::OffloadReorderBuffers)
        );
        assert_eq!(
            QcacldWmaHostConfig {
                max_scan: 9,
                ..defaults
            }
            .derive_resource_counts(QcacldPeerLimit::OtherTarget32),
            Err(QcacldWmaPolicyError::MaxScan)
        );
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn resource_config_view_maps_fields_to_pinned_wire_word_indices() {
        let mut tlv = empty_resource_config_tlv();
        for index in 0..WMI_RESOURCE_CONFIG_WORDS {
            let start = TLV_HEADER_SIZE + index * 4;
            tlv[start..start + 4].copy_from_slice(&(0xa000_0000 | index as u32).to_le_bytes());
        }

        let config = WmiResourceConfig::parse(&tlv).unwrap();
        assert_eq!(config.as_tlv(), tlv.as_slice());
        assert_eq!(config.num_vdevs(), 0xa000_0000);
        assert_eq!(config.num_peers(), 0xa000_0001);
        assert_eq!(config.num_offload_peers(), 0xa000_0002);
        assert_eq!(config.num_offload_reorder_buffers(), 0xa000_0003);
        assert_eq!(config.num_peer_keys(), 0xa000_0004);
        assert_eq!(config.num_tids(), 0xa000_0005);
        assert_eq!(config.tx_chain_mask(), 0xa000_0007);
        assert_eq!(config.rx_chain_mask(), 0xa000_0008);
        assert_eq!(config.num_msdu_descriptors(), 0xa000_001c);
        assert_eq!(config.max_fragment_entries(), 0xa000_001d);
        assert_eq!(config.feature_flags(), 0xa000_002b);
        assert_eq!(config.num_packet_filters(), 0xa000_003d);
        assert_eq!(config.num_max_sta_vdevs(), 0xa000_003e);
    }

    #[test]
    fn resource_config_view_rejects_bad_size_tag_and_value_length() {
        let valid = empty_resource_config_tlv();
        assert_eq!(
            WmiResourceConfig::parse(&valid[..valid.len() - 1]),
            Err(WmiInitEncodeError::ResourceConfigWrongSize {
                expected: WMI_RESOURCE_CONFIG_SIZE,
                observed: WMI_RESOURCE_CONFIG_SIZE - 1,
            })
        );

        let mut wrong_tag = valid.clone();
        wrong_tag[..4]
            .copy_from_slice(&tlv_header(1, WMI_RESOURCE_CONFIG_VALUE_SIZE).to_le_bytes());
        assert_eq!(
            WmiResourceConfig::parse(&wrong_tag),
            Err(WmiInitEncodeError::ResourceConfigWrongTag { observed: 1 })
        );

        let mut wrong_length = valid;
        wrong_length[..4].copy_from_slice(&tlv_header(TAG_RESOURCE_CONFIG, 4).to_le_bytes());
        assert_eq!(
            WmiResourceConfig::parse(&wrong_length),
            Err(WmiInitEncodeError::ResourceConfigWrongLength {
                expected: WMI_RESOURCE_CONFIG_VALUE_SIZE,
                observed: 4,
            })
        );
    }

    fn ready_event_fixture(extra_mac_count: u32, status: u32, with_mac_list: bool) -> Vec<u8> {
        let mut fixed = [0u8; WMI_READY_EVENT_VALUE_SIZE];
        for (index, word) in SUNFISH_WMI_HOST_ABI.into_iter().enumerate() {
            fixed[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        fixed[WMI_READY_MAC_OFFSET..WMI_READY_MAC_OFFSET + 4]
            .copy_from_slice(&0x4433_2211u32.to_le_bytes());
        fixed[WMI_READY_MAC_OFFSET + 4..WMI_READY_MAC_OFFSET + 8]
            .copy_from_slice(&0x0000_6655u32.to_le_bytes());
        fixed[WMI_READY_STATUS_OFFSET..WMI_READY_STATUS_OFFSET + 4]
            .copy_from_slice(&status.to_le_bytes());
        fixed[WMI_READY_EXTRA_MAC_COUNT_OFFSET..WMI_READY_EXTRA_MAC_COUNT_OFFSET + 4]
            .copy_from_slice(&extra_mac_count.to_le_bytes());
        fixed[44..48].copy_from_slice(&16u32.to_le_bytes());
        fixed[48..52].copy_from_slice(&2u32.to_le_bytes());
        let mut bytes = Vec::new();
        append_tlv(&mut bytes, TAG_READY_EVENT, &fixed);
        if with_mac_list {
            append_tlv(
                &mut bytes,
                TAG_ARRAY_FIXED_STRUCT,
                &vec![0x5a; extra_mac_count as usize * WMI_MAC_ADDRESS_SIZE],
            );
        }
        bytes
    }

    #[test]
    fn qcacld_schema_reads_count_from_pinned_fixed_event_and_decodes_array() {
        let mut requests = Vec::new();
        let mut request = [0u8; HOST_MEM_REQ_VALUE_SIZE];
        request[0..4].copy_from_slice(&0x240u32.to_le_bytes());
        request[4..8].copy_from_slice(&0x1000u32.to_le_bytes());
        request[8..12].copy_from_slice(&2u32.to_le_bytes());
        request[12..16].copy_from_slice(&0u32.to_le_bytes());
        append_tlv(&mut requests, TAG_HOST_MEM_REQ, &request);

        let bytes = fixture(&requests, 1);
        let view = ServiceReadyView::parse(&bytes).unwrap();
        assert_eq!(view.declared_memory_request_count, 1);
        assert_eq!(view.service_word(0), Some(0x4433_2211));
        assert_eq!(view.service_word(1), Some(0));
        assert_eq!(view.service_word(32), None);
        assert_eq!(view.service_enabled(0), Some(true));
        assert_eq!(view.service_enabled(1), Some(false));
        assert_eq!(view.service_enabled(1024), None);
        let record = view.memory_requests().next().unwrap().unwrap();
        assert_eq!(
            HostMemoryRequest::parse(record),
            Ok(HostMemoryRequest {
                request_id: 0x240,
                unit_size: 0x1000,
                unit_info: 2,
                unit_count: 0,
            })
        );
    }

    #[test]
    fn base_service_bits_follow_the_pinned_qcacld_sizeof_stride() {
        let mut bytes = fixture(&[], 0);
        // SERVICE_READY fixed TLV (132 bytes), HAL capabilities TLV (40
        // bytes), then ARRAY_UINT32 header. Service 99 is word 24, bit 3 in
        // the QCACLD macro's sizeof(A_UINT32) indexing scheme.
        let service_bitmap_value = 132 + 40 + TLV_HEADER_SIZE;
        let service_word_offset = service_bitmap_value + (99 / SERVICE_BASE_INDEX_STRIDE) * 4;
        bytes[service_word_offset..service_word_offset + 4]
            .copy_from_slice(&(1u32 << (99 % SERVICE_BASE_INDEX_STRIDE)).to_le_bytes());
        let view = ServiceReadyView::parse(&bytes).unwrap();

        assert_eq!(view.service_enabled(99), Some(true));
        assert_eq!(view.service_enabled(98), Some(false));
        assert_eq!(view.service_enabled(100), Some(false));
        assert_eq!(view.service_enabled(SERVICE_MAX_BASE_ID), None);
    }

    #[test]
    fn extended_service_bits_follow_the_service_available_segment() {
        let words = [0, (1 << (176 & 0x1f)) | (1 << (182 & 0x1f)), 0, 0];
        let bytes = service_available_fixture(SERVICE_AVAILABLE_SEGMENT_OFFSET, words);
        let view = ServiceAvailableView::parse(&bytes).unwrap();

        assert_eq!(view.segment_offset(), 128);
        assert_eq!(view.service_enabled(176), Some(true));
        assert_eq!(view.service_enabled(182), Some(true));
        assert_eq!(view.service_enabled(127), None);
        assert_eq!(view.service_enabled(256), None);
    }

    #[test]
    fn service_available_rejects_wrong_segment_and_malformed_fixed_tlv() {
        let wrong_segment = service_available_fixture(160, [0; 4]);
        assert_eq!(
            ServiceAvailableView::parse(&wrong_segment),
            Err(ServiceAvailableError::UnexpectedSegmentOffset { observed: 160 })
        );

        let wrong_length = {
            let mut bytes = Vec::new();
            append_tlv(&mut bytes, TAG_SERVICE_AVAILABLE, &[0; 16]);
            bytes
        };
        assert_eq!(
            ServiceAvailableView::parse(&wrong_length),
            Err(ServiceAvailableError::FixedParameterWrongLength)
        );

        let duplicate = service_available_fixture(SERVICE_AVAILABLE_SEGMENT_OFFSET, [0; 4]);
        let mut duplicate = duplicate.clone();
        duplicate.extend_from_slice(&service_available_fixture(
            SERVICE_AVAILABLE_SEGMENT_OFFSET,
            [0; 4],
        ));
        assert_eq!(
            ServiceAvailableView::parse(&duplicate),
            Err(ServiceAvailableError::FixedParameterDuplicate)
        );
    }

    #[test]
    fn rejects_layout_mismatch_count_overflow_and_bad_array_elements() {
        let mut short = Vec::new();
        append_tlv(&mut short, TAG_SERVICE_READY, &[0; 104]);
        assert_eq!(
            ServiceReadyView::parse(&short),
            Err(ServiceReadyError::ServiceReadyWrongLength)
        );

        assert_eq!(
            ServiceReadyView::parse(&fixture(&[], MAX_MEM_REQS + 1)),
            Err(ServiceReadyError::TooManyMemoryRequests)
        );
        assert_eq!(
            ServiceReadyView::parse(&fixture(&[], 1)),
            Err(ServiceReadyError::MemoryRequestCountMismatch)
        );

        let mut wrong_tag = Vec::new();
        append_tlv(&mut wrong_tag, 0x777, &[0; HOST_MEM_REQ_VALUE_SIZE]);
        assert_eq!(
            ServiceReadyView::parse(&fixture(&wrong_tag, 1)),
            Err(ServiceReadyError::MemoryRequestTagUnexpected)
        );

        let mut bad_length = Vec::new();
        append_tlv(&mut bad_length, TAG_HOST_MEM_REQ, &[0; 15]);
        assert_eq!(
            ServiceReadyView::parse(&fixture(&bad_length, 1)),
            Err(ServiceReadyError::MemoryRequestWrongLength)
        );
    }

    #[test]
    fn zero_declared_requests_may_omit_the_optional_array_struct() {
        let mut bytes = Vec::new();
        append_tlv(
            &mut bytes,
            TAG_SERVICE_READY,
            &[0; SERVICE_READY_VALUE_SIZE],
        );
        append_tlv(
            &mut bytes,
            TAG_HAL_REG_CAPABILITIES,
            &[0; HAL_REG_CAPABILITIES_VALUE_SIZE],
        );
        append_tlv(&mut bytes, TAG_ARRAY_UINT32, &[0; SERVICE_BITMAP_SIZE]);
        let view = ServiceReadyView::parse(&bytes).unwrap();
        assert_eq!(view.declared_memory_request_count, 0);
        assert_eq!(view.memory_requests().next(), None);
    }

    #[test]
    fn iterator_rejects_truncated_tlv_bytes() {
        assert_eq!(
            TlvIter::new(&[4, 0, 34, 0, 1]).next(),
            Some(Err(TlvError::ValueTruncated))
        );
        assert_eq!(
            TlvIter::new(&[1, 0, 34]).next(),
            Some(Err(TlvError::HeaderTruncated))
        );
    }

    #[test]
    fn every_truncation_of_a_service_ready_memory_request_fails_closed() {
        let mut requests = Vec::new();
        let mut request = [0u8; HOST_MEM_REQ_VALUE_SIZE];
        request[0..4].copy_from_slice(&0x240u32.to_le_bytes());
        request[4..8].copy_from_slice(&0x1000u32.to_le_bytes());
        append_tlv(&mut requests, TAG_HOST_MEM_REQ, &request);
        let bytes = fixture(&requests, 1);

        for end in 0..bytes.len() {
            assert!(
                ServiceReadyView::parse(&bytes[..end]).is_err(),
                "accepted truncation at byte {end} of {}",
                bytes.len()
            );
        }
        assert!(ServiceReadyView::parse(&bytes).is_ok());
    }

    #[test]
    fn empty_memory_profile_is_bound_to_the_captured_firmware_build() {
        let mut bytes = fixture(&[], 0);
        bytes[4..8].copy_from_slice(&SUNFISH_WCN3998_FIRMWARE_BUILD.to_le_bytes());
        let view = ServiceReadyView::parse(&bytes).unwrap();
        assert_eq!(
            view.require_empty_sunfish_memory_requests()
                .unwrap()
                .firmware_build_version(),
            SUNFISH_WCN3998_FIRMWARE_BUILD
        );

        bytes[4..8].copy_from_slice(&0x3226_822fu32.to_le_bytes());
        let wrong_build = ServiceReadyView::parse(&bytes).unwrap();
        assert_eq!(
            wrong_build.require_empty_sunfish_memory_requests(),
            Err(EmptyMemoryRequestError::FirmwareBuildMismatch {
                expected: SUNFISH_WCN3998_FIRMWARE_BUILD,
                observed: 0x3226_822f,
            })
        );

        let mut request = Vec::new();
        append_tlv(
            &mut request,
            TAG_HOST_MEM_REQ,
            &[0; HOST_MEM_REQ_VALUE_SIZE],
        );
        let mut with_request = fixture(&request, 1);
        with_request[4..8].copy_from_slice(&SUNFISH_WCN3998_FIRMWARE_BUILD.to_le_bytes());
        let request_event = ServiceReadyView::parse(&with_request).unwrap();
        assert_eq!(
            request_event.require_empty_sunfish_memory_requests(),
            Err(EmptyMemoryRequestError::RequestsPresent { count: 1 })
        );
    }

    #[test]
    fn startup_evidence_checks_sunfish_build_abi_and_ignores_abi_revision() {
        let mut bytes = fixture(&[], 0);
        set_sunfish_startup_identity(&mut bytes, 999);
        let view = ServiceReadyView::parse(&bytes).unwrap();
        let startup = view.require_sunfish_wmi_startup().unwrap();
        assert_eq!(startup.firmware_num_rf_chains(), 2);
        assert_eq!(startup.firmware_max_fragment_entries(), 12);
        assert_eq!(startup.firmware_max_scan_channels(), 59);
        assert_eq!(startup.firmware_max_supported_macs(), 2);
        assert_eq!(startup.firmware_num_dbs_hw_modes(), 1);
        assert_eq!(startup.firmware_txrx_chainmask(), 3);

        bytes[8..12].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        let incompatible_major = ServiceReadyView::parse(&bytes).unwrap();
        assert_eq!(
            incompatible_major.require_sunfish_wmi_startup(),
            Err(SunfishWmiStartupError::AbiMismatch {
                word_index: 0,
                expected: SUNFISH_WMI_HOST_ABI[0],
                observed: 0x0200_0000,
            })
        );

        set_sunfish_startup_identity(&mut bytes, SUNFISH_WMI_HOST_ABI[1]);
        bytes[4..8].copy_from_slice(&0x3226_822fu32.to_le_bytes());
        let wrong_build = ServiceReadyView::parse(&bytes).unwrap();
        assert!(matches!(
            wrong_build.require_sunfish_wmi_startup(),
            Err(SunfishWmiStartupError::MemoryRequests(
                EmptyMemoryRequestError::FirmwareBuildMismatch { .. }
            ))
        ));
    }

    #[test]
    fn init_encoder_emits_exact_no_chunk_qcacld_tlv_body() {
        let mut event = fixture(&[], 0);
        set_sunfish_startup_identity(&mut event, SUNFISH_WMI_HOST_ABI[1]);
        let startup = startup_evidence(&event);
        let resource = empty_resource_config_tlv();
        let mut output = [0xa5; 320];
        let written = startup
            .encode_init_body(&resource, None, &mut output)
            .unwrap();

        assert_eq!(SUNFISH_WMI_INIT_COMMAND_ID, 1);
        assert_eq!(written, 312);
        assert_eq!(read_u32(&output, 0), tlv_header(TAG_INIT_FIXED, 28));
        for (index, expected) in SUNFISH_WMI_HOST_ABI.into_iter().enumerate() {
            assert_eq!(read_u32(&output, 4 + index * 4), expected);
        }
        assert_eq!(read_u32(&output, 28), 0); // no host memory chunks
        assert_eq!(&output[WMI_INIT_FIXED_SIZE..308], resource.as_slice());
        assert_eq!(read_u32(&output, 308), tlv_header(TAG_ARRAY_STRUCT, 0));
        assert_eq!(&output[written..], &[0xa5; 8]);
    }

    #[test]
    fn init_encoder_adds_only_a_bounded_empty_mapping_hw_mode_record() {
        let mut event = fixture(&[], 0);
        set_sunfish_startup_identity(&mut event, SUNFISH_WMI_HOST_ABI[1]);
        let startup = startup_evidence(&event);
        let resource = empty_resource_config_tlv();
        let mut output = [0; 332];
        assert_eq!(
            startup
                .encode_init_body(&resource, Some(0), &mut output)
                .unwrap(),
            332
        );
        assert_eq!(read_u32(&output, 312), tlv_header(TAG_HW_MODE_FIXED, 12));
        assert_eq!(read_u32(&output, 316), 0); // pdev id
        assert_eq!(read_u32(&output, 320), 0); // selected mode index
        assert_eq!(read_u32(&output, 324), 0); // no band mappings
        assert_eq!(read_u32(&output, 328), tlv_header(TAG_ARRAY_STRUCT, 0));
    }

    #[test]
    fn init_encoder_rejects_bad_resource_tlvs_and_short_output_without_writes() {
        let mut event = fixture(&[], 0);
        set_sunfish_startup_identity(&mut event, SUNFISH_WMI_HOST_ABI[1]);
        let startup = startup_evidence(&event);
        let mut output = [0xa5; 332];

        assert_eq!(
            startup.encode_init_body(&[0; 8], None, &mut output),
            Err(WmiInitEncodeError::ResourceConfigWrongSize {
                expected: WMI_RESOURCE_CONFIG_SIZE,
                observed: 8,
            })
        );
        assert_eq!(&output, &[0xa5; 332]);

        let mut resource = empty_resource_config_tlv();
        resource[..4].copy_from_slice(&tlv_header(1, WMI_RESOURCE_CONFIG_VALUE_SIZE).to_le_bytes());
        assert_eq!(
            startup.encode_init_body(&resource, None, &mut output),
            Err(WmiInitEncodeError::ResourceConfigWrongTag { observed: 1 })
        );
        resource[..4].copy_from_slice(&tlv_header(TAG_RESOURCE_CONFIG, 4).to_le_bytes());
        assert_eq!(
            startup.encode_init_body(&resource, None, &mut output),
            Err(WmiInitEncodeError::ResourceConfigWrongLength {
                expected: WMI_RESOURCE_CONFIG_VALUE_SIZE,
                observed: 4,
            })
        );

        resource = empty_resource_config_tlv();
        assert_eq!(
            startup.encode_init_body(&resource, None, &mut output[..311]),
            Err(WmiInitEncodeError::OutputTooSmall {
                required: 312,
                available: 311,
            })
        );
        assert_eq!(
            startup.encode_init_body(&resource, Some(1), &mut output),
            Err(WmiInitEncodeError::UnsupportedHardwareMode { observed: 1 })
        );
        assert_eq!(&output, &[0xa5; 332]);
    }

    #[test]
    fn ready_decoder_accepts_matching_abi_zero_status_and_bounded_mac_list() {
        let mut service_ready = fixture(&[], 0);
        set_sunfish_startup_identity(&mut service_ready, SUNFISH_WMI_HOST_ABI[1]);
        let startup = startup_evidence(&service_ready);

        let event = ready_event_fixture(2, 0, true);
        let ready = startup.decode_ready_event(&event).unwrap();
        assert_eq!(ready.mac_address_words, [0x4433_2211, 0x0000_6655]);
        assert_eq!(ready.extra_mac_addresses, &[0x5a; 16]);
        assert_eq!(ready.total_peers, 16);
        assert_eq!(ready.extra_peers, 2);

        let no_extra_macs = ready_event_fixture(0, 0, false);
        let ready = startup.decode_ready_event(&no_extra_macs).unwrap();
        assert!(ready.extra_mac_addresses.is_empty());
    }

    #[test]
    fn ready_decoder_rejects_bad_status_abi_mac_list_and_truncations() {
        let mut service_ready = fixture(&[], 0);
        set_sunfish_startup_identity(&mut service_ready, SUNFISH_WMI_HOST_ABI[1]);
        let startup = startup_evidence(&service_ready);

        let failed = ready_event_fixture(0, 7, false);
        assert_eq!(
            startup.decode_ready_event(&failed),
            Err(WmiReadyError::InitFailed { status: 7 })
        );

        let mut bad_abi = ready_event_fixture(0, 0, false);
        bad_abi[4..8].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        assert_eq!(
            startup.decode_ready_event(&bad_abi),
            Err(WmiReadyError::AbiMismatch {
                word_index: 0,
                expected: SUNFISH_WMI_HOST_ABI[0],
                observed: 0x0200_0000,
            })
        );

        let missing_list = ready_event_fixture(1, 0, false);
        assert_eq!(
            startup.decode_ready_event(&missing_list),
            Err(WmiReadyError::MacListMissing { count: 1 })
        );
        let mut wrong_list = ready_event_fixture(1, 0, true);
        wrong_list[64..66].copy_from_slice(&7u16.to_le_bytes());
        wrong_list.truncate(75);
        assert!(matches!(
            startup.decode_ready_event(&wrong_list),
            Err(WmiReadyError::MacListWrongLength { .. })
        ));
        let too_many = ready_event_fixture(MAX_READY_EXTRA_MACS + 1, 0, false);
        assert_eq!(
            startup.decode_ready_event(&too_many),
            Err(WmiReadyError::TooManyMacAddresses {
                count: MAX_READY_EXTRA_MACS + 1,
            })
        );

        for end in 0..wrong_list.len() {
            assert!(
                startup.decode_ready_event(&wrong_list[..end]).is_err(),
                "accepted ready-event truncation at byte {end} of {}",
                wrong_list.len()
            );
        }
    }
}
