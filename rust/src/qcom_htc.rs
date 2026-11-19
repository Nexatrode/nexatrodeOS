//! Bounded HTC control and WMI framing for the pinned Sunfish QCACLD path.
//!
//! Wire fields follow `drivers/staging/fw-api/fw/htc.h` and the WMI command
//! header/send path in `qca-wifi-host-cmn/wmi/src/wmi_unified.c` from the
//! LineageOS Sunfish kernel snapshot. This module is an offline codec and
//! startup state machine: it has no MMIO, DMA, IRQ, or transport ownership.

pub const HTC_FRAME_HEADER_SIZE: usize = 8;
pub const HTC_MAX_FRAME_PAYLOAD: usize = 4096 - HTC_FRAME_HEADER_SIZE;
pub const HTC_CONTROL_MESSAGE_MAX: usize = 256;
pub const HTC_SERVICE_METADATA_MAX: usize = 128;
pub const HTC_LOCAL_ENDPOINT_LIMIT: u8 = 9; // ENDPOINT_MAX sentinel; valid IDs are 0..8.
pub const HTC_CONTROL_ENDPOINT: u8 = 0;
pub const HTC_WMI_CONTROL_SERVICE_ID: u16 = 0x0100;
/// Connection-request bits that advertise host receive-buffer allocation.
/// QCACLD places its per-service allocation in bits 15:8 before sending the
/// WMI_CONTROL_SVC connection request.
pub const HTC_CONNECT_RECV_ALLOC_SHIFT: u16 = 8;
pub const HTC_WMI_COMMAND_HEADER_SIZE: usize = 4;
pub const HTC_FLAG_RECV_1MORE_BLOCK: u8 = 1 << 0;
pub const HTC_FLAG_RECV_TRAILER: u8 = 1 << 1;
pub const HTC_RECORD_HEADER_SIZE: usize = 4;
pub const HTC_RECORD_CREDITS: u8 = 1;
pub const HTC_CREDIT_REPORT_SIZE: usize = 4;

const HTC_READY_ID: u16 = 1;
const HTC_CONNECT_SERVICE_ID: u16 = 2;
const HTC_CONNECT_SERVICE_RESPONSE_ID: u16 = 3;
const HTC_SETUP_COMPLETE_EX_ID: u16 = 5;
const HTC_SERVICE_SUCCESS: u8 = 0;
const HTC_ALLOWED_CONNECT_FLAGS: u16 = 0xff1f;
pub const HTC_FLAG_NEED_CREDIT_UPDATE: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtcError {
    Truncated {
        expected: usize,
        observed: usize,
    },
    UnexpectedLength {
        expected: usize,
        observed: usize,
    },
    OutputTooSmall {
        required: usize,
        available: usize,
    },
    WrongMessageId {
        expected: u16,
        observed: u16,
    },
    WrongServiceId {
        expected: u16,
        observed: u16,
    },
    BadReservedBits,
    ZeroCreditCount,
    ZeroCreditSize,
    CreditAllocationTooLarge {
        observed: u16,
    },
    InvalidEndpointLimit {
        observed: u8,
    },
    UnsupportedHtcVersion {
        observed: u8,
    },
    InvalidConnectionFlags {
        observed: u16,
    },
    MetadataTooLong {
        observed: usize,
    },
    ServiceRejected {
        status: u8,
    },
    InvalidEndpoint {
        observed: u8,
    },
    InvalidMaxMessageSize {
        observed: usize,
    },
    InvalidFrameFlags {
        observed: u8,
    },
    InvalidFrameEndpoint {
        observed: u8,
    },
    InvalidFramePayloadLength {
        declared: usize,
        observed: usize,
    },
    EmptyWmiBody,
    InvalidWmiCommandId {
        observed: u32,
    },
    MessageTooLarge {
        limit: usize,
        observed: usize,
    },
    InsufficientCredits {
        required: u16,
        available: u16,
    },
    InvalidGeneration,
    WrongGeneration,
    InvalidCreditLedger,
    InvalidCreditPlan,
    InvalidCreditReport,
    CreditReportExceedsOutstanding {
        reported: u16,
        outstanding: u16,
    },
    CreditOverflow,
    InvalidTrailerLength {
        declared: usize,
        observed: usize,
    },
    InvalidTrailerRecord,
    InvalidState {
        expected: HtcPhase,
        observed: HtcPhase,
    },
    MissingReady,
    MissingConnection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcReady {
    pub credit_count: u16,
    pub credit_size: u16,
    /// Number of endpoint IDs the target exposes, including endpoint zero.
    pub max_endpoints: u8,
    pub htc_version: u8,
    /// The pinned driver clamps zero to one. Sunfish uses no HTC bundling.
    pub max_messages_per_bundle: u8,
    pub alternate_data_credit_size: u16,
}

impl HtcReady {
    /// The pinned Sunfish host initializes one WMI endpoint and allocates
    /// every target transmit credit to WMI_CONTROL_SVC. QCACLD stores that
    /// allocation in an eight-bit field, so refuse values it would narrow.
    pub fn initial_wmi_credit_allocation(self) -> Result<u8, HtcError> {
        u8::try_from(self.credit_count).map_err(|_| HtcError::CreditAllocationTooLarge {
            observed: self.credit_count,
        })
    }
}

pub fn parse_ready_ex(bytes: &[u8]) -> Result<HtcReady, HtcError> {
    const READY_EX_SIZE: usize = 12;
    if bytes.len() < READY_EX_SIZE {
        return Err(HtcError::Truncated {
            expected: READY_EX_SIZE,
            observed: bytes.len(),
        });
    }
    if bytes.len() != READY_EX_SIZE {
        return Err(HtcError::UnexpectedLength {
            expected: READY_EX_SIZE,
            observed: bytes.len(),
        });
    }

    let base = read_u32(bytes, 0);
    let message_id = base as u16;
    if message_id != HTC_READY_ID {
        return Err(HtcError::WrongMessageId {
            expected: HTC_READY_ID,
            observed: message_id,
        });
    }
    let credit_count = (base >> 16) as u16;
    if credit_count == 0 {
        return Err(HtcError::ZeroCreditCount);
    }

    let legacy = read_u32(bytes, 4);
    let credit_size = legacy as u16;
    let max_endpoints = (legacy >> 16) as u8;
    if ((legacy >> 24) as u8) != 0 {
        return Err(HtcError::BadReservedBits);
    }
    if credit_size == 0 {
        return Err(HtcError::ZeroCreditSize);
    }
    if !(2..=HTC_LOCAL_ENDPOINT_LIMIT).contains(&max_endpoints) {
        return Err(HtcError::InvalidEndpointLimit {
            observed: max_endpoints,
        });
    }

    let extended = read_u32(bytes, 8);
    let htc_version = extended as u8;
    if htc_version > 1 {
        return Err(HtcError::UnsupportedHtcVersion {
            observed: htc_version,
        });
    }
    if (extended >> 28) != 0 {
        return Err(HtcError::BadReservedBits);
    }

    let advertised_bundle_count = (extended >> 8) as u8;
    Ok(HtcReady {
        credit_count,
        credit_size,
        max_endpoints,
        htc_version,
        max_messages_per_bundle: advertised_bundle_count.max(1),
        alternate_data_credit_size: ((extended >> 16) & 0x0fff) as u16,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectServiceRequest<'a> {
    pub service_id: u16,
    pub connection_flags: u16,
    pub look_ahead_v2: bool,
    pub metadata: &'a [u8],
}

pub fn encode_connect_service_request(
    request: ConnectServiceRequest<'_>,
    output: &mut [u8],
) -> Result<usize, HtcError> {
    if request.service_id == 0 {
        return Err(HtcError::WrongServiceId {
            expected: HTC_WMI_CONTROL_SERVICE_ID,
            observed: 0,
        });
    }
    if (request.connection_flags & !HTC_ALLOWED_CONNECT_FLAGS) != 0 {
        return Err(HtcError::InvalidConnectionFlags {
            observed: request.connection_flags,
        });
    }
    if request.metadata.len() > HTC_SERVICE_METADATA_MAX {
        return Err(HtcError::MetadataTooLong {
            observed: request.metadata.len(),
        });
    }
    let required = 8 + request.metadata.len();
    if required > HTC_CONTROL_MESSAGE_MAX {
        return Err(HtcError::MessageTooLarge {
            limit: HTC_CONTROL_MESSAGE_MAX,
            observed: required,
        });
    }
    if output.len() < required {
        return Err(HtcError::OutputTooSmall {
            required,
            available: output.len(),
        });
    }

    write_u32(
        output,
        0,
        u32::from(HTC_CONNECT_SERVICE_ID) | (u32::from(request.service_id) << 16),
    );
    let flags_and_length = u32::from(request.connection_flags)
        | ((request.metadata.len() as u32) << 16)
        | (u32::from(request.look_ahead_v2) << 24);
    write_u32(output, 4, flags_and_length);
    output[8..required].copy_from_slice(request.metadata);
    Ok(required)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectServiceResponse<'a> {
    pub service_id: u16,
    pub endpoint_id: u8,
    pub max_message_size: u16,
    pub look_ahead_v2: bool,
    pub metadata: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WmiConnection {
    endpoint_id: u8,
    max_message_size: u16,
}

pub fn parse_connect_service_response<'a>(
    bytes: &'a [u8],
    expected_service_id: u16,
    ready: HtcReady,
) -> Result<ConnectServiceResponse<'a>, HtcError> {
    const RESPONSE_FIXED_SIZE: usize = 12;
    if bytes.len() < RESPONSE_FIXED_SIZE {
        return Err(HtcError::Truncated {
            expected: RESPONSE_FIXED_SIZE,
            observed: bytes.len(),
        });
    }

    let first = read_u32(bytes, 0);
    let message_id = first as u16;
    if message_id != HTC_CONNECT_SERVICE_RESPONSE_ID {
        return Err(HtcError::WrongMessageId {
            expected: HTC_CONNECT_SERVICE_RESPONSE_ID,
            observed: message_id,
        });
    }
    let service_id = (first >> 16) as u16;
    if service_id != expected_service_id {
        return Err(HtcError::WrongServiceId {
            expected: expected_service_id,
            observed: service_id,
        });
    }

    let endpoint_and_size = read_u32(bytes, 4);
    let status = endpoint_and_size as u8;
    if status != HTC_SERVICE_SUCCESS {
        return Err(HtcError::ServiceRejected { status });
    }
    let endpoint_id = (endpoint_and_size >> 8) as u8;
    let max_message_size = (endpoint_and_size >> 16) as u16;

    let metadata_and_flags = read_u32(bytes, 8);
    if (metadata_and_flags & 0xffff_fe00) != 0 {
        return Err(HtcError::BadReservedBits);
    }
    let metadata_len = (metadata_and_flags & 0xff) as usize;
    if metadata_len > HTC_SERVICE_METADATA_MAX {
        return Err(HtcError::MetadataTooLong {
            observed: metadata_len,
        });
    }
    let expected_len = RESPONSE_FIXED_SIZE + metadata_len;
    if bytes.len() != expected_len {
        return Err(HtcError::UnexpectedLength {
            expected: expected_len,
            observed: bytes.len(),
        });
    }

    let endpoint_limit = ready.max_endpoints.min(HTC_LOCAL_ENDPOINT_LIMIT);
    if endpoint_id == HTC_CONTROL_ENDPOINT || endpoint_id >= endpoint_limit {
        return Err(HtcError::InvalidEndpoint {
            observed: endpoint_id,
        });
    }
    let max_message_size = usize::from(max_message_size);
    if max_message_size == 0 || max_message_size > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::InvalidMaxMessageSize {
            observed: max_message_size,
        });
    }

    Ok(ConnectServiceResponse {
        service_id,
        endpoint_id,
        max_message_size: max_message_size as u16,
        look_ahead_v2: ((metadata_and_flags >> 8) & 1) != 0,
        metadata: &bytes[RESPONSE_FIXED_SIZE..],
    })
}

pub fn encode_setup_complete_ex(output: &mut [u8]) -> Result<usize, HtcError> {
    const SETUP_COMPLETE_EX_SIZE: usize = 12;
    if output.len() < SETUP_COMPLETE_EX_SIZE {
        return Err(HtcError::OutputTooSmall {
            required: SETUP_COMPLETE_EX_SIZE,
            available: output.len(),
        });
    }
    // The pinned driver's zero-initialized SetupCompleteEx enables neither
    // receive bundling nor disabled credit flow on this non-SDIO path.
    write_u32(output, 0, u32::from(HTC_SETUP_COMPLETE_EX_ID));
    write_u32(output, 4, 0);
    write_u32(output, 8, 0);
    Ok(SETUP_COMPLETE_EX_SIZE)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcFrame<'a> {
    pub endpoint_id: u8,
    pub flags: u8,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiFrame<'a> {
    pub endpoint_id: u8,
    pub command_id: u32,
    pub tlv_body: &'a [u8],
    credit_trailer: &'a [u8],
    credit_returns: HtcCreditReportTotals,
}

impl<'a> WmiFrame<'a> {
    /// Iterate credit reports after the caller validates this WMI frame.
    /// Reports are borrowed from HTC's optional RX trailer.
    pub fn credit_reports(self) -> HtcCreditReportIter<'a> {
        HtcCreditReportIter::new(self.credit_trailer)
    }

    pub const fn credit_returns(self) -> HtcCreditReportTotals {
        self.credit_returns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiTxFrame<'a> {
    pub endpoint_id: u8,
    pub credit_update_requested: bool,
    pub command_id: u32,
    pub tlv_body: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcCreditReport {
    pub endpoint_id: u8,
    pub credits: u8,
}

/// Bounded aggregate of credit reports in one validated HTC receive trailer.
/// Duplicate records for an endpoint are combined so receive pipelines can
/// carry the result without retaining a borrow into their scratch buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcCreditReportTotals {
    credits_by_endpoint: [u16; HTC_LOCAL_ENDPOINT_LIMIT as usize],
}

impl HtcCreditReportTotals {
    const fn empty() -> Self {
        Self {
            credits_by_endpoint: [0; HTC_LOCAL_ENDPOINT_LIMIT as usize],
        }
    }

    pub const fn for_endpoint(self, endpoint_id: u8) -> u16 {
        if endpoint_id < HTC_LOCAL_ENDPOINT_LIMIT {
            self.credits_by_endpoint[endpoint_id as usize]
        } else {
            0
        }
    }
}

/// Iterator over credit records validated by `parse_rx_frame`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcCreditReportIter<'a> {
    trailer: &'a [u8],
    offset: usize,
    report_offset: usize,
    reports_remaining: usize,
}

impl<'a> HtcCreditReportIter<'a> {
    fn new(trailer: &'a [u8]) -> Self {
        Self {
            trailer,
            offset: 0,
            report_offset: 0,
            reports_remaining: 0,
        }
    }
}

impl Iterator for HtcCreditReportIter<'_> {
    type Item = HtcCreditReport;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.reports_remaining != 0 {
                let offset = self.report_offset;
                self.report_offset += HTC_CREDIT_REPORT_SIZE;
                self.reports_remaining -= 1;
                return Some(HtcCreditReport {
                    endpoint_id: self.trailer[offset],
                    credits: self.trailer[offset + 1],
                });
            }
            if self.offset == self.trailer.len() {
                return None;
            }
            let record_id = self.trailer[self.offset];
            let record_bytes = usize::from(self.trailer[self.offset + 1]);
            let value_start = self.offset + HTC_RECORD_HEADER_SIZE;
            self.offset = value_start + record_bytes;
            if record_id == HTC_RECORD_CREDITS {
                self.report_offset = value_start;
                self.reports_remaining = record_bytes / HTC_CREDIT_REPORT_SIZE;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcRxFrame<'a> {
    pub endpoint_id: u8,
    pub control_bytes1: u8,
    pub payload: &'a [u8],
    credit_trailer: &'a [u8],
    credit_returns: HtcCreditReportTotals,
}

impl<'a> HtcRxFrame<'a> {
    pub const fn credit_returns(self) -> HtcCreditReportTotals {
        self.credit_returns
    }
}

/// Validated single-WMI-endpoint HTC credit state for one boot generation.
/// Credits are reserved at command publication and returned only by parsed
/// HTC credit reports for the dynamically assigned WMI endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcCreditLedger {
    generation: u64,
    endpoint_id: u8,
    total: u16,
    available: u16,
    in_flight: u16,
    credit_size: u16,
    max_message_size: u16,
}

impl HtcCreditLedger {
    pub fn new(
        generation: u64,
        endpoint_id: u8,
        total_credits: u16,
        credit_size: u16,
        max_message_size: u16,
    ) -> Result<Self, HtcError> {
        if generation == 0 {
            return Err(HtcError::InvalidGeneration);
        }
        if endpoint_id == HTC_CONTROL_ENDPOINT
            || endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT
            || total_credits == 0
            || total_credits > u16::from(u8::MAX)
            || credit_size == 0
            || max_message_size == 0
            || usize::from(max_message_size) > HTC_MAX_FRAME_PAYLOAD
        {
            return Err(HtcError::InvalidCreditLedger);
        }
        Ok(Self {
            generation,
            endpoint_id,
            total: total_credits,
            available: total_credits,
            in_flight: 0,
            credit_size,
            max_message_size,
        })
    }

    pub const fn endpoint_id(self) -> u8 {
        self.endpoint_id
    }

    pub const fn available(self) -> u16 {
        self.available
    }

    pub const fn in_flight(self) -> u16 {
        self.in_flight
    }

    pub const fn credit_size(self) -> u16 {
        self.credit_size
    }

    pub const fn max_message_size(self) -> u16 {
        self.max_message_size
    }

    /// Reserve credits computed by the encoder from this ledger's exact
    /// available count and the negotiated HTC message/credit geometry.
    pub fn reserve(&mut self, generation: u64, plan: WmiTxPlan) -> Result<(), HtcError> {
        if generation == 0 {
            return Err(HtcError::InvalidGeneration);
        }
        if generation != self.generation {
            return Err(HtcError::WrongGeneration);
        }
        let encoded_available = plan
            .credits_required
            .checked_add(plan.credits_remaining)
            .ok_or(HtcError::InvalidCreditPlan)?;
        let required = u16::try_from(div_ceil(plan.frame_len, usize::from(self.credit_size)))
            .map_err(|_| HtcError::InvalidCreditPlan)?;
        let credits_per_max_message = div_ceil(
            usize::from(self.max_message_size),
            usize::from(self.credit_size),
        );
        if plan.frame_len < HTC_FRAME_HEADER_SIZE
            || plan.credits_required != required
            || encoded_available != self.available
            || plan.credits_remaining != self.available.saturating_sub(required)
            || plan.requested_credit_update
                != (usize::from(plan.credits_remaining) <= credits_per_max_message)
        {
            return Err(HtcError::InvalidCreditPlan);
        }
        if required > self.available {
            return Err(HtcError::InsufficientCredits {
                required,
                available: self.available,
            });
        }
        self.available -= required;
        self.in_flight = self
            .in_flight
            .checked_add(required)
            .ok_or(HtcError::CreditOverflow)?;
        Ok(())
    }

    /// Return a reservation only when CE publication is known not to have
    /// occurred. Uncertain publications must keep their credits reserved.
    pub fn cancel_unpublished(&mut self, generation: u64, credits: u16) -> Result<(), HtcError> {
        if generation == 0 {
            return Err(HtcError::InvalidGeneration);
        }
        if generation != self.generation {
            return Err(HtcError::WrongGeneration);
        }
        if credits == 0 || credits > self.in_flight {
            return Err(HtcError::InvalidCreditPlan);
        }
        self.in_flight -= credits;
        self.available = self
            .available
            .checked_add(credits)
            .filter(|available| *available <= self.total)
            .ok_or(HtcError::CreditOverflow)?;
        Ok(())
    }

    /// Apply a fully validated HTC trailer transactionally. Reports for
    /// services outside this ledger are ignored; a report cannot restore more
    /// WMI credits than were previously reserved.
    pub fn apply_reports(
        &mut self,
        generation: u64,
        reports: HtcCreditReportIter<'_>,
    ) -> Result<u16, HtcError> {
        let mut credit_returns = HtcCreditReportTotals::empty();
        for report in reports {
            if report.endpoint_id < HTC_LOCAL_ENDPOINT_LIMIT {
                let slot = &mut credit_returns.credits_by_endpoint[report.endpoint_id as usize];
                *slot = slot
                    .checked_add(u16::from(report.credits))
                    .ok_or(HtcError::CreditOverflow)?;
            }
        }
        self.apply_credit_returns(generation, credit_returns)
    }

    /// Apply a validated aggregate of trailer reports transactionally.
    pub fn apply_credit_returns(
        &mut self,
        generation: u64,
        credit_returns: HtcCreditReportTotals,
    ) -> Result<u16, HtcError> {
        if generation == 0 {
            return Err(HtcError::InvalidGeneration);
        }
        if generation != self.generation {
            return Err(HtcError::WrongGeneration);
        }
        let returned = credit_returns.for_endpoint(self.endpoint_id);
        if returned > self.in_flight {
            return Err(HtcError::CreditReportExceedsOutstanding {
                reported: returned,
                outstanding: self.in_flight,
            });
        }
        let available = self
            .available
            .checked_add(returned)
            .filter(|available| *available <= self.total)
            .ok_or(HtcError::CreditOverflow)?;
        self.in_flight -= returned;
        self.available = available;
        Ok(returned)
    }
}

fn validate_credit_trailer(trailer: &[u8]) -> Result<HtcCreditReportTotals, HtcError> {
    let mut offset = 0usize;
    let mut credit_returns = HtcCreditReportTotals::empty();
    while offset < trailer.len() {
        let remaining = trailer.len() - offset;
        if remaining < HTC_RECORD_HEADER_SIZE {
            return Err(HtcError::InvalidTrailerRecord);
        }
        let record_id = trailer[offset];
        let record_bytes = usize::from(trailer[offset + 1]);
        if trailer[offset + 2] != 0 || trailer[offset + 3] != 0 {
            return Err(HtcError::BadReservedBits);
        }
        let value_start = offset + HTC_RECORD_HEADER_SIZE;
        let record_end = value_start
            .checked_add(record_bytes)
            .ok_or(HtcError::InvalidTrailerRecord)?;
        if record_end > trailer.len() {
            return Err(HtcError::InvalidTrailerRecord);
        }
        if record_id == HTC_RECORD_CREDITS {
            if record_bytes == 0 || !record_bytes.is_multiple_of(HTC_CREDIT_REPORT_SIZE) {
                return Err(HtcError::InvalidTrailerRecord);
            }
            for report in trailer[value_start..record_end].chunks_exact(HTC_CREDIT_REPORT_SIZE) {
                if report[0] >= HTC_LOCAL_ENDPOINT_LIMIT || report[2] != 0 || report[3] != 0 {
                    return Err(HtcError::InvalidCreditReport);
                }
                let slot = &mut credit_returns.credits_by_endpoint[report[0] as usize];
                *slot = slot
                    .checked_add(u16::from(report[1]))
                    .ok_or(HtcError::CreditOverflow)?;
            }
        }
        offset = record_end;
    }
    Ok(credit_returns)
}

/// Split an inbound non-bundled HTC frame into application bytes and its
/// optional validated trailer. Receive-direction flag bit 0 denotes another
/// bundle block, not a request for credits; that mode remains unsupported.
pub fn parse_rx_frame(
    bytes: &[u8],
    expected_endpoint_id: u8,
    max_message_size: usize,
) -> Result<HtcRxFrame<'_>, HtcError> {
    parse_rx_frame_inner(bytes, expected_endpoint_id, max_message_size, false)
}

/// Parse an HTC data frame whose ControlBytes1 carries its sequence number.
/// Receive bundle flags remain unsupported; trailer records are validated and
/// returned as both a borrowed iterator and bounded per-endpoint totals.
pub fn parse_rx_data_frame(
    bytes: &[u8],
    expected_endpoint_id: u8,
    max_message_size: usize,
) -> Result<HtcRxFrame<'_>, HtcError> {
    parse_rx_frame_inner(bytes, expected_endpoint_id, max_message_size, true)
}

fn parse_rx_frame_inner(
    bytes: &[u8],
    expected_endpoint_id: u8,
    max_message_size: usize,
    allow_sequence: bool,
) -> Result<HtcRxFrame<'_>, HtcError> {
    if bytes.len() < HTC_FRAME_HEADER_SIZE {
        return Err(HtcError::Truncated {
            expected: HTC_FRAME_HEADER_SIZE,
            observed: bytes.len(),
        });
    }
    if expected_endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT {
        return Err(HtcError::InvalidEndpoint {
            observed: expected_endpoint_id,
        });
    }
    if max_message_size > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::InvalidMaxMessageSize {
            observed: max_message_size,
        });
    }
    let first = read_u32(bytes, 0);
    let endpoint_id = first as u8;
    if endpoint_id != expected_endpoint_id {
        return Err(HtcError::InvalidFrameEndpoint {
            observed: endpoint_id,
        });
    }
    let flags = (first >> 8) as u8;
    if flags & !HTC_FLAG_RECV_TRAILER != 0 {
        return Err(HtcError::InvalidFrameFlags { observed: flags });
    }
    let declared = (first >> 16) as usize;
    let trailer_bytes = if flags & HTC_FLAG_RECV_TRAILER != 0 {
        usize::from(bytes[4])
    } else {
        0
    };
    if (!allow_sequence && bytes[5] != 0) || bytes[6] != 0 || bytes[7] != 0 {
        return Err(HtcError::BadReservedBits);
    }
    if declared > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::MessageTooLarge {
            limit: HTC_MAX_FRAME_PAYLOAD,
            observed: declared,
        });
    }
    if flags & HTC_FLAG_RECV_TRAILER != 0
        && (trailer_bytes < HTC_RECORD_HEADER_SIZE || trailer_bytes > declared)
    {
        return Err(HtcError::InvalidTrailerLength {
            declared: trailer_bytes,
            observed: declared,
        });
    }
    let payload_bytes = declared - trailer_bytes;
    if payload_bytes > max_message_size {
        return Err(HtcError::MessageTooLarge {
            limit: max_message_size,
            observed: payload_bytes,
        });
    }
    if bytes.len() != HTC_FRAME_HEADER_SIZE + declared {
        return Err(HtcError::InvalidFramePayloadLength {
            declared,
            observed: bytes.len().saturating_sub(HTC_FRAME_HEADER_SIZE),
        });
    }
    let payload_end = HTC_FRAME_HEADER_SIZE + payload_bytes;
    let trailer = &bytes[payload_end..HTC_FRAME_HEADER_SIZE + declared];
    let credit_returns = validate_credit_trailer(trailer)?;
    Ok(HtcRxFrame {
        endpoint_id,
        control_bytes1: if allow_sequence { bytes[5] } else { 0 },
        payload: &bytes[HTC_FRAME_HEADER_SIZE..payload_end],
        credit_trailer: trailer,
        credit_returns,
    })
}

/// Parses the no-bundle/no-trailer HTC frame profile used by the initial
/// Sunfish control and WMI path. Other HTC flag modes fail closed.
pub fn parse_plain_frame(bytes: &[u8]) -> Result<HtcFrame<'_>, HtcError> {
    if bytes.len() < HTC_FRAME_HEADER_SIZE {
        return Err(HtcError::Truncated {
            expected: HTC_FRAME_HEADER_SIZE,
            observed: bytes.len(),
        });
    }
    let first = read_u32(bytes, 0);
    let endpoint_id = first as u8;
    let flags = (first >> 8) as u8;
    let declared = (first >> 16) as usize;
    if endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT {
        return Err(HtcError::InvalidFrameEndpoint {
            observed: endpoint_id,
        });
    }
    if flags != 0 {
        return Err(HtcError::InvalidFrameFlags { observed: flags });
    }
    if read_u32(bytes, 4) != 0 {
        return Err(HtcError::BadReservedBits);
    }
    if declared > HTC_MAX_FRAME_PAYLOAD || bytes.len() != HTC_FRAME_HEADER_SIZE + declared {
        return Err(HtcError::InvalidFramePayloadLength {
            declared,
            observed: bytes.len().saturating_sub(HTC_FRAME_HEADER_SIZE),
        });
    }
    Ok(HtcFrame {
        endpoint_id,
        flags,
        payload: &bytes[HTC_FRAME_HEADER_SIZE..],
    })
}

/// Parse one non-bundled WMI HTC receive frame. A source-valid credit trailer
/// is separated from the WMI payload and exposed through `credit_reports()`.
/// HTC receive bundles and command-header high bits remain unsupported.
pub fn parse_wmi_frame(
    bytes: &[u8],
    expected_endpoint_id: u8,
    max_message_size: usize,
) -> Result<WmiFrame<'_>, HtcError> {
    if expected_endpoint_id == HTC_CONTROL_ENDPOINT
        || expected_endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT
    {
        return Err(HtcError::InvalidEndpoint {
            observed: expected_endpoint_id,
        });
    }
    if max_message_size == 0 || max_message_size > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::InvalidMaxMessageSize {
            observed: max_message_size,
        });
    }
    let rx = parse_rx_frame(bytes, expected_endpoint_id, max_message_size)?;
    if rx.payload.len() < HTC_WMI_COMMAND_HEADER_SIZE {
        return Err(HtcError::Truncated {
            expected: HTC_WMI_COMMAND_HEADER_SIZE,
            observed: rx.payload.len(),
        });
    }
    let command_word = read_u32(rx.payload, 0);
    if command_word & !0x00ff_ffff != 0 {
        return Err(HtcError::InvalidWmiCommandId {
            observed: command_word,
        });
    }
    if rx.payload.len() == HTC_WMI_COMMAND_HEADER_SIZE {
        return Err(HtcError::EmptyWmiBody);
    }
    Ok(WmiFrame {
        endpoint_id: rx.endpoint_id,
        command_id: command_word,
        tlv_body: &rx.payload[HTC_WMI_COMMAND_HEADER_SIZE..],
        credit_trailer: rx.credit_trailer,
        credit_returns: rx.credit_returns,
    })
}

/// Parse a host-to-target WMI frame for source audits and offline transport
/// validation. The credit-update bit has its send-direction meaning here;
/// receive frames must use `parse_wmi_frame` and HTC's trailer semantics.
pub fn parse_wmi_tx_frame(
    bytes: &[u8],
    expected_endpoint_id: u8,
    max_message_size: usize,
) -> Result<WmiTxFrame<'_>, HtcError> {
    if bytes.len() < HTC_FRAME_HEADER_SIZE {
        return Err(HtcError::Truncated {
            expected: HTC_FRAME_HEADER_SIZE,
            observed: bytes.len(),
        });
    }
    if expected_endpoint_id == HTC_CONTROL_ENDPOINT
        || expected_endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT
    {
        return Err(HtcError::InvalidEndpoint {
            observed: expected_endpoint_id,
        });
    }
    if max_message_size == 0 || max_message_size > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::InvalidMaxMessageSize {
            observed: max_message_size,
        });
    }
    let first = read_u32(bytes, 0);
    let endpoint_id = first as u8;
    if endpoint_id != expected_endpoint_id {
        return Err(HtcError::InvalidFrameEndpoint {
            observed: endpoint_id,
        });
    }
    let flags = (first >> 8) as u8;
    if flags & !HTC_FLAG_NEED_CREDIT_UPDATE != 0 {
        return Err(HtcError::InvalidFrameFlags { observed: flags });
    }
    let declared = (first >> 16) as usize;
    if read_u32(bytes, 4) != 0 {
        return Err(HtcError::BadReservedBits);
    }
    if declared > max_message_size {
        return Err(HtcError::MessageTooLarge {
            limit: max_message_size,
            observed: declared,
        });
    }
    if bytes.len() != HTC_FRAME_HEADER_SIZE + declared {
        return Err(HtcError::InvalidFramePayloadLength {
            declared,
            observed: bytes.len().saturating_sub(HTC_FRAME_HEADER_SIZE),
        });
    }
    if declared < HTC_WMI_COMMAND_HEADER_SIZE {
        return Err(HtcError::Truncated {
            expected: HTC_WMI_COMMAND_HEADER_SIZE,
            observed: declared,
        });
    }
    let payload = &bytes[HTC_FRAME_HEADER_SIZE..];
    let command_id = read_u32(payload, 0);
    if command_id & !0x00ff_ffff != 0 {
        return Err(HtcError::InvalidWmiCommandId {
            observed: command_id,
        });
    }
    if declared == HTC_WMI_COMMAND_HEADER_SIZE {
        return Err(HtcError::EmptyWmiBody);
    }
    Ok(WmiTxFrame {
        endpoint_id,
        credit_update_requested: flags & HTC_FLAG_NEED_CREDIT_UPDATE != 0,
        command_id,
        tlv_body: &payload[HTC_WMI_COMMAND_HEADER_SIZE..],
    })
}

pub fn encode_control_frame(payload: &[u8], output: &mut [u8]) -> Result<usize, HtcError> {
    encode_frame(HTC_CONTROL_ENDPOINT, 0, payload, output)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiTxPlan {
    pub frame_len: usize,
    pub wmi_payload_len: usize,
    pub credits_required: u16,
    pub credits_remaining: u16,
    pub requested_credit_update: bool,
}

/// Encodes one WMI command inside one HTC frame and calculates the exact
/// credit reservation used by QCACLD (`HTC header + packet length`).
/// Callers supply credits from a separately validated allocation ledger.
pub fn encode_wmi_command_frame(
    endpoint_id: u8,
    max_message_size: usize,
    credit_size: u16,
    available_credits: u16,
    command_id: u32,
    tlv_body: &[u8],
    output: &mut [u8],
) -> Result<WmiTxPlan, HtcError> {
    if endpoint_id == HTC_CONTROL_ENDPOINT || endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT {
        return Err(HtcError::InvalidEndpoint {
            observed: endpoint_id,
        });
    }
    if command_id > 0x00ff_ffff {
        return Err(HtcError::InvalidWmiCommandId {
            observed: command_id,
        });
    }
    if tlv_body.is_empty() {
        return Err(HtcError::EmptyWmiBody);
    }
    let wmi_payload_len = HTC_WMI_COMMAND_HEADER_SIZE
        .checked_add(tlv_body.len())
        .ok_or(HtcError::MessageTooLarge {
            limit: max_message_size,
            observed: usize::MAX,
        })?;
    if max_message_size == 0 || max_message_size > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::InvalidMaxMessageSize {
            observed: max_message_size,
        });
    }
    if wmi_payload_len > max_message_size || wmi_payload_len > HTC_MAX_FRAME_PAYLOAD {
        return Err(HtcError::MessageTooLarge {
            limit: max_message_size.min(HTC_MAX_FRAME_PAYLOAD),
            observed: wmi_payload_len,
        });
    }
    if credit_size == 0 {
        return Err(HtcError::ZeroCreditSize);
    }
    let frame_len = HTC_FRAME_HEADER_SIZE + wmi_payload_len;
    if output.len() < frame_len {
        return Err(HtcError::OutputTooSmall {
            required: frame_len,
            available: output.len(),
        });
    }

    let credits_required_usize = div_ceil(frame_len, usize::from(credit_size));
    let credits_required =
        u16::try_from(credits_required_usize).map_err(|_| HtcError::MessageTooLarge {
            limit: usize::from(u16::MAX) * usize::from(credit_size),
            observed: frame_len,
        })?;
    if credits_required > available_credits {
        return Err(HtcError::InsufficientCredits {
            required: credits_required,
            available: available_credits,
        });
    }
    let credits_remaining = available_credits - credits_required;
    let credits_per_max_message = div_ceil(max_message_size, usize::from(credit_size));
    let requested_credit_update = usize::from(credits_remaining) <= credits_per_max_message;
    let flags = if requested_credit_update {
        HTC_FLAG_NEED_CREDIT_UPDATE
    } else {
        0
    };
    let first = u32::from(endpoint_id) | (u32::from(flags) << 8) | ((wmi_payload_len as u32) << 16);
    write_u32(output, 0, first);
    write_u32(output, 4, 0);
    write_u32(output, HTC_FRAME_HEADER_SIZE, command_id);
    output[HTC_FRAME_HEADER_SIZE + HTC_WMI_COMMAND_HEADER_SIZE..frame_len]
        .copy_from_slice(tlv_body);

    Ok(WmiTxPlan {
        frame_len,
        wmi_payload_len,
        credits_required,
        credits_remaining,
        requested_credit_update,
    })
}

fn encode_frame(
    endpoint_id: u8,
    flags: u8,
    payload: &[u8],
    output: &mut [u8],
) -> Result<usize, HtcError> {
    if endpoint_id >= HTC_LOCAL_ENDPOINT_LIMIT {
        return Err(HtcError::InvalidFrameEndpoint {
            observed: endpoint_id,
        });
    }
    if flags != 0 {
        return Err(HtcError::InvalidFrameFlags { observed: flags });
    }
    if payload.len() > HTC_MAX_FRAME_PAYLOAD || payload.len() > usize::from(u16::MAX) {
        return Err(HtcError::MessageTooLarge {
            limit: HTC_MAX_FRAME_PAYLOAD,
            observed: payload.len(),
        });
    }
    let required = HTC_FRAME_HEADER_SIZE + payload.len();
    if output.len() < required {
        return Err(HtcError::OutputTooSmall {
            required,
            available: output.len(),
        });
    }
    let first = u32::from(endpoint_id) | ((payload.len() as u32) << 16);
    write_u32(output, 0, first);
    write_u32(output, 4, 0);
    output[HTC_FRAME_HEADER_SIZE..required].copy_from_slice(payload);
    Ok(required)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtcPhase {
    AwaitReady,
    Ready,
    ConnectStaged,
    AwaitConnectResponse,
    ServiceConnected,
    SetupStaged,
    Started,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcStartup {
    phase: HtcPhase,
    ready: Option<HtcReady>,
    wmi_connection: Option<WmiConnection>,
}

impl HtcStartup {
    pub const fn new() -> Self {
        Self {
            phase: HtcPhase::AwaitReady,
            ready: None,
            wmi_connection: None,
        }
    }

    pub const fn phase(&self) -> HtcPhase {
        self.phase
    }

    pub fn initial_wmi_credit_allocation(&self) -> Result<u8, HtcError> {
        self.ready
            .ok_or(HtcError::MissingReady)?
            .initial_wmi_credit_allocation()
    }

    /// Create the generation-bound credit state after the negotiated WMI
    /// endpoint has completed HTC setup. Sunfish's pinned QCACLD target uses
    /// one WMI endpoint, so it assigns the target's complete credit count to
    /// that endpoint.
    pub fn wmi_credit_ledger(&self, generation: u64) -> Result<HtcCreditLedger, HtcError> {
        self.require_phase(HtcPhase::Started)?;
        let ready = self.ready.ok_or(HtcError::MissingReady)?;
        let connection = self.wmi_connection.ok_or(HtcError::MissingConnection)?;
        let credits = ready.initial_wmi_credit_allocation()?;
        HtcCreditLedger::new(
            generation,
            connection.endpoint_id,
            u16::from(credits),
            ready.credit_size,
            connection.max_message_size,
        )
    }

    pub fn accept_ready(&mut self, payload: &[u8]) -> Result<HtcReady, HtcError> {
        self.require_phase(HtcPhase::AwaitReady)?;
        let ready = parse_ready_ex(payload)?;
        self.ready = Some(ready);
        self.phase = HtcPhase::Ready;
        Ok(ready)
    }

    pub fn accept_ready_frame(&mut self, frame: &[u8]) -> Result<HtcReady, HtcError> {
        let parsed = parse_plain_frame(frame)?;
        if parsed.endpoint_id != HTC_CONTROL_ENDPOINT {
            return Err(HtcError::InvalidFrameEndpoint {
                observed: parsed.endpoint_id,
            });
        }
        self.accept_ready(parsed.payload)
    }

    /// Stage the single WMI_CONTROL_SVC connect request. The phase advances
    /// only after the caller reports successful control-endpoint submission.
    pub fn stage_wmi_connect(&mut self, output: &mut [u8]) -> Result<usize, HtcError> {
        self.require_phase(HtcPhase::Ready)?;
        let receive_allocation = self.initial_wmi_credit_allocation()?;
        let written = encode_connect_service_request(
            ConnectServiceRequest {
                service_id: HTC_WMI_CONTROL_SERVICE_ID,
                // htc_connect_service() replaces HTC_SET_RECV_ALLOC_MASK
                // with htc_get_credit_allocation(). Sunfish has one WMI
                // endpoint, so QCACLD assigns all READY credits to it.
                connection_flags: u16::from(receive_allocation) << HTC_CONNECT_RECV_ALLOC_SHIFT,
                look_ahead_v2: false,
                metadata: &[],
            },
            output,
        )?;
        self.phase = HtcPhase::ConnectStaged;
        Ok(written)
    }

    pub fn accept_connect_response<'a>(
        &mut self,
        payload: &'a [u8],
    ) -> Result<ConnectServiceResponse<'a>, HtcError> {
        self.require_phase(HtcPhase::AwaitConnectResponse)?;
        let ready = self.ready.ok_or(HtcError::MissingReady)?;
        let response = parse_connect_service_response(payload, HTC_WMI_CONTROL_SERVICE_ID, ready)?;
        self.wmi_connection = Some(WmiConnection {
            endpoint_id: response.endpoint_id,
            max_message_size: response.max_message_size,
        });
        self.phase = HtcPhase::ServiceConnected;
        Ok(response)
    }

    pub fn accept_connect_response_frame<'a>(
        &mut self,
        frame: &'a [u8],
    ) -> Result<ConnectServiceResponse<'a>, HtcError> {
        let parsed = parse_plain_frame(frame)?;
        if parsed.endpoint_id != HTC_CONTROL_ENDPOINT {
            return Err(HtcError::InvalidFrameEndpoint {
                observed: parsed.endpoint_id,
            });
        }
        self.accept_connect_response(parsed.payload)
    }

    pub fn stage_setup_complete(&mut self, output: &mut [u8]) -> Result<usize, HtcError> {
        self.require_phase(HtcPhase::ServiceConnected)?;
        let written = encode_setup_complete_ex(output)?;
        self.phase = HtcPhase::SetupStaged;
        Ok(written)
    }

    /// Commit one staged control message after its transport accepted it.
    pub fn control_tx_submitted(&mut self) -> Result<HtcPhase, HtcError> {
        self.phase = match self.phase {
            HtcPhase::ConnectStaged => HtcPhase::AwaitConnectResponse,
            HtcPhase::SetupStaged => HtcPhase::Started,
            observed => {
                return Err(HtcError::InvalidState {
                    expected: HtcPhase::ConnectStaged,
                    observed,
                });
            }
        };
        Ok(self.phase)
    }

    /// Discard a staged message when transport submission failed; no retry is
    /// performed by this codec and the caller controls whether to try again.
    pub fn control_tx_failed(&mut self) -> Result<HtcPhase, HtcError> {
        self.phase = match self.phase {
            HtcPhase::ConnectStaged => HtcPhase::Ready,
            HtcPhase::SetupStaged => HtcPhase::ServiceConnected,
            observed => {
                return Err(HtcError::InvalidState {
                    expected: HtcPhase::ConnectStaged,
                    observed,
                });
            }
        };
        Ok(self.phase)
    }

    pub fn encode_wmi_command_frame(
        &self,
        credit_size: u16,
        available_credits: u16,
        command_id: u32,
        tlv_body: &[u8],
        output: &mut [u8],
    ) -> Result<WmiTxPlan, HtcError> {
        self.require_phase(HtcPhase::Started)?;
        let connection = self.wmi_connection.ok_or(HtcError::MissingConnection)?;
        encode_wmi_command_frame(
            connection.endpoint_id,
            usize::from(connection.max_message_size),
            credit_size,
            available_credits,
            command_id,
            tlv_body,
            output,
        )
    }

    fn require_phase(&self, expected: HtcPhase) -> Result<(), HtcError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(HtcError::InvalidState {
                expected,
                observed: self.phase,
            })
        }
    }
}

impl Default for HtcStartup {
    fn default() -> Self {
        Self::new()
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn div_ceil(numerator: usize, denominator: usize) -> usize {
    numerator / denominator + usize::from(numerator % denominator != 0)
}
