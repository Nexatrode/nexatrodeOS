//! Host-testable WCN3990 Copy Engine descriptor and ring primitives.
//!
//! This module does not access MMIO, allocate DMA memory, or start WLAN. It
//! captures the descriptor representation and one-empty-slot ring rule used by
//! the upstream ath10k WCN3990 path so those details can be checked before a
//! Microkit device boundary exists.

const WCN3990_IOVA_LIMIT: u64 = 1 << 35;
const WCN3990_GATHER_ADDRESS_FLAG: u32 = 1 << 31;
const CE_DESC_GATHER_FLAG: u16 = 1 << 0;
const CE_DESC_BYTE_SWAP_FLAG: u16 = 1 << 1;
const HTC_FLAG_TRAILER_PRESENT: u8 = 1 << 1;
const HTC_READY_MESSAGE_ID: u16 = 1;
const HTC_CONNECT_REQUEST_MESSAGE_ID: u16 = 2;
const HTC_CONNECT_RESPONSE_MESSAGE_ID: u16 = 3;
const HTC_SETUP_COMPLETE_EXT_MESSAGE_ID: u16 = 5;
const HTC_SEND_SUSPEND_COMPLETE_MESSAGE_ID: u16 = 6;
pub const HTC_WMI_CONTROL_SERVICE_ID: u16 = 0x0100;
const HTC_HTT_DATA_SERVICE_ID: u16 = 0x0300;
const HTC_CREDIT_RECORD_ID: u8 = 1;
const HTC_NEED_CREDIT_UPDATE: u8 = 1 << 0;
const HTC_MAX_FRAME_LEN: usize = 4096;
const HTC_MAX_RX_BUNDLE: u8 = 32;
const HTC_ENDPOINT_COUNT: u8 = 9;
const HTC_CONNECT_DISABLE_CREDIT_FLOW: u16 = 1 << 3;
const HTC_CONNECT_RECEIVE_ALLOC_SHIFT: u16 = 8;
const HTC_SETUP_COMPLETE_RX_BUNDLE_FLAG: u32 = 1;
const WCN3990_METADATA_SHIFT: u32 = 4;
const WCN3990_METADATA_MAX: u16 = 0x0fff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CeError {
    AddressOutOfRange,
    TransferTooLong,
    TransferIdOutOfRange,
    HtcHeaderTruncated,
    HtcPayloadTruncated,
    HtcPayloadTooLong,
    HtcTrailerMissing,
    HtcTrailerTooShort,
    HtcTrailerTooLong,
    HtcRecordHeaderTruncated,
    HtcRecordPayloadTruncated,
    HtcControlMessageTruncated,
    HtcUnexpectedMessageId,
    HtcInvalidReady,
    HtcServiceIdMismatch,
    HtcServiceConnectFailed,
    HtcEndpointOutOfRange,
    HtcEndpointInUse,
    HtcEndpointNotConnected,
    HtcEndpointServiceMismatch,
    HtcMessageSizeZero,
    InvalidRingSize,
    RingFull,
    RingEmpty,
    InvalidHardwareIndex,
    CompletionBeyondPosted,
    CompletionOutputTooSmall,
    RxBufferTooSmall,
    RxBufferAlreadyPosted,
    TxTokenAlreadyPosted,
    RxCompletionDuplicate,
    RxCompletionNotReady,
    EventCountOverflow,
    HtcCreditSizeZero,
    HtcEndpointNotConfigured,
    HtcInsufficientCredits,
    HtcCreditOverflow,
    HtcCreditCountTooLarge,
    HtcCreditReportMalformed,
    HtcCreditReportEndpointOutOfRange,
    HtcTxEndpointOutOfRange,
    HtcTxPayloadTooLong,
    HtcTxOutputTooSmall,
    HtcCreditAllocationTooLarge,
    HtcInvalidBundleLimit,
    HtcSuspendCompleteUnexpectedPayload,
    CeInvalidLifecycleTransition,
    CeGenerationOverflow,
}

/// Work classes that an interrupt handler may enqueue for deferred service.
/// The order is only the initial round-robin order; no class has permanent
/// priority over another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum CeWorkClass {
    Control = 0,
    Receive = 1,
    TransmitComplete = 2,
    Fault = 3,
}

const CE_WORK_CLASS_COUNT: usize = 4;

/// Number of work items of each class handled by one bounded service pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CeWorkBatch {
    pub control: u16,
    pub receive: u16,
    pub transmit_complete: u16,
    pub fault: u16,
}

impl CeWorkBatch {
    fn record(&mut self, class: CeWorkClass) {
        let count = match class {
            CeWorkClass::Control => &mut self.control,
            CeWorkClass::Receive => &mut self.receive,
            CeWorkClass::TransmitComplete => &mut self.transmit_complete,
            CeWorkClass::Fault => &mut self.fault,
        };
        *count += 1;
    }

    pub const fn total(self) -> u16 {
        self.control + self.receive + self.transmit_complete + self.fault
    }
}

/// Coalesced software work queue for a CE event loop. An IRQ path may add
/// bounded work counts, then a deferred handler calls `service` with its CPU
/// budget. This does not encode or acknowledge hardware interrupt status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeWorkQueue {
    pending: [u16; CE_WORK_CLASS_COUNT],
    next_class: usize,
}

impl CeWorkQueue {
    pub const fn new() -> Self {
        Self {
            pending: [0; CE_WORK_CLASS_COUNT],
            next_class: 0,
        }
    }

    pub fn enqueue(&mut self, class: CeWorkClass, count: u16) -> Result<(), CeError> {
        let slot = &mut self.pending[class as usize];
        *slot = slot.checked_add(count).ok_or(CeError::EventCountOverflow)?;
        Ok(())
    }

    /// Consume at most `budget` items, rotating across non-empty classes.
    /// Returns the completed batch and whether work remains for a later pass.
    pub fn service(&mut self, budget: u16) -> (CeWorkBatch, bool) {
        let mut batch = CeWorkBatch::default();
        let mut remaining_budget = budget;
        let mut empty_visits = 0;

        while remaining_budget > 0 && empty_visits < CE_WORK_CLASS_COUNT {
            let class_index = self.next_class;
            self.next_class = (self.next_class + 1) % CE_WORK_CLASS_COUNT;
            if self.pending[class_index] == 0 {
                empty_visits += 1;
                continue;
            }

            self.pending[class_index] -= 1;
            let class = match class_index {
                0 => CeWorkClass::Control,
                1 => CeWorkClass::Receive,
                2 => CeWorkClass::TransmitComplete,
                _ => CeWorkClass::Fault,
            };
            batch.record(class);
            remaining_budget -= 1;
            empty_visits = 0;
        }

        (batch, self.pending.iter().any(|count| *count != 0))
    }

    pub const fn pending(&self) -> [u16; CE_WORK_CLASS_COUNT] {
        self.pending
    }
}

impl Default for CeWorkQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CeLifecycleState {
    Stopped,
    Running,
    Quiescing,
}

/// Software guard for queue generations across device resets. Calling
/// `confirm_quiesced` represents an external proof that DMA has stopped; this
/// model cannot produce that proof or control a device itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeSession {
    generation: u32,
    state: CeLifecycleState,
    ever_started: bool,
}

impl CeSession {
    pub const fn new() -> Self {
        Self {
            generation: 0,
            state: CeLifecycleState::Stopped,
            ever_started: false,
        }
    }

    pub fn start(&mut self) -> Result<u32, CeError> {
        if self.state != CeLifecycleState::Stopped || self.ever_started {
            return Err(CeError::CeInvalidLifecycleTransition);
        }
        self.state = CeLifecycleState::Running;
        self.ever_started = true;
        Ok(self.generation)
    }

    pub fn request_quiesce(&mut self) -> Result<(), CeError> {
        if self.state != CeLifecycleState::Running {
            return Err(CeError::CeInvalidLifecycleTransition);
        }
        self.state = CeLifecycleState::Quiescing;
        Ok(())
    }

    /// Accept only after the platform layer has established that the device
    /// can no longer access buffers in this session.
    pub fn confirm_quiesced(&mut self) -> Result<(), CeError> {
        if self.state != CeLifecycleState::Quiescing {
            return Err(CeError::CeInvalidLifecycleTransition);
        }
        self.state = CeLifecycleState::Stopped;
        Ok(())
    }

    /// Advance the generation before reusing queue slots after a reset.
    pub fn start_new_generation(&mut self) -> Result<u32, CeError> {
        if self.state != CeLifecycleState::Stopped || !self.ever_started {
            return Err(CeError::CeInvalidLifecycleTransition);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(CeError::CeGenerationOverflow)?;
        self.state = CeLifecycleState::Running;
        self.ever_started = true;
        Ok(self.generation)
    }

    pub fn accepts_completion(&self, generation: u32) -> bool {
        self.state == CeLifecycleState::Running && generation == self.generation
    }

    pub const fn state(&self) -> CeLifecycleState {
        self.state
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }
}

impl Default for CeSession {
    fn default() -> Self {
        Self::new()
    }
}

/// The WCN3990 CE descriptor is 16 bytes. Its first 64 bits contain the IOVA;
/// the high word also carries the hardware gather marker. The next two words
/// contain length/flags and the Toeplitz result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Wcn3990Descriptor([u8; 16]);

impl Wcn3990Descriptor {
    pub fn encode(
        iova: u64,
        length: usize,
        transfer_id: u16,
        gather: bool,
        byte_swap: bool,
    ) -> Result<Self, CeError> {
        if iova >= WCN3990_IOVA_LIMIT {
            return Err(CeError::AddressOutOfRange);
        }
        let length = u16::try_from(length).map_err(|_| CeError::TransferTooLong)?;
        if transfer_id > WCN3990_METADATA_MAX {
            return Err(CeError::TransferIdOutOfRange);
        }

        let mut bytes = [0; 16];
        let mut address_high = (iova >> 32) as u32;
        if gather {
            address_high |= WCN3990_GATHER_ADDRESS_FLAG;
        }
        let mut flags = transfer_id << WCN3990_METADATA_SHIFT;
        if gather {
            flags |= CE_DESC_GATHER_FLAG;
        }
        if byte_swap {
            flags |= CE_DESC_BYTE_SWAP_FLAG;
        }

        bytes[0..4].copy_from_slice(&(iova as u32).to_le_bytes());
        bytes[4..8].copy_from_slice(&address_high.to_le_bytes());
        bytes[8..10].copy_from_slice(&length.to_le_bytes());
        bytes[10..12].copy_from_slice(&flags.to_le_bytes());
        // The final u32 is the hardware-populated Toeplitz hash result.
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Parsed receive-side HTC header. `payload_len` includes any HTC trailer, as
/// it does in ath10k; `data_len` excludes the trailer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcRxHeader {
    pub endpoint_id: u8,
    pub flags: u8,
    pub payload_len: u16,
    pub control_byte0: u8,
    pub control_byte1: u8,
    pub pad_len: u16,
}

impl HtcRxHeader {
    pub const HEADER_SIZE: usize = 8;

    /// Parse only the fixed HTC receive header and verify the bounded frame
    /// extent. `max_frame_len` includes the 8-byte HTC header, not unused
    /// capacity in the backing DMA buffer.
    pub fn parse(frame: &[u8], max_frame_len: usize) -> Result<Self, CeError> {
        if frame.len() < Self::HEADER_SIZE {
            return Err(CeError::HtcHeaderTruncated);
        }
        if frame[0] >= HTC_ENDPOINT_COUNT {
            return Err(CeError::HtcEndpointOutOfRange);
        }
        let payload_len = u16::from_le_bytes([frame[2], frame[3]]);
        let payload_size = usize::from(payload_len);
        if Self::HEADER_SIZE + payload_size > max_frame_len
            || Self::HEADER_SIZE + payload_size > HTC_MAX_FRAME_LEN
        {
            return Err(CeError::HtcPayloadTooLong);
        }
        if Self::HEADER_SIZE + payload_size > frame.len() {
            return Err(CeError::HtcPayloadTruncated);
        }
        let control_byte0 = frame[4];
        let trailer_len = if frame[1] & HTC_FLAG_TRAILER_PRESENT != 0 {
            Some(control_byte0)
        } else {
            None
        };
        if trailer_len.is_some_and(|len| usize::from(len) > payload_size) {
            return Err(CeError::HtcTrailerTooLong);
        }

        Ok(Self {
            endpoint_id: frame[0],
            flags: frame[1],
            payload_len,
            control_byte0,
            control_byte1: frame[5],
            pad_len: u16::from_le_bytes([frame[6], frame[7]]),
        })
    }

    pub const fn data_len(self) -> usize {
        let trailer_len = if self.flags & HTC_FLAG_TRAILER_PRESENT != 0 {
            self.control_byte0
        } else {
            0
        };
        self.payload_len as usize - trailer_len as usize
    }

    /// Iterate the records at the end of the HTC payload. Unknown record IDs
    /// are returned unchanged for the caller to ignore or handle.
    pub fn trailer_records<'a>(self, frame: &'a [u8]) -> Result<HtcRecordIter<'a>, CeError> {
        if self.flags & HTC_FLAG_TRAILER_PRESENT == 0 {
            return Err(CeError::HtcTrailerMissing);
        }
        let trailer_len = usize::from(self.control_byte0);
        if trailer_len < 4 {
            return Err(CeError::HtcTrailerTooShort);
        }
        if trailer_len > usize::from(self.payload_len) {
            return Err(CeError::HtcTrailerTooLong);
        }
        let payload_end = Self::HEADER_SIZE + usize::from(self.payload_len);
        if payload_end > frame.len() {
            return Err(CeError::HtcPayloadTruncated);
        }
        let trailer_start = payload_end - trailer_len;
        Ok(HtcRecordIter {
            bytes: &frame[trailer_start..payload_end],
            offset: 0,
        })
    }
}

/// Encoded host-to-target HTC frame header. `write_frame` writes into a
/// caller-owned buffer and leaves it unchanged when validation fails.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcTxHeader([u8; HtcRxHeader::HEADER_SIZE]);

impl HtcTxHeader {
    pub fn encode(
        endpoint_id: u8,
        payload_len: usize,
        sequence: u8,
        request_credit_update: bool,
    ) -> Result<Self, CeError> {
        if endpoint_id >= HTC_ENDPOINT_COUNT {
            return Err(CeError::HtcTxEndpointOutOfRange);
        }
        if payload_len > HTC_MAX_FRAME_LEN - HtcRxHeader::HEADER_SIZE {
            return Err(CeError::HtcTxPayloadTooLong);
        }
        let mut bytes = [0; HtcRxHeader::HEADER_SIZE];
        bytes[0] = endpoint_id;
        bytes[1] = if request_credit_update {
            HTC_NEED_CREDIT_UPDATE
        } else {
            0
        };
        bytes[2..4].copy_from_slice(&(payload_len as u16).to_le_bytes());
        bytes[5] = sequence;
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; HtcRxHeader::HEADER_SIZE] {
        &self.0
    }

    pub fn write_frame(
        endpoint_id: u8,
        payload: &[u8],
        sequence: u8,
        request_credit_update: bool,
        output: &mut [u8],
    ) -> Result<usize, CeError> {
        let header = Self::encode(endpoint_id, payload.len(), sequence, request_credit_update)?;
        let frame_len = HtcRxHeader::HEADER_SIZE + payload.len();
        if output.len() < frame_len {
            return Err(CeError::HtcTxOutputTooSmall);
        }
        output[..HtcRxHeader::HEADER_SIZE].copy_from_slice(&header.0);
        output[HtcRxHeader::HEADER_SIZE..frame_len].copy_from_slice(payload);
        Ok(frame_len)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcRecord<'a> {
    pub id: u8,
    pub payload: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcCreditReport {
    pub endpoint_id: u8,
    pub credits: u8,
}

/// Fixed-size iterator over HTC trailer credit reports. A record payload must
/// contain whole 4-byte report entries; the two reserved bytes are ignored.
pub struct HtcCreditReportIter<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> HtcCreditReportIter<'a> {
    pub fn parse(record: HtcRecord<'a>) -> Result<Self, CeError> {
        if record.id != HTC_CREDIT_RECORD_ID || record.payload.len() % 4 != 0 {
            return Err(CeError::HtcCreditReportMalformed);
        }
        Ok(Self {
            payload: record.payload,
            offset: 0,
        })
    }
}

impl Iterator for HtcCreditReportIter<'_> {
    type Item = Result<HtcCreditReport, CeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.payload.len() {
            return None;
        }
        let endpoint_id = self.payload[self.offset];
        let credits = self.payload[self.offset + 1];
        self.offset += 4;
        if endpoint_id >= HTC_ENDPOINT_COUNT {
            return Some(Err(CeError::HtcCreditReportEndpointOutOfRange));
        }
        Some(Ok(HtcCreditReport {
            endpoint_id,
            credits,
        }))
    }
}

pub struct HtcRecordIter<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Iterator for HtcRecordIter<'a> {
    type Item = Result<HtcRecord<'a>, CeError>;

    fn next(&mut self) -> Option<Self::Item> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if remaining == 0 {
            return None;
        }
        if remaining < 4 {
            self.offset = self.bytes.len();
            return Some(Err(CeError::HtcRecordHeaderTruncated));
        }

        let record_start = self.offset;
        let payload_len = usize::from(self.bytes[record_start + 1]);
        let payload_start = record_start + 4;
        let record_end = payload_start + payload_len;
        if record_end > self.bytes.len() {
            self.offset = self.bytes.len();
            return Some(Err(CeError::HtcRecordPayloadTruncated));
        }

        self.offset = record_end;
        Some(Ok(HtcRecord {
            id: self.bytes[record_start],
            payload: &self.bytes[payload_start..record_end],
        }))
    }
}

/// Parsed HTC target-ready control message. Extended fields are optional
/// because older targets send only the six-byte base payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcReady {
    pub credit_count: u16,
    pub credit_size: u16,
    pub max_endpoints: u8,
    pub version: Option<u8>,
    pub max_rx_bundle: Option<u8>,
    pub alternate_data_credit_size: Option<u16>,
}

/// Host-to-target HTC service connection request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcConnectServiceRequest {
    pub service_id: u16,
    pub receive_allocation: u8,
    pub disable_credit_flow: bool,
}

impl HtcConnectServiceRequest {
    pub const MESSAGE_SIZE: usize = 8;

    /// Encode the fixed HTC message header and service request fields from
    /// `ath10k_htc_conn_svc`. Reserved bytes are zeroed.
    pub fn encode(self) -> [u8; Self::MESSAGE_SIZE] {
        let mut bytes = [0; Self::MESSAGE_SIZE];
        bytes[0..2].copy_from_slice(&HTC_CONNECT_REQUEST_MESSAGE_ID.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.service_id.to_le_bytes());
        let mut flags = u16::from(self.receive_allocation) << HTC_CONNECT_RECEIVE_ALLOC_SHIFT;
        if self.disable_credit_flow {
            flags |= HTC_CONNECT_DISABLE_CREDIT_FLOW;
        }
        bytes[4..6].copy_from_slice(&flags.to_le_bytes());
        bytes
    }
}

/// Parsed response to an HTC service connection request. Policy can reject a
/// non-success `status` or out-of-range endpoint after inspecting the result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcConnectServiceResponse {
    pub service_id: u16,
    pub status: u8,
    pub endpoint_id: u8,
    pub max_message_size: u16,
}

impl HtcConnectServiceResponse {
    pub fn parse(message: &[u8]) -> Result<Self, CeError> {
        if message.len() < 8 {
            return Err(CeError::HtcControlMessageTruncated);
        }
        let message_id = u16::from_le_bytes([message[0], message[1]]);
        if message_id != HTC_CONNECT_RESPONSE_MESSAGE_ID {
            return Err(CeError::HtcUnexpectedMessageId);
        }
        Ok(Self {
            service_id: u16::from_le_bytes([message[2], message[3]]),
            status: message[4],
            endpoint_id: message[5],
            max_message_size: u16::from_le_bytes([message[6], message[7]]),
        })
    }
}

/// Host-to-target extended HTC setup-complete message. `rx_bundle_limit`
/// enables the target's receive bundling only when a nonzero limit is given.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcSetupComplete {
    pub rx_bundle_limit: Option<u8>,
}

impl HtcSetupComplete {
    pub const MESSAGE_SIZE: usize = 12;

    /// Encode the extended message used by ath10k. Reserved fields remain
    /// zero; the bundle count is bounded to the driver's supported maximum.
    pub fn encode(self) -> Result<[u8; Self::MESSAGE_SIZE], CeError> {
        let bundle_limit = match self.rx_bundle_limit {
            Some(limit) if (1..=HTC_MAX_RX_BUNDLE).contains(&limit) => limit,
            Some(_) => return Err(CeError::HtcInvalidBundleLimit),
            None => 0,
        };

        let mut bytes = [0; Self::MESSAGE_SIZE];
        bytes[0..2].copy_from_slice(&HTC_SETUP_COMPLETE_EXT_MESSAGE_ID.to_le_bytes());
        if bundle_limit != 0 {
            bytes[4..8].copy_from_slice(&HTC_SETUP_COMPLETE_RX_BUNDLE_FLAG.to_le_bytes());
            bytes[8] = bundle_limit;
        }
        Ok(bytes)
    }
}

/// Decoded target-to-host HTC control message. Unknown IDs are retained as a
/// borrowed payload so callers can log or safely ignore future extensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtcControlMessage<'a> {
    Ready(HtcReady),
    ConnectServiceResponse(HtcConnectServiceResponse),
    SendSuspendComplete,
    Unknown { message_id: u16, payload: &'a [u8] },
}

impl<'a> HtcControlMessage<'a> {
    pub fn parse(message: &'a [u8]) -> Result<Self, CeError> {
        if message.len() < 2 {
            return Err(CeError::HtcControlMessageTruncated);
        }
        let message_id = u16::from_le_bytes([message[0], message[1]]);
        match message_id {
            HTC_READY_MESSAGE_ID => HtcReady::parse(message).map(Self::Ready),
            HTC_CONNECT_RESPONSE_MESSAGE_ID => {
                HtcConnectServiceResponse::parse(message).map(Self::ConnectServiceResponse)
            }
            HTC_SEND_SUSPEND_COMPLETE_MESSAGE_ID => {
                if message.len() != 2 {
                    return Err(CeError::HtcSuspendCompleteUnexpectedPayload);
                }
                Ok(Self::SendSuspendComplete)
            }
            _ => Ok(Self::Unknown {
                message_id,
                payload: &message[2..],
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcEndpoint {
    pub service_id: u16,
    pub endpoint_id: u8,
    pub max_message_size: u16,
}

/// Fixed-capacity endpoint map for the eight target endpoints; endpoint zero
/// remains reserved for HTC control messages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcEndpointMap {
    endpoints: [Option<HtcEndpoint>; HTC_ENDPOINT_COUNT as usize - 1],
}

impl HtcEndpointMap {
    pub const fn new() -> Self {
        Self {
            endpoints: [None; HTC_ENDPOINT_COUNT as usize - 1],
        }
    }

    pub fn connect(
        &mut self,
        requested_service_id: u16,
        response: HtcConnectServiceResponse,
    ) -> Result<HtcEndpoint, CeError> {
        if response.service_id != requested_service_id {
            return Err(CeError::HtcServiceIdMismatch);
        }
        if response.status != 0 {
            return Err(CeError::HtcServiceConnectFailed);
        }
        if response.endpoint_id == 0 || response.endpoint_id >= HTC_ENDPOINT_COUNT {
            return Err(CeError::HtcEndpointOutOfRange);
        }
        if response.max_message_size == 0 {
            return Err(CeError::HtcMessageSizeZero);
        }
        if self.endpoints.iter().flatten().any(|endpoint| {
            endpoint.endpoint_id == response.endpoint_id
                || endpoint.service_id == response.service_id
        }) {
            return Err(CeError::HtcEndpointInUse);
        }

        let connection = HtcEndpoint {
            service_id: response.service_id,
            endpoint_id: response.endpoint_id,
            max_message_size: response.max_message_size,
        };
        self.endpoints[usize::from(response.endpoint_id - 1)] = Some(connection);
        Ok(connection)
    }

    pub fn disconnect(
        &mut self,
        endpoint_id: u8,
        expected_service_id: u16,
    ) -> Result<HtcEndpoint, CeError> {
        if endpoint_id == 0 || endpoint_id >= HTC_ENDPOINT_COUNT {
            return Err(CeError::HtcEndpointOutOfRange);
        }
        let slot = &mut self.endpoints[usize::from(endpoint_id - 1)];
        let connection = slot.ok_or(CeError::HtcEndpointNotConnected)?;
        if connection.service_id != expected_service_id {
            return Err(CeError::HtcEndpointServiceMismatch);
        }
        *slot = None;
        Ok(connection)
    }

    pub fn by_endpoint(&self, endpoint_id: u8) -> Option<HtcEndpoint> {
        self.endpoints
            .get(usize::from(endpoint_id.checked_sub(1)?))
            .copied()
            .flatten()
    }
}

impl Default for HtcEndpointMap {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HtcCreditEndpoint {
    credit_size: u16,
    available: u16,
    flow_control: bool,
}

/// Bounded HTC per-endpoint transmit-credit accounting. It models protocol
/// state only; sending a frame and accepting device-originated reports remain
/// separate responsibilities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcCreditLedger {
    endpoints: [Option<HtcCreditEndpoint>; HTC_ENDPOINT_COUNT as usize],
}

impl HtcCreditLedger {
    pub const fn new() -> Self {
        Self {
            endpoints: [None; HTC_ENDPOINT_COUNT as usize],
        }
    }

    pub fn configure(
        &mut self,
        endpoint_id: u8,
        credit_size: u16,
        initial_credits: u16,
        flow_control: bool,
    ) -> Result<(), CeError> {
        if endpoint_id == 0 || endpoint_id >= HTC_ENDPOINT_COUNT {
            return Err(CeError::HtcEndpointOutOfRange);
        }
        if credit_size == 0 {
            return Err(CeError::HtcCreditSizeZero);
        }
        self.endpoints[usize::from(endpoint_id)] = Some(HtcCreditEndpoint {
            credit_size,
            available: initial_credits,
            flow_control,
        });
        Ok(())
    }

    /// Consume ceil(frame_len / credit_size) credits when flow control is
    /// enabled. Returns the amount consumed (zero when disabled).
    pub fn consume(&mut self, endpoint_id: u8, frame_len: usize) -> Result<u16, CeError> {
        let endpoint = self.endpoint_mut(endpoint_id)?;
        if !endpoint.flow_control {
            return Ok(0);
        }
        let size = usize::from(endpoint.credit_size);
        let credits = frame_len / size + usize::from(frame_len % size != 0);
        let credits = u16::try_from(credits).map_err(|_| CeError::HtcCreditCountTooLarge)?;
        if endpoint.available < credits {
            return Err(CeError::HtcInsufficientCredits);
        }
        endpoint.available -= credits;
        Ok(credits)
    }

    pub fn release(&mut self, endpoint_id: u8, credits: u16) -> Result<(), CeError> {
        let endpoint = self.endpoint_mut(endpoint_id)?;
        if !endpoint.flow_control {
            return Ok(());
        }
        endpoint.available = endpoint
            .available
            .checked_add(credits)
            .ok_or(CeError::HtcCreditOverflow)?;
        Ok(())
    }

    /// Apply one validated HTC trailer credit report to its endpoint.
    pub fn apply_report(&mut self, endpoint_id: u8, credits: u16) -> Result<(), CeError> {
        let endpoint = self.endpoint_mut(endpoint_id)?;
        endpoint.available = endpoint
            .available
            .checked_add(credits)
            .ok_or(CeError::HtcCreditOverflow)?;
        Ok(())
    }

    pub fn apply_wire_report(&mut self, report: HtcCreditReport) -> Result<(), CeError> {
        self.apply_report(report.endpoint_id, u16::from(report.credits))
    }

    pub fn available(&self, endpoint_id: u8) -> Result<u16, CeError> {
        self.endpoints
            .get(usize::from(endpoint_id))
            .copied()
            .flatten()
            .map(|endpoint| endpoint.available)
            .ok_or(if endpoint_id == 0 || endpoint_id >= HTC_ENDPOINT_COUNT {
                CeError::HtcEndpointOutOfRange
            } else {
                CeError::HtcEndpointNotConfigured
            })
    }

    fn endpoint_mut(&mut self, endpoint_id: u8) -> Result<&mut HtcCreditEndpoint, CeError> {
        if endpoint_id == 0 || endpoint_id >= HTC_ENDPOINT_COUNT {
            return Err(CeError::HtcEndpointOutOfRange);
        }
        self.endpoints[usize::from(endpoint_id)]
            .as_mut()
            .ok_or(CeError::HtcEndpointNotConfigured)
    }

    fn clear(&mut self, endpoint_id: u8) -> Result<(), CeError> {
        if endpoint_id == 0 || endpoint_id >= HTC_ENDPOINT_COUNT {
            return Err(CeError::HtcEndpointOutOfRange);
        }
        let slot = &mut self.endpoints[usize::from(endpoint_id)];
        if slot.is_none() {
            return Err(CeError::HtcEndpointNotConfigured);
        }
        *slot = None;
        Ok(())
    }
}

impl Default for HtcCreditLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Upstream-derived policy for one requested HTC service. WMI control receives
/// the target's credit allocation; other services disable HTC credit flow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcServicePolicy {
    pub receive_allocation: u8,
    pub disable_credit_flow: bool,
    pub credit_size: u16,
    pub initial_credits: u16,
}

impl HtcServicePolicy {
    pub fn derive(service_id: u16, ready: HtcReady) -> Result<Self, CeError> {
        if ready.credit_count == 0 || ready.credit_size == 0 {
            return Err(CeError::HtcInvalidReady);
        }
        let flow_control = service_id == HTC_WMI_CONTROL_SERVICE_ID;
        let receive_allocation = if flow_control {
            u8::try_from(ready.credit_count).map_err(|_| CeError::HtcCreditAllocationTooLarge)?
        } else {
            0
        };
        let credit_size = if service_id == HTC_HTT_DATA_SERVICE_ID {
            ready
                .alternate_data_credit_size
                .filter(|size| *size != 0)
                .unwrap_or(ready.credit_size)
        } else {
            ready.credit_size
        };
        Ok(Self {
            receive_allocation,
            disable_credit_flow: !flow_control,
            credit_size,
            initial_credits: if flow_control { ready.credit_count } else { 0 },
        })
    }

    pub const fn flow_control_enabled(self) -> bool {
        !self.disable_credit_flow
    }

    pub const fn connect_request(self, service_id: u16) -> HtcConnectServiceRequest {
        HtcConnectServiceRequest {
            service_id,
            receive_allocation: self.receive_allocation,
            disable_credit_flow: self.disable_credit_flow,
        }
    }
}

/// Host-only HTC connection state. Endpoint and credit changes are committed
/// together so failed service responses cannot leave half-configured state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtcSession {
    ready: HtcReady,
    endpoints: HtcEndpointMap,
    credits: HtcCreditLedger,
}

impl HtcSession {
    pub fn new(ready: HtcReady) -> Result<Self, CeError> {
        if ready.credit_count == 0 || ready.credit_size == 0 {
            return Err(CeError::HtcInvalidReady);
        }
        Ok(Self {
            ready,
            endpoints: HtcEndpointMap::new(),
            credits: HtcCreditLedger::new(),
        })
    }

    pub fn connect(
        &mut self,
        requested_service_id: u16,
        response: HtcConnectServiceResponse,
    ) -> Result<(HtcEndpoint, HtcServicePolicy), CeError> {
        let policy = HtcServicePolicy::derive(requested_service_id, self.ready)?;
        let mut endpoints = self.endpoints;
        let mut credits = self.credits;
        let endpoint = endpoints.connect(requested_service_id, response)?;
        credits.configure(
            endpoint.endpoint_id,
            policy.credit_size,
            policy.initial_credits,
            policy.flow_control_enabled(),
        )?;
        self.endpoints = endpoints;
        self.credits = credits;
        Ok((endpoint, policy))
    }

    pub fn disconnect(
        &mut self,
        endpoint_id: u8,
        expected_service_id: u16,
    ) -> Result<HtcEndpoint, CeError> {
        let mut endpoints = self.endpoints;
        let mut credits = self.credits;
        let endpoint = endpoints.disconnect(endpoint_id, expected_service_id)?;
        credits.clear(endpoint_id)?;
        self.endpoints = endpoints;
        self.credits = credits;
        Ok(endpoint)
    }

    pub fn endpoint(&self, endpoint_id: u8) -> Option<HtcEndpoint> {
        self.endpoints.by_endpoint(endpoint_id)
    }

    pub const fn credits(&self) -> HtcCreditLedger {
        self.credits
    }
}

/// Fixed-size Copy Engine TX ring model with opaque per-transfer tokens.
/// Completion indices are checked against the number of outstanding entries
/// before any token is returned or any state is changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeTxRing<const N: usize> {
    write_index: usize,
    complete_index: usize,
    contexts: [Option<u32>; N],
}

impl<const N: usize> CeTxRing<N> {
    pub fn new() -> Result<Self, CeError> {
        if N < 2 || !N.is_power_of_two() {
            return Err(CeError::InvalidRingSize);
        }
        Ok(Self {
            write_index: 0,
            complete_index: 0,
            contexts: [None; N],
        })
    }

    pub fn enqueue(&mut self, context: u32) -> Result<usize, CeError> {
        if self
            .contexts
            .iter()
            .flatten()
            .any(|posted| *posted == context)
        {
            return Err(CeError::TxTokenAlreadyPosted);
        }
        let next = self.next(self.write_index);
        if next == self.complete_index {
            return Err(CeError::RingFull);
        }
        let slot = self.write_index;
        if self.contexts[slot].is_some() {
            return Err(CeError::RingFull);
        }
        self.contexts[slot] = Some(context);
        self.write_index = next;
        Ok(slot)
    }

    /// Reclaim entries up to the hardware's source-ring read index. A duplicate
    /// or stale index reclaims zero entries; an index past posted work fails
    /// closed and leaves every context untouched.
    pub fn reclaim_to(
        &mut self,
        hardware_read_index: usize,
        output: &mut [u32],
    ) -> Result<usize, CeError> {
        if hardware_read_index >= N {
            return Err(CeError::InvalidHardwareIndex);
        }
        let completed = self.distance(self.complete_index, hardware_read_index);
        let pending = self.distance(self.complete_index, self.write_index);
        if completed > pending {
            return Err(CeError::CompletionBeyondPosted);
        }
        if output.len() < completed {
            return Err(CeError::CompletionOutputTooSmall);
        }

        for item in output.iter_mut().take(completed) {
            let context = self.contexts[self.complete_index]
                .take()
                .ok_or(CeError::CompletionBeyondPosted)?;
            *item = context;
            self.complete_index = self.next(self.complete_index);
        }
        Ok(completed)
    }

    pub const fn indices(&self) -> (usize, usize) {
        (self.write_index, self.complete_index)
    }

    fn next(&self, index: usize) -> usize {
        (index + 1) & (N - 1)
    }

    fn distance(&self, from: usize, to: usize) -> usize {
        (to + N - from) & (N - 1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RxSlot {
    buffer_token: u32,
    capacity: u16,
    completed_len: Option<u16>,
}

/// Fixed-size RX destination ring model. `device_complete` is a host-test
/// hook representing a descriptor update by the device; it performs no MMIO
/// and is not callable by a NexatrodeOS client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeRxRing<const N: usize> {
    write_index: usize,
    consume_index: usize,
    slots: [Option<RxSlot>; N],
}

impl<const N: usize> CeRxRing<N> {
    pub fn new() -> Result<Self, CeError> {
        if N < 2 || !N.is_power_of_two() {
            return Err(CeError::InvalidRingSize);
        }
        Ok(Self {
            write_index: 0,
            consume_index: 0,
            slots: [None; N],
        })
    }

    pub fn post_buffer(&mut self, buffer_token: u32, capacity: u16) -> Result<usize, CeError> {
        let next = self.next(self.write_index);
        if next == self.consume_index {
            return Err(CeError::RingFull);
        }
        if capacity == 0 {
            return Err(CeError::RxBufferTooSmall);
        }
        if self
            .slots
            .iter()
            .flatten()
            .any(|slot| slot.buffer_token == buffer_token)
        {
            return Err(CeError::RxBufferAlreadyPosted);
        }
        let slot = self.write_index;
        if self.slots[slot].is_some() {
            return Err(CeError::RingFull);
        }
        self.slots[slot] = Some(RxSlot {
            buffer_token,
            capacity,
            completed_len: None,
        });
        self.write_index = next;
        Ok(slot)
    }

    pub fn device_complete(&mut self, slot: usize, length: u16) -> Result<(), CeError> {
        if slot >= N {
            return Err(CeError::InvalidHardwareIndex);
        }
        let rx_slot = self.slots[slot]
            .as_mut()
            .ok_or(CeError::CompletionBeyondPosted)?;
        if rx_slot.completed_len.is_some() {
            return Err(CeError::RxCompletionDuplicate);
        }
        if length == 0 || length > rx_slot.capacity {
            return Err(CeError::RxBufferTooSmall);
        }
        rx_slot.completed_len = Some(length);
        Ok(())
    }

    pub fn reclaim_next(&mut self) -> Result<(u32, u16), CeError> {
        if self.consume_index == self.write_index {
            return Err(CeError::RingEmpty);
        }
        let rx_slot = self.slots[self.consume_index].ok_or(CeError::CompletionBeyondPosted)?;
        let length = rx_slot.completed_len.ok_or(CeError::RxCompletionNotReady)?;
        self.slots[self.consume_index] = None;
        self.consume_index = self.next(self.consume_index);
        Ok((rx_slot.buffer_token, length))
    }

    pub const fn indices(&self) -> (usize, usize) {
        (self.write_index, self.consume_index)
    }

    fn next(&self, index: usize) -> usize {
        (index + 1) & (N - 1)
    }
}

impl HtcReady {
    pub fn parse(message: &[u8]) -> Result<Self, CeError> {
        const BASE_LEN: usize = 2 + 6;
        const EXTENDED_LEN: usize = 2 + 10;
        if message.len() < BASE_LEN {
            return Err(CeError::HtcControlMessageTruncated);
        }
        let message_id = u16::from_le_bytes([message[0], message[1]]);
        if message_id != HTC_READY_MESSAGE_ID {
            return Err(CeError::HtcUnexpectedMessageId);
        }

        let credit_count = u16::from_le_bytes([message[2], message[3]]);
        let credit_size = u16::from_le_bytes([message[4], message[5]]);
        if credit_count == 0 || credit_size == 0 {
            return Err(CeError::HtcInvalidReady);
        }

        let (version, max_rx_bundle, alternate_data_credit_size) = if message.len() >= EXTENDED_LEN
        {
            (
                Some(message[8]),
                Some(message[9].min(HTC_MAX_RX_BUNDLE)),
                Some(u16::from_le_bytes([message[10], message[11]]) & 0x0fff),
            )
        } else {
            (None, None, None)
        };

        Ok(Self {
            credit_count,
            credit_size,
            max_endpoints: message[6],
            version,
            max_rx_bundle,
            alternate_data_credit_size,
        })
    }
}

/// A bounded host model of the CE ring's producer and software-completion
/// cursors. One slot remains unused so equal indices always mean empty.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CeRing {
    entries: u32,
    write_index: u32,
    complete_index: u32,
}

impl CeRing {
    pub fn new(entries: u32) -> Result<Self, CeError> {
        if entries < 2 || !entries.is_power_of_two() {
            return Err(CeError::InvalidRingSize);
        }
        Ok(Self {
            entries,
            write_index: 0,
            complete_index: 0,
        })
    }

    /// Reserve and return the next producer slot, advancing the write index.
    pub fn enqueue(&mut self) -> Result<u32, CeError> {
        let next = self.next(self.write_index);
        if next == self.complete_index {
            return Err(CeError::RingFull);
        }
        let slot = self.write_index;
        self.write_index = next;
        Ok(slot)
    }

    /// Reclaim one completed descriptor in FIFO order.
    pub fn complete_one(&mut self) -> Result<u32, CeError> {
        if self.complete_index == self.write_index {
            return Err(CeError::RingEmpty);
        }
        let slot = self.complete_index;
        self.complete_index = self.next(self.complete_index);
        Ok(slot)
    }

    pub const fn indices(&self) -> (u32, u32) {
        (self.write_index, self.complete_index)
    }

    fn next(&self, index: u32) -> u32 {
        (index + 1) & (self.entries - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn read_u16(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
    }

    #[test]
    fn event_service_respects_budget_and_preserves_pending_work() {
        let mut queue = CeWorkQueue::new();
        queue.enqueue(CeWorkClass::Control, 1).unwrap();
        queue.enqueue(CeWorkClass::Receive, 3).unwrap();
        queue.enqueue(CeWorkClass::TransmitComplete, 2).unwrap();
        queue.enqueue(CeWorkClass::Fault, 1).unwrap();

        let (first, more) = queue.service(4);
        assert_eq!(first.total(), 4);
        assert_eq!(first.control, 1);
        assert_eq!(first.receive, 1);
        assert_eq!(first.transmit_complete, 1);
        assert_eq!(first.fault, 1);
        assert!(more);
        assert_eq!(queue.pending(), [0, 2, 1, 0]);

        let (second, more) = queue.service(8);
        assert_eq!(second.receive, 2);
        assert_eq!(second.transmit_complete, 1);
        assert_eq!(second.total(), 3);
        assert!(!more);
    }

    #[test]
    fn event_service_rotates_start_class_to_prevent_starvation() {
        let mut queue = CeWorkQueue::new();
        queue.enqueue(CeWorkClass::Control, 3).unwrap();
        queue.enqueue(CeWorkClass::Receive, 3).unwrap();
        queue.enqueue(CeWorkClass::TransmitComplete, 3).unwrap();
        queue.enqueue(CeWorkClass::Fault, 3).unwrap();

        let (first, more) = queue.service(1);
        assert_eq!(first.control, 1);
        assert!(more);
        let (second, more) = queue.service(1);
        assert_eq!(second.receive, 1);
        assert!(more);
        let (third, more) = queue.service(1);
        assert_eq!(third.transmit_complete, 1);
        assert!(more);
        let (fourth, more) = queue.service(1);
        assert_eq!(fourth.fault, 1);
        assert!(more);
    }

    #[test]
    fn ce_session_rejects_stale_completions_across_quiesced_reset() {
        let mut session = CeSession::new();
        let first_generation = session.start().unwrap();
        assert!(session.accepts_completion(first_generation));
        assert_eq!(
            session.start_new_generation(),
            Err(CeError::CeInvalidLifecycleTransition)
        );

        session.request_quiesce().unwrap();
        assert!(!session.accepts_completion(first_generation));
        assert_eq!(
            session.start_new_generation(),
            Err(CeError::CeInvalidLifecycleTransition)
        );
        session.confirm_quiesced().unwrap();
        assert_eq!(session.start(), Err(CeError::CeInvalidLifecycleTransition));
        let second_generation = session.start_new_generation().unwrap();
        assert_ne!(first_generation, second_generation);
        assert!(!session.accepts_completion(first_generation));
        assert!(session.accepts_completion(second_generation));
    }

    #[test]
    fn ce_session_rejects_out_of_order_lifecycle_calls() {
        let mut session = CeSession::new();
        assert_eq!(
            session.confirm_quiesced(),
            Err(CeError::CeInvalidLifecycleTransition)
        );
        assert_eq!(
            session.request_quiesce(),
            Err(CeError::CeInvalidLifecycleTransition)
        );
        session.start().unwrap();
        assert_eq!(session.request_quiesce(), Ok(()));
        assert_eq!(session.state(), CeLifecycleState::Quiescing);
    }

    #[test]
    fn event_queue_zero_budget_and_overflow_are_non_mutating() {
        let mut queue = CeWorkQueue::new();
        queue.enqueue(CeWorkClass::Fault, u16::MAX).unwrap();
        assert_eq!(
            queue.enqueue(CeWorkClass::Fault, 1),
            Err(CeError::EventCountOverflow)
        );
        let before = queue.pending();
        let (batch, more) = queue.service(0);
        assert_eq!(batch.total(), 0);
        assert!(more);
        assert_eq!(queue.pending(), before);
    }

    #[test]
    fn htc_credit_ledger_consumes_rounded_credits_and_applies_reports() {
        let mut ledger = HtcCreditLedger::new();
        ledger.configure(1, 128, 3, true).unwrap();
        assert_eq!(ledger.consume(1, 128), Ok(1));
        assert_eq!(ledger.consume(1, 129), Ok(2));
        assert_eq!(ledger.available(1), Ok(0));
        assert_eq!(ledger.consume(1, 1), Err(CeError::HtcInsufficientCredits));
        ledger.apply_report(1, 2).unwrap();
        assert_eq!(ledger.available(1), Ok(2));
    }

    #[test]
    fn htc_credit_ledger_checks_configuration_and_overflow() {
        let mut ledger = HtcCreditLedger::new();
        assert_eq!(
            ledger.configure(1, 0, 1, true),
            Err(CeError::HtcCreditSizeZero)
        );
        assert_eq!(ledger.consume(1, 1), Err(CeError::HtcEndpointNotConfigured));
        assert_eq!(
            ledger.configure(0, 128, 1, true),
            Err(CeError::HtcEndpointOutOfRange)
        );
        ledger.configure(2, 256, u16::MAX, true).unwrap();
        assert_eq!(ledger.apply_report(2, 1), Err(CeError::HtcCreditOverflow));
        assert_eq!(ledger.available(2), Ok(u16::MAX));
    }

    #[test]
    fn htc_credit_flow_disable_skips_consumption_but_reports_update_balance() {
        let mut ledger = HtcCreditLedger::new();
        ledger.configure(3, 64, 5, false).unwrap();
        assert_eq!(ledger.consume(3, 4096), Ok(0));
        ledger.apply_report(3, 7).unwrap();
        assert_eq!(ledger.available(3), Ok(12));
    }

    #[test]
    fn descriptor_matches_wcn3990_64_bit_layout() {
        let descriptor = Wcn3990Descriptor::encode(0x1_2345_6789, 1500, 0x321, true, true).unwrap();
        let bytes = descriptor.as_bytes();
        assert_eq!(read_u32(bytes, 0), 0x2345_6789);
        assert_eq!(read_u32(bytes, 4), 0x8000_0001);
        assert_eq!(read_u16(bytes, 8), 1500);
        assert_eq!(read_u16(bytes, 10), (0x321 << 4) | 0b11);
        assert_eq!(read_u32(bytes, 12), 0);
    }

    #[test]
    fn descriptor_rejects_values_that_do_not_fit_hardware_fields() {
        assert_eq!(
            Wcn3990Descriptor::encode(WCN3990_IOVA_LIMIT, 1, 0, false, false),
            Err(CeError::AddressOutOfRange)
        );
        assert_eq!(
            Wcn3990Descriptor::encode(0, usize::from(u16::MAX) + 1, 0, false, false),
            Err(CeError::TransferTooLong)
        );
        assert_eq!(
            Wcn3990Descriptor::encode(0, 1, WCN3990_METADATA_MAX + 1, false, false),
            Err(CeError::TransferIdOutOfRange)
        );
    }

    #[test]
    fn ring_reserves_one_entry_and_wraps_in_fifo_order() {
        let mut ring = CeRing::new(4).unwrap();
        assert_eq!(ring.enqueue(), Ok(0));
        assert_eq!(ring.enqueue(), Ok(1));
        assert_eq!(ring.enqueue(), Ok(2));
        assert_eq!(ring.enqueue(), Err(CeError::RingFull));
        assert_eq!(ring.complete_one(), Ok(0));
        assert_eq!(ring.enqueue(), Ok(3));
        assert_eq!(ring.complete_one(), Ok(1));
        assert_eq!(ring.complete_one(), Ok(2));
        assert_eq!(ring.complete_one(), Ok(3));
        assert_eq!(ring.complete_one(), Err(CeError::RingEmpty));
        assert_eq!(ring.indices(), (0, 0));
    }

    #[test]
    fn ring_size_must_be_power_of_two() {
        assert_eq!(CeRing::new(0), Err(CeError::InvalidRingSize));
        assert_eq!(CeRing::new(1), Err(CeError::InvalidRingSize));
        assert_eq!(CeRing::new(3), Err(CeError::InvalidRingSize));
        assert!(CeRing::new(8).is_ok());
    }

    #[test]
    fn htc_receive_header_parses_little_endian_lengths() {
        let frame = [3, 2, 5, 0, 2, 7, 4, 0, 0xaa, 0xbb, 0xcc, 0xdd, 0xee];
        let header = HtcRxHeader::parse(&frame, 64).unwrap();
        assert_eq!(header.endpoint_id, 3);
        assert_eq!(header.flags, 2);
        assert_eq!(header.payload_len, 5);
        assert_eq!(header.control_byte0, 2);
        assert_eq!(header.control_byte1, 7);
        assert_eq!(header.pad_len, 4);
        assert_eq!(header.data_len(), 3);
    }

    #[test]
    fn htc_receive_header_rejects_truncated_or_unbounded_frames() {
        assert_eq!(
            HtcRxHeader::parse(&[0; 7], 64),
            Err(CeError::HtcHeaderTruncated)
        );

        let header_only = [0, 0, 1, 0, 0, 0, 0, 0];
        assert_eq!(
            HtcRxHeader::parse(&header_only, 64),
            Err(CeError::HtcPayloadTruncated)
        );

        let oversized = [0, 0, 2, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            HtcRxHeader::parse(&oversized, 9),
            Err(CeError::HtcPayloadTooLong)
        );

        let invalid_endpoint = [HTC_ENDPOINT_COUNT, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            HtcRxHeader::parse(&invalid_endpoint, 64),
            Err(CeError::HtcEndpointOutOfRange)
        );

        let oversized_declared_frame = [0, 0, 0xf9, 0x0f, 0, 0, 0, 0];
        assert_eq!(
            HtcRxHeader::parse(&oversized_declared_frame, usize::MAX),
            Err(CeError::HtcPayloadTooLong)
        );

        let invalid_trailer = [0, 2, 1, 0, 2, 0, 0, 0, 0];
        assert_eq!(
            HtcRxHeader::parse(&invalid_trailer, 64),
            Err(CeError::HtcTrailerTooLong)
        );
    }

    #[test]
    fn htc_trailer_records_are_bounded_and_unknown_ids_are_preserved() {
        // Payload is AABB followed by records: credits(2 bytes), unknown(1).
        let frame = [
            1, 2, 13, 0, 11, 0, 0, 0, 0xaa, 0xbb, 1, 2, 0, 0, 0x10, 0x20, 4, 1, 0, 0, 0xcc,
        ];
        let header = HtcRxHeader::parse(&frame, 4096).unwrap();
        let mut records = header.trailer_records(&frame).unwrap();
        assert_eq!(
            records.next(),
            Some(Ok(HtcRecord {
                id: 1,
                payload: &[0x10, 0x20]
            }))
        );
        assert_eq!(
            records.next(),
            Some(Ok(HtcRecord {
                id: 4,
                payload: &[0xcc]
            }))
        );
        assert_eq!(records.next(), None);
    }

    #[test]
    fn htc_trailer_records_reject_incomplete_record_data() {
        let mut truncated_header = HtcRecordIter {
            bytes: &[1, 2, 3],
            offset: 0,
        };
        assert_eq!(
            truncated_header.next(),
            Some(Err(CeError::HtcRecordHeaderTruncated))
        );
        assert_eq!(truncated_header.next(), None);

        let short_payload = [1, 2, 6, 0, 6, 0, 0, 0, 1, 3, 0, 0, 0xaa, 0xbb];
        let header = HtcRxHeader::parse(&short_payload, 4096).unwrap();
        let mut records = header.trailer_records(&short_payload).unwrap();
        assert_eq!(
            records.next(),
            Some(Err(CeError::HtcRecordPayloadTruncated))
        );
        assert_eq!(records.next(), None);
    }

    #[test]
    fn htc_credit_report_record_parses_all_fixed_width_reports() {
        let record = HtcRecord {
            id: HTC_CREDIT_RECORD_ID,
            payload: &[1, 3, 0xaa, 0xbb, 8, 7, 0, 0],
        };
        let reports = HtcCreditReportIter::parse(record)
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(
            reports,
            vec![
                Ok(HtcCreditReport {
                    endpoint_id: 1,
                    credits: 3
                }),
                Ok(HtcCreditReport {
                    endpoint_id: 8,
                    credits: 7
                })
            ]
        );
    }

    #[test]
    fn htc_credit_report_record_rejects_unknown_record_bad_length_and_endpoint() {
        assert!(matches!(
            HtcCreditReportIter::parse(HtcRecord {
                id: HTC_CREDIT_RECORD_ID,
                payload: &[1, 2, 3]
            }),
            Err(CeError::HtcCreditReportMalformed)
        ));
        assert!(matches!(
            HtcCreditReportIter::parse(HtcRecord {
                id: 4,
                payload: &[]
            }),
            Err(CeError::HtcCreditReportMalformed)
        ));
        let mut reports = HtcCreditReportIter::parse(HtcRecord {
            id: HTC_CREDIT_RECORD_ID,
            payload: &[9, 1, 0, 0],
        })
        .unwrap();
        assert_eq!(
            reports.next(),
            Some(Err(CeError::HtcCreditReportEndpointOutOfRange))
        );
        assert_eq!(reports.next(), None);
    }

    #[test]
    fn parsed_htc_credit_reports_update_only_the_named_endpoint() {
        let mut ledger = HtcCreditLedger::new();
        ledger.configure(1, 128, 2, true).unwrap();
        ledger.configure(2, 128, 4, true).unwrap();
        let report = HtcCreditReportIter::parse(HtcRecord {
            id: HTC_CREDIT_RECORD_ID,
            payload: &[2, 3, 0, 0],
        })
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
        ledger.apply_wire_report(report).unwrap();
        assert_eq!(ledger.available(1), Ok(2));
        assert_eq!(ledger.available(2), Ok(7));
    }

    #[test]
    fn htc_tx_frame_encodes_header_and_payload_with_bounds() {
        let payload = [0xaa, 0xbb, 0xcc];
        let mut output = [0x55; 16];
        let length = HtcTxHeader::write_frame(2, &payload, 17, true, &mut output).unwrap();
        assert_eq!(length, 11);
        assert_eq!(
            &output[..8],
            &[2, HTC_NEED_CREDIT_UPDATE, 3, 0, 0, 17, 0, 0]
        );
        assert_eq!(&output[8..11], &payload);
        assert_eq!(output[11], 0x55);

        let too_small_before = [0x5a; 10];
        let mut too_small = too_small_before;
        assert_eq!(
            HtcTxHeader::write_frame(2, &payload, 0, false, &mut too_small),
            Err(CeError::HtcTxOutputTooSmall)
        );
        assert_eq!(too_small, too_small_before);
        assert_eq!(
            HtcTxHeader::encode(HTC_ENDPOINT_COUNT, 0, 0, false),
            Err(CeError::HtcTxEndpointOutOfRange)
        );
        assert_eq!(
            HtcTxHeader::encode(1, HTC_MAX_FRAME_LEN, 0, false),
            Err(CeError::HtcTxPayloadTooLong)
        );
    }

    #[test]
    fn htc_ready_parses_base_and_extended_target_capabilities() {
        let base = [1, 0, 0x20, 0, 0x80, 0, 9, 0];
        assert_eq!(
            HtcReady::parse(&base),
            Ok(HtcReady {
                credit_count: 32,
                credit_size: 128,
                max_endpoints: 9,
                version: None,
                max_rx_bundle: None,
                alternate_data_credit_size: None,
            })
        );

        let extended = [1, 0, 0x20, 0, 0x80, 0, 9, 0, 1, 40, 0x34, 0x12];
        let ready = HtcReady::parse(&extended).unwrap();
        assert_eq!(ready.version, Some(1));
        assert_eq!(ready.max_rx_bundle, Some(HTC_MAX_RX_BUNDLE));
        assert_eq!(ready.alternate_data_credit_size, Some(0x0234));
    }

    #[test]
    fn htc_ready_rejects_wrong_or_incomplete_messages() {
        assert_eq!(
            HtcReady::parse(&[1, 0]),
            Err(CeError::HtcControlMessageTruncated)
        );
        assert_eq!(
            HtcReady::parse(&[2, 0, 1, 0, 1, 0, 9, 0]),
            Err(CeError::HtcUnexpectedMessageId)
        );
        assert_eq!(
            HtcReady::parse(&[1, 0, 0, 0, 1, 0, 9, 0]),
            Err(CeError::HtcInvalidReady)
        );
    }

    #[test]
    fn htc_setup_complete_encodes_bundle_policy_and_reserved_zeroes() {
        let plain = HtcSetupComplete {
            rx_bundle_limit: None,
        }
        .encode()
        .unwrap();
        assert_eq!(plain, [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

        let bundled = HtcSetupComplete {
            rx_bundle_limit: Some(HTC_MAX_RX_BUNDLE),
        }
        .encode()
        .unwrap();
        assert_eq!(read_u16(&bundled, 0), HTC_SETUP_COMPLETE_EXT_MESSAGE_ID);
        assert_eq!(&bundled[2..4], &[0; 2]);
        assert_eq!(read_u32(&bundled, 4), HTC_SETUP_COMPLETE_RX_BUNDLE_FLAG);
        assert_eq!(bundled[8], HTC_MAX_RX_BUNDLE);
        assert_eq!(&bundled[9..], &[0; 3]);
        assert_eq!(
            HtcSetupComplete {
                rx_bundle_limit: Some(0)
            }
            .encode(),
            Err(CeError::HtcInvalidBundleLimit)
        );
        assert_eq!(
            HtcSetupComplete {
                rx_bundle_limit: Some(HTC_MAX_RX_BUNDLE + 1)
            }
            .encode(),
            Err(CeError::HtcInvalidBundleLimit)
        );
    }

    #[test]
    fn htc_control_dispatch_parses_known_and_preserves_unknown_messages() {
        let ready = [1, 0, 0x20, 0, 0x80, 0, 9, 0];
        assert!(matches!(
            HtcControlMessage::parse(&ready),
            Ok(HtcControlMessage::Ready(HtcReady {
                credit_count: 32,
                credit_size: 128,
                ..
            }))
        ));

        let response = [3, 0, 0, 1, 0, 2, 0x00, 0x08];
        assert_eq!(
            HtcControlMessage::parse(&response),
            Ok(HtcControlMessage::ConnectServiceResponse(
                HtcConnectServiceResponse {
                    service_id: 0x0100,
                    status: 0,
                    endpoint_id: 2,
                    max_message_size: 2048,
                }
            ))
        );

        assert_eq!(
            HtcControlMessage::parse(&[6, 0]),
            Ok(HtcControlMessage::SendSuspendComplete)
        );
        assert_eq!(
            HtcControlMessage::parse(&[6, 0, 0]),
            Err(CeError::HtcSuspendCompleteUnexpectedPayload)
        );

        let unknown = [0x34, 0x12, 0xaa, 0xbb];
        assert_eq!(
            HtcControlMessage::parse(&unknown),
            Ok(HtcControlMessage::Unknown {
                message_id: 0x1234,
                payload: &[0xaa, 0xbb],
            })
        );
        assert_eq!(
            HtcControlMessage::parse(&[1]),
            Err(CeError::HtcControlMessageTruncated)
        );
        assert_eq!(
            HtcControlMessage::parse(&[3, 0]),
            Err(CeError::HtcControlMessageTruncated)
        );
    }

    #[test]
    fn htc_connect_response_parses_without_assuming_success() {
        let response = HtcConnectServiceResponse::parse(&[3, 0, 0, 1, 1, 4, 0x00, 0x08]).unwrap();
        assert_eq!(response.service_id, 0x0100);
        assert_eq!(response.status, 1);
        assert_eq!(response.endpoint_id, 4);
        assert_eq!(response.max_message_size, 2048);
    }

    #[test]
    fn htc_connect_request_encodes_service_flags_and_zero_padding() {
        let message = HtcConnectServiceRequest {
            service_id: 0x0100,
            receive_allocation: 7,
            disable_credit_flow: true,
        }
        .encode();
        assert_eq!(read_u16(&message, 0), HTC_CONNECT_REQUEST_MESSAGE_ID);
        assert_eq!(read_u16(&message, 2), 0x0100);
        assert_eq!(
            read_u16(&message, 4),
            (7 << 8) | HTC_CONNECT_DISABLE_CREDIT_FLOW
        );
        assert_eq!(&message[6..8], &[0, 0]);
    }

    fn test_htc_ready() -> HtcReady {
        HtcReady {
            credit_count: 24,
            credit_size: 256,
            max_endpoints: 9,
            version: Some(1),
            max_rx_bundle: Some(8),
            alternate_data_credit_size: Some(128),
        }
    }

    #[test]
    fn htc_service_policy_matches_wmi_credit_flow_and_htt_credit_size() {
        let ready = test_htc_ready();
        assert_eq!(
            HtcServicePolicy::derive(HTC_WMI_CONTROL_SERVICE_ID, ready),
            Ok(HtcServicePolicy {
                receive_allocation: 24,
                disable_credit_flow: false,
                credit_size: 256,
                initial_credits: 24,
            })
        );
        assert_eq!(
            HtcServicePolicy::derive(HTC_HTT_DATA_SERVICE_ID, ready),
            Ok(HtcServicePolicy {
                receive_allocation: 0,
                disable_credit_flow: true,
                credit_size: 128,
                initial_credits: 0,
            })
        );
        assert_eq!(
            HtcServicePolicy::derive(0x0301, ready).unwrap().credit_size,
            256
        );
        assert_eq!(
            HtcServicePolicy::derive(
                HTC_WMI_CONTROL_SERVICE_ID,
                HtcReady {
                    credit_count: 300,
                    ..ready
                }
            ),
            Err(CeError::HtcCreditAllocationTooLarge)
        );
    }

    #[test]
    fn htc_session_commits_endpoint_and_credit_state_together() {
        let mut session = HtcSession::new(test_htc_ready()).unwrap();
        let failed = HtcConnectServiceResponse {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            status: 2,
            endpoint_id: 1,
            max_message_size: 512,
        };
        assert_eq!(
            session.connect(HTC_WMI_CONTROL_SERVICE_ID, failed),
            Err(CeError::HtcServiceConnectFailed)
        );
        assert_eq!(session.endpoint(1), None);
        assert_eq!(
            session.credits().available(1),
            Err(CeError::HtcEndpointNotConfigured)
        );

        let response = HtcConnectServiceResponse {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            status: 0,
            endpoint_id: 1,
            max_message_size: 512,
        };
        let (endpoint, policy) = session
            .connect(HTC_WMI_CONTROL_SERVICE_ID, response)
            .unwrap();
        assert_eq!(endpoint.max_message_size, 512);
        assert_eq!(policy.receive_allocation, 24);
        assert_eq!(session.credits().available(1), Ok(24));

        session.disconnect(1, HTC_WMI_CONTROL_SERVICE_ID).unwrap();
        assert_eq!(session.endpoint(1), None);
        assert_eq!(
            session.credits().available(1),
            Err(CeError::HtcEndpointNotConfigured)
        );
    }

    #[test]
    fn htc_endpoint_map_accepts_only_valid_unique_connections() {
        let mut endpoints = HtcEndpointMap::new();
        let response = HtcConnectServiceResponse {
            service_id: 0x0100,
            status: 0,
            endpoint_id: 4,
            max_message_size: 2048,
        };
        assert_eq!(
            endpoints.connect(0x0100, response),
            Ok(HtcEndpoint {
                service_id: 0x0100,
                endpoint_id: 4,
                max_message_size: 2048,
            })
        );
        assert_eq!(
            endpoints.by_endpoint(4),
            Some(HtcEndpoint {
                service_id: 0x0100,
                endpoint_id: 4,
                max_message_size: 2048,
            })
        );
        assert_eq!(endpoints.by_endpoint(0), None);
        assert_eq!(
            endpoints.connect(0x0100, response),
            Err(CeError::HtcEndpointInUse)
        );
    }

    #[test]
    fn htc_endpoint_map_rejects_failed_or_mismatched_responses() {
        let mut endpoints = HtcEndpointMap::new();
        let response = HtcConnectServiceResponse {
            service_id: 0x0100,
            status: 1,
            endpoint_id: 2,
            max_message_size: 1024,
        };
        assert_eq!(
            endpoints.connect(0x0101, response),
            Err(CeError::HtcServiceIdMismatch)
        );
        assert_eq!(
            endpoints.connect(0x0100, response),
            Err(CeError::HtcServiceConnectFailed)
        );
        assert_eq!(endpoints.by_endpoint(2), None);
    }

    #[test]
    fn htc_endpoint_map_rejects_invalid_endpoint_and_message_size() {
        let mut endpoints = HtcEndpointMap::new();
        let response = |endpoint_id, max_message_size| HtcConnectServiceResponse {
            service_id: 0x0100,
            status: 0,
            endpoint_id,
            max_message_size,
        };
        assert_eq!(
            endpoints.connect(0x0100, response(0, 1024)),
            Err(CeError::HtcEndpointOutOfRange)
        );
        assert_eq!(
            endpoints.connect(0x0100, response(HTC_ENDPOINT_COUNT, 1024)),
            Err(CeError::HtcEndpointOutOfRange)
        );
        assert_eq!(
            endpoints.connect(0x0100, response(1, 0)),
            Err(CeError::HtcMessageSizeZero)
        );
        assert_eq!(endpoints.by_endpoint(1), None);
    }

    #[test]
    fn htc_endpoint_disconnect_rejects_stale_ids_and_allows_reuse() {
        let mut endpoints = HtcEndpointMap::new();
        let connected = HtcConnectServiceResponse {
            service_id: 0x0100,
            status: 0,
            endpoint_id: 3,
            max_message_size: 1024,
        };
        let endpoint = endpoints.connect(0x0100, connected).unwrap();
        assert_eq!(
            endpoints.disconnect(3, 0x0101),
            Err(CeError::HtcEndpointServiceMismatch)
        );
        assert_eq!(endpoints.by_endpoint(3), Some(endpoint));
        assert_eq!(endpoints.disconnect(3, 0x0100), Ok(endpoint));
        assert_eq!(endpoints.by_endpoint(3), None);
        assert_eq!(
            endpoints.disconnect(3, 0x0100),
            Err(CeError::HtcEndpointNotConnected)
        );

        let replacement = HtcConnectServiceResponse {
            service_id: 0x0300,
            status: 0,
            endpoint_id: 3,
            max_message_size: 512,
        };
        assert_eq!(
            endpoints.connect(0x0300, replacement).unwrap().service_id,
            0x0300
        );
    }

    #[test]
    fn ce_tx_completion_reclaims_tokens_only_within_posted_range() {
        let mut ring = CeTxRing::<4>::new().unwrap();
        ring.enqueue(10).unwrap();
        ring.enqueue(20).unwrap();
        assert_eq!(
            ring.reclaim_to(3, &mut [0; 2]),
            Err(CeError::CompletionBeyondPosted)
        );
        assert_eq!(
            ring.reclaim_to(2, &mut [0; 1]),
            Err(CeError::CompletionOutputTooSmall)
        );

        let mut output = [0; 2];
        assert_eq!(ring.reclaim_to(2, &mut output), Ok(2));
        assert_eq!(output, [10, 20]);
        assert_eq!(ring.indices(), (2, 2));
        assert_eq!(ring.reclaim_to(2, &mut output), Ok(0));
        assert_eq!(
            ring.reclaim_to(4, &mut output),
            Err(CeError::InvalidHardwareIndex)
        );
    }

    #[test]
    fn ce_tx_completion_handles_wrap_without_replaying_old_tokens() {
        let mut ring = CeTxRing::<4>::new().unwrap();
        for token in [10, 20, 30] {
            ring.enqueue(token).unwrap();
        }
        assert_eq!(ring.enqueue(10), Err(CeError::TxTokenAlreadyPosted));
        let mut output = [0; 3];
        assert_eq!(ring.reclaim_to(3, &mut output), Ok(3));
        assert_eq!(output, [10, 20, 30]);

        ring.enqueue(40).unwrap();
        ring.enqueue(10).unwrap();
        assert_eq!(ring.reclaim_to(1, &mut output), Ok(2));
        assert_eq!(&output[..2], &[40, 10]);
        assert_eq!(
            ring.reclaim_to(3, &mut output),
            Err(CeError::CompletionBeyondPosted)
        );
    }

    #[test]
    fn ce_rx_ring_reclaims_only_completed_bounded_buffers() {
        let mut ring = CeRxRing::<4>::new().unwrap();
        let first = ring.post_buffer(10, 1500).unwrap();
        let second = ring.post_buffer(20, 512).unwrap();
        assert_eq!(ring.reclaim_next(), Err(CeError::RxCompletionNotReady));
        assert_eq!(ring.device_complete(second, 64), Ok(()));
        assert_eq!(ring.reclaim_next(), Err(CeError::RxCompletionNotReady));
        assert_eq!(ring.device_complete(first, 1400), Ok(()));
        assert_eq!(ring.reclaim_next(), Ok((10, 1400)));
        assert_eq!(ring.reclaim_next(), Ok((20, 64)));
        assert_eq!(ring.reclaim_next(), Err(CeError::RingEmpty));
    }

    #[test]
    fn ce_rx_ring_rejects_oversized_and_duplicate_completions() {
        let mut ring = CeRxRing::<4>::new().unwrap();
        assert_eq!(ring.post_buffer(77, 0), Err(CeError::RxBufferTooSmall));
        let slot = ring.post_buffer(77, 128).unwrap();
        assert_eq!(
            ring.post_buffer(77, 128),
            Err(CeError::RxBufferAlreadyPosted)
        );
        assert_eq!(
            ring.device_complete(slot, 129),
            Err(CeError::RxBufferTooSmall)
        );
        assert_eq!(
            ring.device_complete(slot, 0),
            Err(CeError::RxBufferTooSmall)
        );
        assert_eq!(ring.device_complete(slot, 128), Ok(()));
        assert_eq!(
            ring.device_complete(slot, 64),
            Err(CeError::RxCompletionDuplicate)
        );
        assert_eq!(
            ring.device_complete(4, 1),
            Err(CeError::InvalidHardwareIndex)
        );
    }
}
