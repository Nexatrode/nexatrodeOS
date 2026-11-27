//! Bounded, allocation-free GLINK/QRTR/QMI exchanges for an already opened channel.
//!
//! Input is ONE complete, eight-byte-aligned GLINK record, copied out of the
//! native FIFO by qcom_glink_fifo. Channel negotiation, FIFO cursors, barriers,
//! interrupts and storage/memory actions are outside this session core. The
//! opt-in bounded QRTR controller handles direct-peer control and announcements.
//! All buffers here are private host memory, never peer addresses.
//!
//! `publish_next` calls a synchronous writer under exclusive access. Ok means
//! the whole record was published exactly once; WouldBlock means NONE of it
//! was published. An uncertain write quarantines the entire session until a
//! trusted transport reset. Generation changes alone cannot stop native DMA.
//!
//! Wire reference: LineageOS sunfish kernel 28f9290ae067d3e7857126eb6312720dbe9c7c80,
//! drivers/rpmsg/qcom_glink_native.c and net/qrtr/qrtr.c. This deliberately
//! supports a narrower profile than the complete Linux transports.
#![forbid(unsafe_code)]
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
#[path="qcom_efs_overlap.rs"]
mod efs_overlap;
#[cfg(feature="sunfish-efs-reply-overlap")]
pub use efs_overlap::EfsOverlapTrace;
/* EFS_REPLY_OVERLAP_END */

#[cfg(feature="sunfish-mcfg-close-cleanup")]
#[path="qcom_mcfg_close.rs"]
mod mcfg_close;
#[cfg(feature="sunfish-mcfg-close-cleanup")]
pub(crate) use mcfg_close::McfgCloseOutcome;

#[cfg(feature="sunfish-elf-window-trace")]
#[path="qcom_elf_window.rs"]
mod elf_window;
/* READY_HANDOFF_BEGIN */
#[cfg(feature="sunfish-registry-ready-handoffs")]
#[path="qcom_registry_ready_handoff.rs"]
mod ready_handoff;
/* READY_HANDOFF_END */
#[cfg(feature="sunfish-elf-first-slot-turn")]
#[path="qcom_elf_slot_turn.rs"]
mod elf_slot_turn;
#[cfg(feature="sunfish-elf-tx-slots")]
#[path="qcom_elf_tx_slots.rs"]
mod elf_tx_slots;
#[cfg(feature="sunfish-elf-tx-slots")]
pub use elf_tx_slots::{ElfTxSlot,ElfTxFull,ElfTxFree,ElfTxSlots};
#[cfg(feature="sunfish-elf-third-reply")]
#[path="qcom_elf_third_reply.rs"]
mod elf_third_reply;
#[cfg(feature="sunfish-elf-window-trace")]
pub use elf_window::{ElfWindowTrace,ElfWindowSnapshot,ElfWindowEntry,ElfWindowReceipt};

#[cfg(feature="sunfish-post-elf-mcfg-trace")]
#[path="qcom_post_elf_mcfg.rs"]
mod post_elf_mcfg;
#[cfg(feature="sunfish-post-elf-mcfg-trace")]
pub use post_elf_mcfg::{PostElfMcfgTrace, PostMcfgRequest, McfgOptionIds, McfgWireRecord};
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
#[path="qcom_startup_prerequisites.rs"]
mod startup_prerequisites;
#[cfg(feature="sunfish-startup-prerequisites")]
pub use startup_prerequisites::{StartupPrerequisites,StartupRead};
/* STARTUP_PREREQ_END */

#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "sunfish_ipa_linux_qmi_link.rs"]
mod linux_ipa_link;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
pub use linux_ipa_link::{LinuxIpaHandle, LinuxIpaReceipt, LinuxIpaTxDiagnostic};
#[cfg(feature = "sunfish-ipa-init-continuation")]
pub use linux_ipa_link::IpaInitTransportTrace;
#[cfg(feature = "sunfish-ipa-registration-trace")]
pub use linux_ipa_link::IpaRegistrationTrace;

use crate::qcom_ipa::{self as ipa_wire, InitRequestFields};
use crate::qcom_ipa_client::{
    self as ipa_client, Client as IpaClient, ResponseOutcome as IpaOutcome, State as IpaState,
};
use crate::qcom_ipa_service::{self as ipa_service, PreparedIndication};
use crate::qcom_modem_services::{self as services, Request, Service};
use crate::qcom_msa::MsaReadyPermit;
use crate::qcom_msa::Reservation as MsaReservation;
use crate::qcom_qmi::{self as qmi, Message};
use crate::qcom_qmi_router::{self as routing, Address, Context, ReplyRoute, Router, Ticket};
use crate::qcom_qrtr_control::{self as control, Controller};
use crate::qcom_servreg_notif_client::{
    self as servreg_notif, Client as ServregNotifClient, Outcome as ServregNotifOutcome,
};
use crate::qcom_wlan_power::PowerReadyPermit;
use crate::qcom_wlfw_bdf::VerifiedBdfImage;
use crate::qcom_wlfw_client::{
    self as wlfw, BdfSegmentOutcome as WlfwBdfSegmentOutcome,
    CalReportOutcome as WlfwCalReportOutcome, CapabilitySummary as WlfwCapabilitySummary,
    Client as WlfwClient, DynamicFeatureMaskOutcome as WlfwDynamicFeatureMaskOutcome,
    IndicationOutcome as WlfwIndicationOutcome, MsaInfoOutcome as WlfwMsaInfoOutcome,
    MsaReadyOutcome as WlfwMsaReadyOutcome, RegistrationOutcome as WlfwOutcome, State as WlfwState,
    WlanConfigOutcome as WlfwConfigOutcome, WlanModeOutcome as WlfwModeOutcome,
};
use core::sync::atomic::{AtomicU64, Ordering};

pub const MAX_DATAGRAM_BYTES: usize = routing::MAX_DATAGRAM_BYTES;
pub const RX_SLOTS: usize = 8;
const REQUEST_SLOTS: usize = RX_SLOTS
    + cfg!(feature = "sunfish-glink-registry-early-release") as usize
    + cfg!(feature = "sunfish-glink-registry-second-host-entry") as usize;
pub const MAX_REQUESTS: usize = 4;
#[cfg(all(feature = "sunfish-tftp-peer-close-experiment", not(feature = "sunfish-tftp-close-owner-fence")))]
pub const TFTP_OWNER_PEER_SLOTS: usize = MAX_REQUESTS * 2 + 1;
#[cfg(all(feature = "sunfish-tftp-peer-close-experiment", feature = "sunfish-tftp-close-owner-fence"))]
pub const TFTP_OWNER_PEER_SLOTS: usize = MAX_REQUESTS * 3 + 1;
pub const TX_SLOTS: usize = 8;
const TX_STORAGE_SLOTS: usize = TX_SLOTS + cfg!(feature = "sunfish-tftp-oack-reserve") as usize;

#[cfg(feature = "sunfish-tftp-oack-reserve")]
#[derive(Clone, Copy, Debug)]
pub struct ReservedOack {
    pub peer: Address,
    pub transfer_port: u32,
    pub queued_at: Option<u64>,
    pub published_at: Option<u64>,
    pub done_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
    pub wire_bytes: [u8; 72],
    #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
    pub wire_len: u32,
    #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
    pub wire_intent: u32,
    #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
    pub writer_trace: ReservedWriterTrace,
    #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
    pub early_status: u8,
    #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
    pub early_command: u8,
    #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
    pub early_guards: u16,
    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub retry_state: u8,
    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub retry_result: u8,
    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub retry_ack_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub retry_credit_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub retry_attempt_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub credit_pass_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub credit_counts: [u16; 7],
    #[cfg(feature = "sunfish-tftp-oack-credit-early")]
    pub credit_turn: u8,
    #[cfg(feature = "sunfish-tftp-oack-credit-early")]
    pub retry_choice: u8,
    #[cfg(feature = "sunfish-tftp-oack-credit-early")]
    pub retry_choice_id: u32,
    #[cfg(feature = "sunfish-tftp-oack-credit-early")]
    pub retry_gates: u32,
    #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
    pub after_done_state: u8,
    #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
    pub after_done_result: u8,
    #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
    pub after_done_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
    pub after_done_retry_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-after-done-trace")]
    pub after_done_first_check: Option<u64>,
    #[cfg(feature = "sunfish-tftp-after-done-trace")]
    pub after_done_grant_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-after-done-trace")]
    pub after_done_pending_checks: u32,
    #[cfg(feature = "sunfish-tftp-after-done-trace")]
    pub after_done_origin: u8,
    #[cfg(feature = "sunfish-tftp-after-done-trace")]
    pub after_done_choice: u8,
    #[cfg(feature = "sunfish-tftp-after-done-trace")]
    pub after_done_choice_id: u32,
    #[cfg(feature = "sunfish-tftp-after-done-trace")]
    pub after_done_gates: u32,
    #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
    pub after_done_ad: u8,
    #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
    pub after_done_ack: u8,
}
// The observed IPCRTR startup inventory contains 30 entries in four size groups.
pub const REMOTE_INTENTS: usize = 32;
pub const MAX_FRAGMENTS: usize = 64;
pub const TX_CHUNK_BYTES: usize = 256;
pub const MAX_TX_RECORD_BYTES: usize = 16 + TX_CHUNK_BYTES;
/// First rejected peer intent request, retained without reading hardware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntentDenialDiagnostic {
    pub requested: u32,
    /// One four-bit receive-state code per slot, least-significant slot first.
    pub states: u32,
}
#[cfg(feature = "sunfish-grant-ad-writer-trace")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GrantAdWriter {
    Ordinary = 1,
    Registration = 2,
    OtherFollowup = 3,
}

#[cfg(feature = "sunfish-grant-ad-writer-trace")]
#[derive(Debug, Default, Clone, Copy)]
pub struct GrantBackingTrace {
    pub requested: u32,
    pub intent: Option<u32>,
    pub reserved_at: Option<u64>,
    pub writer: Option<GrantAdWriter>,
    pub advertised_at: Option<u64>,
}

#[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntentDenialSlots {
    pub at: Option<u64>,
    pub slots: [(u32, u32); RX_SLOTS + 2],
}

#[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntentDenialWindow {
    pub requested: u32,
    pub request: IntentDenialSlots,
    pub ack: IntentDenialSlots,
}

/// Private local receive-slot metadata, never a peer-delivery receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveSnapshot {
    pub states: u32,
    pub services: u32,
    pub replies: u32,
}
/// Software-only metadata for held TFTP datagrams and transmit records.
#[cfg(feature = "sunfish-native-tftp-server-check")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TftpQueueSnapshot {
    /// Low 8 bits opcode; bits 8/9 mark service/transfer port; bit 10
    /// marks the exact observed WRQ; bits 16..31 contain payload length.
    pub metadata: [u32; 5],
    pub peers: [u32; 5],
    pub total: u32,
    /// OR across records: present, started, fully published, GLINK done, QRTR done.
    pub tx_flags: u32,
    pub tx_count: u32,
    pub all_tx_used: u32,
    pub flow_used: u32,
}
#[cfg(feature = "sunfish-ipa-tftp-queue-block")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TftpQueueBlock {
    /// 1 same-peer transmit, 2 transmit slots, 3 QRTR flow table,
    /// 4 retired peer, 5 reference flow blocked, 6 reference flow capacity.
    pub check: u8,
    pub kind: u8,
    pub opcode: u8,
    /// Bits 0..3: started, fully published, GLINK done, QRTR done.
    pub flags: u8,
}

#[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TftpTxSlot {
    pub kind: u8,
    pub opcode: u8,
    pub flags: u8,
    pub peer_port: u32,
    pub remote_intent: u32,
}

/// Header-only metadata from the last failed routing attempt; no payload bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingDiagnostic {
    pub service: u32,
    pub source: u32,
    pub destination: u32,
    pub body_bytes: u32,
    pub kind: u32,
    pub message_id: u32,
    pub transaction: u32,
    /// TLV type in bits 16..23 and declared length in bits 0..15.
    pub tlvs: [u32; 2],
}
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Control(control::Error),
    Routing(routing::Error),
    Wire(qmi::Error),
    InvalidChannel,
    StaleGeneration,
    Quarantined,
    TruncatedRecord,
    RecordLength,
    UnsupportedCommand,
    WrongChannel,
    InvalidIntent,
    IntentCapacity,
    IntentAlreadyActive,
    NoCapacity,
    FragmentOrder,
    FragmentLength,
    FragmentLimit,
    InvalidHandle,
    NoRemoteIntent,
    InvalidCompletion,
    InvalidCredit,
    FlowBusy,
    InvalidReply,
    WouldBlock,
    PublicationUncertain,
    IdExhausted,
    IpaClient(ipa_client::Error),
    Wlfw(wlfw::Error),
    ServregNotif(servreg_notif::Error),
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    LinuxIpa(crate::sunfish_ipa_linux_qmi::Error),
}

impl From<routing::Error> for Error {
    fn from(value: routing::Error) -> Self {
        Self::Routing(value)
    }
}
impl From<qmi::Error> for Error {
    fn from(value: qmi::Error) -> Self {
        Self::Wire(value)
    }
}
impl From<control::Error> for Error {
    fn from(value: control::Error) -> Self {
        Self::Control(value)
    }
}
impl From<wlfw::Error> for Error {
    fn from(value: wlfw::Error) -> Self {
        Self::Wlfw(value)
    }
}
impl From<servreg_notif::Error> for Error {
    fn from(value: servreg_notif::Error) -> Self {
        Self::ServregNotif(value)
    }
}
impl From<ipa_client::Error> for Error {
    fn from(value: ipa_client::Error) -> Self {
        Self::IpaClient(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    pub local_node: u32,
    pub peer_node: u32,
    pub local_channel: u16,
    pub remote_channel: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TftpReplyRoute {
    pub source: Address,
    pub destination: Address,
}

/// A lookup capability, not a caller-editable response route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestHandle {
    owner: u64,
    generation: u64,
    intent: u32,
}

/// Opaque capability for one complete raw QRTR TFTP datagram held in a
/// receive slot. The payload remains borrowed from that slot and is released
/// only after `finish_tftp_request` publishes its GLINK RX_DONE.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TftpHandle {
    owner: u64,
    generation: u64,
    intent: u32,
}

#[cfg(feature = "sunfish-ipa-tftp-original-trace")]
impl TftpHandle {
    pub(crate) fn diagnostic_id(self) -> u32 { self.intent }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    #[cfg(feature="sunfish-mcfg-close-cleanup")]
    McfgClosePending { peer: Address },
    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
    FirmwareClosePending { peer: Address },
    /// An observed close is held pending an owner decision; no cleanup or
    /// receive completion has been performed.
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    TftpClosePending { peer: Address },
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    RemoteIntentRequestAck { granted: bool },
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    RequestDeferred,
    QrtrControl(control::Event),
    /// Payload-free TFTP request metadata. This observation path sends no TFTP
    /// response and exists only for the explicitly enabled probe profile.
    TftpObserved {
        peer: Address,
        observation: Result<crate::qcom_tftp::RequestObservation, crate::qcom_tftp::Error>,
    },
    /// A raw datagram for the explicitly prepared TFTP server profile. The
    /// private receive slot is held until the owner finishes dispatching it.
    TftpRequest(TftpHandle),
    RemoteIntents,
    #[cfg(feature = "sunfish-ipa-ignore-config-reference")]
    LinuxIpaConfigIgnored,
    #[cfg(feature = "sunfish-ipa-ignore-unhandled-reference")]
    LinuxIpaUnhandledIgnored { message_id: u16 },
    #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
    LinuxIpaInitResponseIgnored,
    ReceiveIntentRequest {
        granted: bool,
    },
    Fragment,
    Request(RequestHandle),
    CreditReceived,
    TransmitDone,
    /// Peer GLINK RX_DONE for a QMI reply associated with this request.
    ReplyRxDone(RequestHandle),
    /// Matching QRTR RESUME_TX for a published QMI reply associated with this
    /// request. The reply is retired only after both this and RX_DONE arrive.
    ReplyQrtrResume(RequestHandle),
    WlfwRegistration(WlfwOutcome),
    WlfwMsaInfo(WlfwMsaInfoOutcome),
    WlfwMsaReady(WlfwMsaReadyOutcome),
    WlfwCapabilities(WlfwCapabilitySummary),
    WlfwDynamicFeatureMask(WlfwDynamicFeatureMaskOutcome),
    WlfwBdfSegment(WlfwBdfSegmentOutcome),
    WlfwCalReport(WlfwCalReportOutcome),
    WlfwConfig(WlfwConfigOutcome),
    WlfwMode(WlfwModeOutcome),
    WlfwIndication(WlfwIndicationOutcome),
    ServregNotifState(ServregNotifOutcome),
    IpaInitModemDriver(IpaOutcome),
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    LinuxIpaResponse(LinuxIpaHandle),
}

/// Precise reason a peer RX_DONE record could not be matched to the bounded
/// transmit state. Kept as diagnostic state so native owners can report which
/// invariant rejected the completion without weakening validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionFailure {
    NoBusyRemoteIntent,
    NoFullyPublishedTransmit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionDiagnostic {
    pub intent_id: u32,
    pub reuse: bool,
    pub failure: CompletionFailure,
    pub busy_remote_intents: u8,
    pub incomplete_transmits: u8,
    pub completed_transmits: u8,
}

/// Bounded snapshot of a rejected QRTR RESUME_TX datagram. `words` contains
/// the 13 u32 values at QRTR offsets 0..48; a word not present in the declared
/// datagram is `u32::MAX`. `bad_fields` uses the bit assignments documented
/// at the receive_credit validation site. This is observation only: rejected
/// packets remain held and do not advance RX confirmation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreditDiagnostic {
    pub packet_len: u32,
    pub words: [u32; 13],
    pub bad_fields: u32,
    pub known_flow: bool,
    /// bit 0: matching reply; bit 1: fully published reply; bit 2: partial reply
    pub reply_state: u32,
}

/// Action results must come from an authorized backend. This codec does not
/// allocate memory, open partitions, or infer success from a decoded request.
/// Success fields are checked structurally, not against a service response
/// schema; the backend must supply the appropriate optional fields.
pub enum Reply<'a> {
    Success { optional_tlvs: &'a [u8] },
    Failure { error: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteError {
    WouldBlock,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Published {
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    RemoteIntentRequest { size: u32 },
    ReceiveIntent(u32),
    IntentRequestAck {
        granted: bool,
    },
    ReceiveDone(u32),
    Data {
        remote_intent: u32,
        last_fragment: bool,
    },
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RxState {
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    Deferred,
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    LinuxIpa,
    Free,
    Advertising,
    Available,
    Assembling,
    Request,
    ReplyQueued,
    Control,
    Tftp,
    Held,
}
impl RxState {
    fn diagnostic_code(self) -> u32 {
        match self {
            Self::Free => 0, Self::Advertising => 1, Self::Available => 2,
            Self::Assembling => 3, Self::Request => 4, Self::ReplyQueued => 5,
            Self::Control => 6, Self::Tftp => 7, Self::Held => 8,
            #[cfg(feature = "sunfish-linux-ipa-qmi")]
            Self::LinuxIpa => 9,
            #[cfg(feature = "sunfish-qrtr-deferred-requests")]
            Self::Deferred => 10,
        }
    }
}

struct Receive {
    state: RxState,
    id: u32,
    #[cfg(feature = "sunfish-glink-announcement-reuse")]
    announcement_reuse: bool,
    #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
    reused_datagram_id: Option<u32>,
    bytes: [u8; MAX_DATAGRAM_BYTES],
    used: usize,
    remaining: usize,
    fragments: usize,
    done_pending: bool,
    context: Option<Context>,
    ticket: Option<Ticket>,
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    deferred_sequence: u64,
    #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
    elf_wrq_observed: bool,
    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    server_check_wrq: Option<ServerCheckOackTrace>,
}

impl Receive {
    const fn empty() -> Self {
        Self {
            state: RxState::Free,
            id: 0,
            #[cfg(feature = "sunfish-glink-announcement-reuse")]
            announcement_reuse: false,
            #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
            reused_datagram_id: None,
            bytes: [0; MAX_DATAGRAM_BYTES],
            used: 0,
            remaining: 0,
            fragments: 0,
            done_pending: false,
            context: None,
            ticket: None,
            #[cfg(feature = "sunfish-qrtr-deferred-requests")]
            deferred_sequence: 0,
            #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
            elf_wrq_observed: false,
            #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
            server_check_wrq: None,
        }
    }

    fn tftp_datagram_id(&self) -> u32 {
        #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
        if let Some(id) = self.reused_datagram_id {
            return id;
        }
        self.id
    }
}

#[derive(Clone, Copy)]
struct RemoteIntent {
    id: u32,
    capacity: u32,
    busy: bool,
}

/// One request per generation; denial and uncertain publication never retry.
#[cfg(feature = "sunfish-glink-intent-request-experiment")]
#[derive(Clone, Copy, Eq, PartialEq)]
enum OutboundIntentRequest {
    Idle,
    Waiting { size: u32, granted: bool },
    Done { size: u32, granted: bool },
}

#[derive(Clone, Copy)]
enum TxKind {
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    LinuxIpa {
        source: Address,
        destination: Address,
        receive: Option<usize>,
        id: crate::sunfish_ipa_linux_qmi::PublicationId,
    },
    Resume,
    Control(control::Outbound),
    Reply {
        receive: usize,
        route: ReplyRoute,
        ipa_token: Option<u64>,
    },
    TftpReply {
        source: Address,
        destination: Address,
    },
    IpaIndication {
        source: Address,
        destination: Address,
        token: u64,
    },
    IpaRequest,
    WlfwRequest,
    ServregNotifRequest,
}

impl TxKind {
    #[cfg(any(feature = "sunfish-wlan-elf-queue-trace", feature = "sunfish-ipa-tftp-queue-block"))]
    fn diagnostic_kind(self) -> u8 {
        match self {
            TxKind::Resume => 1,
            TxKind::Control(_) => 2,
            TxKind::Reply { route, .. } if route.service == Service::RemoteStorage => 3,
            TxKind::Reply { route, .. } if route.service == Service::ServiceRegistry => 4,
            TxKind::Reply { .. } => 5,
            TxKind::TftpReply { .. } => 6,
            #[cfg(feature = "sunfish-linux-ipa-qmi")]
            TxKind::LinuxIpa { .. } => 7,
            TxKind::IpaRequest | TxKind::IpaIndication { .. } => 7,
            _ => 8,
        }
    }

    fn qrtr_addresses(self) -> Option<(Address, Address)> {
        match self {
            #[cfg(feature = "sunfish-linux-ipa-qmi")]
            Self::LinuxIpa {
                source,
                destination,
                ..
            } => Some((source, destination)),
            Self::Reply { route, .. } => Some((route.source, route.destination)),
            Self::TftpReply {
                source,
                destination,
            } => Some((source, destination)),
            Self::IpaIndication {
                source,
                destination,
                ..
            } => Some((source, destination)),
            _ => None,
        }
    }
}

#[cfg(feature = "sunfish-wlan-elf-queue-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryQueuePressure {
    /// 1 same-peer TX, 2 full TX slots, 3 legacy flow table capacity,
    /// 4 reference flow blocked, 5 reference flow table capacity.
    pub reason: u8,
    pub used_slots: u8,
    pub unpublished_slots: u8,
    pub glink_wait_slots: u8,
    pub qrtr_wait_slots: u8,
    pub kind_mask: u32,
    pub peer_kind: u8,
    pub peer_flags: u8,
    #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
    pub peer_publication: Option<RegistryPublication>,
    #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
    pub peer_request: Option<RequestHandle>,
    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    pub pending_ack_age: Option<u64>,
}

#[cfg(feature = "sunfish-wlan-elf-publish-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryPublication {
    pub transaction: u16,
    pub bytes: u16,
    pub calls: u32,
    /// Bits 0–6: no suitable intent, outstanding intent request,
    /// control selected, other data selected, selected but FIFO blocked,
    /// written, and outbound intent request selected. Bit 7 records a
    /// successful experimental registry-priority publication. Bits 8/9
    /// record the borrowed pre-grant DATA attempt/successful commit. Bits
    /// 10/11/12 record conversion arming, its callback attempt and commit.
    /// 13/14/15/16 record conversion seen, own return pending, no exact
    /// candidate and an already-used borrowed opportunity at conversion.
    pub mask: u32,
    /// First priority gate on each call: cooldown, TFTP, intent ACK, own
    /// RX_DONE, no candidate, different registry candidate, credit, selected.
    #[cfg(feature = "sunfish-wlan-registry-turn-gates")]
    pub turn_gates: [u32; 8],
    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    pub ack: RegistryAckTrace,
    /// First failed borrowed-turn condition, or selected: no ACK, denial,
    /// no unadvertised backing, no ACK yield, cooldown, TFTP, WRQ return,
    /// own return, used opportunity, no exact candidate, credit, selected.
    /// Observation only; none of these counters controls publication.
    #[cfg(feature = "sunfish-registry-grant-eligibility")]
    pub grant_gates: [u32; 12],
}

#[cfg(feature = "sunfish-wlan-registry-ack-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryAckTrace {
    pub sent: u32,
    pub blocked: u32,
    pub done_ahead: u32,
    pub ads_ahead: u32,
    pub max_age: Option<u64>,
}

#[cfg(feature = "sunfish-wlan-registry-ack-trace")]
impl RegistryAckTrace {
    const EMPTY: Self = Self { sent: 0, blocked: 0, done_ahead: 0, ads_ahead: 0, max_age: None };
}

#[cfg(feature = "sunfish-wlan-registry-ack-trace")]
enum AckObservation {
    Sent(Option<u64>),
    Blocked,
    DoneAhead,
    AdvertisementAhead,
}

#[cfg(feature = "sunfish-wlan-elf-publish-trace")]
#[derive(Clone, Copy)]
enum PublishChoice {
    Idle,
    IntentRequest,
    Control,
    Data(usize),
}

struct Transmit {
    kind: TxKind,
    remote_slot: Option<usize>,
    bytes: [u8; MAX_DATAGRAM_BYTES],
    len: usize,
    sent: usize,
    #[cfg(feature = "sunfish-registry-between-grants")]
    registry_grant_turn_used: bool,
/* READY_HANDOFF_BEGIN */
    #[cfg(feature="sunfish-registry-ready-handoffs")]
    registry_return_turn_due:bool,
/* READY_HANDOFF_END */
    #[cfg(feature = "sunfish-registry-converted-credit-turn")]
    registry_credit_turn_used: bool,
    #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
    registry_publication: RegistryPublication,
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    final_ack_observed: bool,
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    missing_reply_observed: bool,
    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    server_check_wrq: Option<u32>,
    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
    traced_oack_request: Option<TftpHandle>,
    glink_done: bool,
    qrtr_done: bool,
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    reference_flow: Option<crate::qcom_qrtr_flow::Reservation>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct QrtrFlow {
    source: Address,
    destination: Address,
}

impl Transmit {
    const fn empty() -> Self {
        Self {
            kind: TxKind::Resume,
            remote_slot: None,
            bytes: [0; MAX_DATAGRAM_BYTES],
            len: 0,
            sent: 0,
            #[cfg(feature = "sunfish-registry-between-grants")]
            registry_grant_turn_used: false,
/* READY_HANDOFF_BEGIN */
            #[cfg(feature="sunfish-registry-ready-handoffs")]
            registry_return_turn_due:false,
/* READY_HANDOFF_END */
            #[cfg(feature = "sunfish-registry-converted-credit-turn")]
            registry_credit_turn_used: false,
            #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
            registry_publication: RegistryPublication {
                transaction: 0, bytes: 0, calls: 0, mask: 0,
                #[cfg(feature = "sunfish-wlan-registry-turn-gates")]
                turn_gates: [0; 8],
                #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
                ack: RegistryAckTrace::EMPTY,
                #[cfg(feature = "sunfish-registry-grant-eligibility")]
                grant_gates: [0; 12],
            },
            #[cfg(feature = "sunfish-tftp-final-ack-observer")]
            final_ack_observed: false,
            #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
            missing_reply_observed: false,
            #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
            server_check_wrq: None,
            #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
            traced_oack_request: None,
            glink_done: false,
            qrtr_done: true,
            #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
            reference_flow: None,
        }
    }
}

/// Observation only: successful bounded second admissions and the exact startup
/// pair. Opaque request identities survive TX-slot reuse. No clock is used by
/// admission, ordering, completion matching, or the overlap proof.
#[cfg(feature = "sunfish-registry-window-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryWindowTrace {
    pub admissions: u32,
    pub target: Option<RegistryWindowPair>,
    #[cfg(feature = "sunfish-registry-handoff-turn")]
    pub predecessor: Option<RegistryPredecessorTrace>,
}
#[cfg(feature = "sunfish-registry-handoff-turn")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryPredecessorTrace {
    #[cfg(feature = "sunfish-registry-post-reply-trace")]
    pub grant: Option<RegistryConvertedGrantTrace>,
    request: RequestHandle,
    #[cfg(feature = "sunfish-registry-predecessor-pressure")]
    pub publication: Option<RegistryPublication>,
    #[cfg(feature = "sunfish-registry-predecessor-pressure")]
    pub publication_frozen: bool,
    pub queued_at: Option<u64>,
    pub published_at: Option<u64>,
    pub done_at: Option<u64>,
}
/// Exact converted backing retained for observation after the scheduling token expires.
#[cfg(feature = "sunfish-registry-post-reply-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryConvertedGrantTrace {
    pub intent: u32,
    pub converted_at: Option<u64>,
    pub advertised_at: Option<u64>,
    pub ack_at: Option<u64>,
    advertised: bool,
    acked: bool,
}
#[cfg(feature = "sunfish-registry-window-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryWindowPair {
    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    pub prearm: RegistryPrearmTrace,
    #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
    pub conversion: RegistryConversionLifetime,
    #[cfg(feature = "sunfish-registry-post-reply-trace")]
    pub publication: Option<RegistryPublication>,
    #[cfg(feature = "sunfish-registry-post-reply-trace")]
    pub publication_frozen: bool,
    older: RequestHandle,
    newer: RequestHandle,
    pub older_transaction: u16,
    pub newer_transaction: u16,
    pub peer: Address,
    pub queued_at: Option<u64>,
    pub newer_published_at: Option<u64>,
    pub older_done_at: Option<u64>,
    pub published_before_done: Option<bool>,
    older_completed: bool,
}

/// Observation of the first converted grant for the exact startup transaction 4.
/// Identity, active/finished state and writer outcomes do not depend on clocks.
#[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryConversionLifetime {
    pub arms: u32,
    pub first: Option<RegistryConversionTurn>,
}
#[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryConversionTurn {
    #[cfg(feature = "sunfish-registry-converted-credit-turn")]
    pub packed_len_sent: u32,
    #[cfg(feature = "sunfish-registry-converted-credit-turn")]
    pub length_consistent: bool,
    #[cfg(feature = "sunfish-registry-converted-credit-turn")]
    pub credit_ports: Option<u32>,
    #[cfg(feature = "sunfish-registry-converted-credit-turn")]
    pub credit_nodes: Option<u32>,
    pub intent: u32,
    pub request_intent: u32,
    pub bytes: u16,
    pub sent: u16,
    pub armed_at: Option<u64>,
    pub entry: Option<RegistryConversionEntry>,
    pub choice: Option<RegistryConversionChoice>,
    /// 1: borrowed callback starts; 2: matching advertisement committed;
    /// 3: ACK committed; 4: another conversion replaces the token;
    /// 5: bounded ready-credit callback starts.
    pub exit: Option<(u8, Option<u64>)>,
    pub advertised_at: Option<u64>,
    pub ack_at: Option<u64>,
    active: bool,
    advertised: bool,
    acked: bool,
}
#[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryConversionEntry {
    pub at: Option<u64>,
    pub flags: u32,
    pub backing: Option<u32>,
    pub token_request: Option<u32>,
    pub gate: Option<u8>,
}
#[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryConversionChoice {
    pub at: Option<u64>,
    pub command: u16,
    pub parameter: u32,
    pub transaction: Option<u16>,
    /// 0: selected but no callback (e.g. bounded follow-up veto);
    /// 1: committed, 2: WouldBlock, 3: uncertain, 4: callback entered but
    /// no returned result (e.g. unwind).
    pub result: u8,
}

#[cfg(feature = "sunfish-registry-prearm-io-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryPrearmTrace {
    pub queued_flags: u32,
    pub first_pass_at: Option<u64>,
    pub first_flags: Option<u32>,
    pub first_gate: Option<u8>,
    pub first_choice: Option<RegistryConversionChoice>,
    pub passes: u32,
    pub own_done_at: Option<u64>,
    own_done_seen: bool,
}

/// Fixed private buffers: 8 receive and 8 transmit slots of 4096 bytes each,
/// plus bookkeeping. A native integration must budget this storage and its
/// construction/reset stack separately from peer-visible memory pools.
pub struct Session {
    #[cfg(feature = "sunfish-registry-window-trace")]
    registry_window: RegistryWindowTrace,
    #[cfg(feature = "sunfish-ipa-before-first-oack")]
    ipa_before_first_oack: bool,
    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    first_oack_after_ipa_publish: bool,
    #[cfg(feature = "sunfish-first-wrq-return-oack")]
    pub(crate) wrq_return_writer: bool,
    #[cfg(feature = "sunfish-first-wrq-spare-oack")]
    first_wrq_spare_oack: bool,
    #[cfg(feature = "sunfish-first-oack-spare-buffer")]
    first_oack_108_seen: bool,
    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    first_oack_eligibility: FirstOackEligibility,
    #[cfg(feature = "sunfish-post-reg-oack-turn")]
    post_reg_turns: PostRegOackTurns,
    #[cfg(feature = "sunfish-tftp-first-oack-unheld")]
    first_tftp_unheld: bool,
    #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
    elf_exchange: Option<ElfExchangeTrace>,
    #[cfg(feature="sunfish-post-elf-mcfg-trace")]
    post_elf_mcfg: Option<PostElfMcfgTrace>,
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
    startup_prerequisites: StartupPrerequisites,
/* STARTUP_PREREQ_END */
    #[cfg(feature="sunfish-elf-window-trace")]
    elf_window:Option<ElfWindowTrace>,
    #[cfg(feature="sunfish-elf-tx-slots")]
    elf_tx_slots:Option<ElfTxSlots>,
    #[cfg(feature="sunfish-elf-first-slot-turn")]
    elf_slot_receipt:Option<(RequestHandle,ReplyRoute)>,
    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    first_server_check: Option<ElfServerCheckTrace>,
    #[cfg(feature = "sunfish-ipa-announcement-timeline")]
    ipa_announcement: Option<(u32, Option<u64>)>,
    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    ipa_endpoint_ready_at: Option<u64>,
    #[cfg(feature = "sunfish-wlan-first-wrq-window")]
    first_wrq_window: FirstWrqWindow,
    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    server_check_transfers: [Option<ServerCheckOackTrace>; crate::qcom_tftp_server_check::TRANSFER_COUNT],
    #[cfg(feature = "sunfish-tftp-oack-resources")]
    server_check_resources: [OackResources; crate::qcom_tftp_server_check::TRANSFER_COUNT],
    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
    oack_slot_follow: Option<OackSlotFollow>,
    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    completed_server_check: Option<usize>,
    #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
    elf_host_credit: Option<ElfHostCredit>,
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    linux_ipa: Option<linux_ipa_link::LinuxIpa>,
    owner: u64,
    generation: u64,
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    runtime_profile: bool,
    config: Config,
    router: Router,
    control: Controller,
    receive: [Receive; REQUEST_SLOTS],
    transmit: [Transmit; TX_STORAGE_SLOTS],
    #[cfg(feature = "sunfish-tftp-oack-reserve")]
    reserved_oack: Option<ReservedOack>,
    qrtr_flows: [Option<QrtrFlow>; TX_STORAGE_SLOTS],
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
    efs_overlap: efs_overlap::EfsOverlap,
/* EFS_REPLY_OVERLAP_END */
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    rmtfs_reference_flows: crate::qcom_qrtr_flow::Table<64>,
    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    rmtfs_flow_destinations: [Option<Address>; 64],
    // Closed RMTFS peers may still send a delayed flow credit.
    retired_rmtfs_peers: [Option<Address>; 64],
    remote: [Option<RemoteIntent>; REMOTE_INTENTS],
    next_local_intent: u32,
    intent_request_ack: Option<bool>,
    #[cfg(feature = "sunfish-glink-retry-full-intent")]
    retry_full_intent: bool,
    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    intent_ack_backing: Option<u32>,
    #[cfg(feature = "sunfish-glink-announcement-reuse")]
    intent_ack_reused: Option<u32>,
    #[cfg(feature = "sunfish-registry-converted-grant-turn")]
    registry_converted_grant: Option<(u32, RequestHandle)>,
    #[cfg(feature = "sunfish-grant-ad-writer-trace")]
    pub(crate) grant_backing: GrantBackingTrace,
    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    intent_ack_yield: bool,
    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    intent_ack_summary: (u32, Option<u64>),
    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    intent_ack_clock: Option<fn() -> u64>,
    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    intent_ack_started: Option<u64>,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    tftp_tx_progress: [u32; 3],
    #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
    tftp_queue_block: TftpQueueBlock,
    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
    registry_pressure: Option<(RequestHandle, RegistryQueuePressure)>,
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    first_tftp_endpoint: Option<TftpEndpointTrace>,
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    first_tftp_slot: Option<usize>,
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    mcfg_oack: Option<TftpEndpointTrace>,
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    mcfg_oack_slot: Option<usize>,
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    mcfg_first_data: Option<McfgDataTrace>,
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    final_tftp_ack: Option<TftpFinalAckTrace>,
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    missing_tftp_reply: Option<TftpEndpointTrace>,
    #[cfg(feature = "sunfish-glink-credit-fairness-experiment")]
    credit_sent_since_data: bool,
    #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
    other_sent_since_tftp: bool,
    #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
    first_tftp_schedule: TftpScheduleTrace,
    #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
    tftp_control_turn_due: bool,
    #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
    registry_turn_used: bool,
    #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
    receive_done_since_data: bool,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    outbound_intent_request: OutboundIntentRequest,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    outbound_intent_attempts: u32,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    outbound_intent_blocked: u32,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    intent_gate_calls: u32,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    intent_gate_pending_calls: u32,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    intent_gate_mask: u32,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    intent_gate_combinations: u32,
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    intent_gate_last: u32,
    intent_denial: Option<IntentDenialDiagnostic>,
    #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
    pending_denial_slots: Option<(u32, IntentDenialSlots)>,
    #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
    last_denial_window: Option<IntentDenialWindow>,
    routing_attempt: Option<usize>,
    ipa_client: Option<IpaClient>,
    wlfw_client: Option<WlfwClient>,
    wlfw_power_gate: bool,
    wlfw_power_ready: bool,
    servreg_notif_client: Option<ServregNotifClient>,
    completed_ipa_indication: Option<u64>,
    completed_ipa_service_response: Option<u64>,
    tftp_server: bool,
    last_completion_error: Option<CompletionDiagnostic>,
    last_credit_diagnostic: Option<CreditDiagnostic>,
    client_gone_diagnostic: Option<ClientGoneDiagnostic>,
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    deferred_close_diagnostic: Option<DeferredCloseDiagnostic>,
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pending_tftp_close: Option<(Address, usize, usize)>,
    #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
    test_fail_close_stage: u8,
    // Fail closed until the serialized service owner supplies its current
    // pending-work snapshot immediately before each receive poll.
    #[cfg(feature = "sunfish-tftp-deferred-close-experiment")]
    tftp_owner_busy: bool,
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    tftp_owner_peers: [Option<Address>; TFTP_OWNER_PEER_SLOTS],
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    tftp_owner_snapshot_valid: bool,
    #[cfg(feature = "sunfish-tftp-close-owner-fence")]
    tftp_firmware_peers: [Option<Address>; TFTP_OWNER_PEER_SLOTS],
    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
    pending_firmware_close: Option<(Address, usize)>,
    #[cfg(feature="sunfish-mcfg-close-cleanup")]
    pending_mcfg_close: Option<(Address, usize)>,
    #[cfg(feature="sunfish-mcfg-close-cleanup")]
    mcfg_close_transfers: [Option<(Address,u32)>; crate::qcom_tftp_server_check::TRANSFER_COUNT],
    capacity_diagnostic: Option<CapacityDiagnostic>,
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    next_deferred_sequence: u64,
    quarantined: bool,
}

/// The serialized ELF reply and first complete QRTR datagrams following its
/// validated GLINK completion. Only header metadata is retained.
#[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfExchangeTrace {
    request: RequestHandle,
    pub source: Address,
    pub destination: Address,
    pub transaction: u16,
    pub confirm_rx: u32,
    pub wire_bytes: u16,
    pub qmi_matches_reference: bool,
    pub completed: bool,
    pub following: [Option<ElfQrtrHeader>; 4],
    #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
    pub credit: ElfCreditTrace,
    #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
    pub host_credit: Option<ElfHostCredit>,
    #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
    pub first_tftp_request: Option<ElfTftpRequest>,
    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
    pub server_check: Option<ElfServerCheckTrace>,
}

#[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfServerCheckTrace {
    request: TftpHandle,
    pub peer: Address,
    pub received_at: Option<u64>,
    pub wrq_result: Option<u32>,
    pub coalesced: bool,
    pub transfer_port: Option<u32>,
    pub oack: ElfWriteReply,
    pub data: Option<ElfWriteData>,
    pub final_block: Option<u16>,
    pub ack: ElfWriteReply,
    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    pub before_elf: bool,
    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    pub(crate) exact_wrq: bool,
    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
    pub repeat: Option<ServerCheckRepeat>,
    #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
    pub timing: ElfServerCheckTiming,
}

#[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
impl ElfServerCheckTrace {
    fn new(
        request: TftpHandle,
        peer: Address,
        received_at: Option<u64>,
        #[cfg(feature = "sunfish-wlan-first-server-check-trace")] before_elf: bool,
        #[cfg(feature = "sunfish-wlan-first-server-check-retry")] exact_wrq: bool,
    ) -> Self {
        Self {
            request, peer, received_at, wrq_result: None, coalesced: false,
            transfer_port: None, oack: ElfWriteReply::default(), data: None,
            final_block: None, ack: ElfWriteReply::default(),
            #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
            before_elf,
            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
            exact_wrq,
            #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
            repeat: None,
            #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
            timing: ElfServerCheckTiming::default(),
        }
    }
}

#[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServerCheckRepeat {
    request: Option<TftpHandle>,
    pub coalesced: bool,
    pub dispatch_at: Option<u64>,
    pub result: Option<u32>,
    pub flow_busy: u32,
    pub no_capacity: u32,
    pub queued_at: Option<u64>,
    pub oack: ElfWriteReply,
    pub close_phase: Option<u8>,
    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    eligible: bool,
    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    pub overlapped: bool,
    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    pub cancelled: bool,
    #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
    pub tx: ServerCheckRetryTx,
}

#[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServerCheckRetryTx {
    pub turn_at: Option<u64>,
    pub first_poll_end: Option<u64>,
    pub first_call: Option<u64>,
    pub calls: u32,
    /// Selection in bits 0–7, transmit conditions in bits 8–12.
    pub first: Option<u32>,
    pub last: Option<u32>,
    /// Bits 0–7 accumulate selections 1–8. Bits 8–12 accumulate conditions.
    pub seen: u32,
}

#[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfServerCheckTiming {
    pub dispatch_at: Option<u64>,
    pub ready_at: Option<u64>,
    pub queued_at: Option<u64>,
    pub repeats: u32,
    pub first_repeat_at: Option<u64>,
    pub closes: u32,
    pub first_close_at: Option<u64>,
    pub first_close_oack_phase: Option<u8>,
    #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
    pub queue: ElfOackQueueTrace,
    #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
    pub peer_rx: ElfOackPeerTrace,
}

#[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfOackPeerTrace {
    pub data: ElfPeerReceive,
    pub error: ElfPeerReceive,
    pub repeat: ElfPeerReceive,
}

#[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfPeerReceive {
    pub count: u32,
    pub first: Option<ElfPeerPacket>,
}

#[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfPeerPacket {
    pub destination_port: u32,
    pub body_bytes: u16,
    pub block_or_error: Option<u16>,
    pub received_at: Option<u64>,
}

/// Writer observations while the exact reserved OACK remains queued.
#[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReservedWriterTrace {
    pub calls: u32, pub first_at: Option<u64>, previous_end: Option<u64>,
    pub max_gap_ticks: u64, pub active_ticks: u64, pub writer_ticks: u64,
    pub no_fit: u32, pub intent_wait: u32, pub older_flow: u32, pub ready: u32,
    pub advertisements: u32, pub intent_acks: u32, pub receive_dones: u32,
    pub other_tftp: u32, pub other_data: u32, pub oack_selected: u32,
    pub oack_blocked: u32, pub other_blocked: u32, pub intent_requests: u32,
    pub idle: u32, pub followup_yield: u32,
}

#[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfOackQueueTrace {
    pub calls: u32,
    pub first_at: Option<u64>,
    previous_end: Option<u64>,
    pub max_gap_ticks: u64,
    pub active_ticks: u64,
    pub writer_ticks: u64,
    pub oack_writer_ticks: u64,
    pub no_fit: u32,
    pub intent_wait: u32,
    pub older_flow: u32,
    pub advertisements: u32,
    pub intent_acks: u32,
    pub receive_dones: u32,
    pub other_tftp: u32,
    pub other_data: u32,
    pub oack_blocked: u32,
    pub other_blocked: u32,
    pub intent_requests: u32,
    pub idle: u32,
}

#[cfg(any(feature = "sunfish-wlan-elf-oack-queue-trace", feature = "sunfish-ipa-init-tx-trace"))]
#[derive(Clone, Copy)]
enum PublishSelection {
    #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
    Idle,
    #[cfg(any(feature = "sunfish-wlan-elf-oack-queue-trace", feature = "sunfish-glink-intent-request-experiment"))]
    IntentRequest,
    Advertisement,
    IntentAck,
    ReceiveDone,
    Data(usize),
}

#[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfWriteReply {
    slot: Option<usize>,
    /// 0 absent, 1 prepared by server, 2 queued, 3 published, 4 GLINK RXDONE.
    pub phase: u8,
    pub published_at: Option<u64>,
    pub done_at: Option<u64>,
}

#[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfWriteData {
    request: TftpHandle,
    pub block: u16,
    pub body_bytes: u32,
    pub received_at: Option<u64>,
    pub result: Option<u32>,
    #[cfg(feature = "sunfish-ack-elf-phase-trace")]
    pub stages: FinalAckStages,
}

/// First matching DATA lifecycle observations; these never drive protocol decisions.
#[cfg(feature = "sunfish-ack-elf-phase-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FinalAckStages {
    /// Identity proofs independent of whether an optional observation clock exists.
    pub final_prepared: bool,
    pub final_queued: bool,
    pub dispatch_at: Option<u64>,
    pub prepared_at: Option<u64>,
    pub returned_at: Option<u64>,
    pub queued_at: Option<u64>,
}

#[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfHostCredit {
    request: RequestHandle,
    resume_slot: Option<usize>,
    pub requested: bool,
    pub remote_intent: Option<u32>,
    pub completed: bool,
    /// Publication and completion observed when the ELF reply was published.
    pub before_reply: Option<[bool; 2]>,
}

#[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfTftpRequest {
    pub opcode: u16,
    pub path_kind: &'static str,
    pub body_bytes: u32,
    pub source_port: u32,
    pub destination_port: u32,
}

#[cfg(feature = "sunfish-wlan-elf-credit-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfCreditTrace {
    pub published: bool,
    /// Valid RESUME_TX records keyed to the reply's destination before
    /// publication, before GLINK RXDONE, and after RXDONE.
    pub resumes: [u32; 3],
    pub applied: u32,
    pub first_resume: Option<ElfResumeObservation>,
    pub first_data: Option<ElfQrtrHeader>,
    pub servers: [Option<control::Server>; 4],
}

#[cfg(feature = "sunfish-wlan-elf-credit-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfResumeObservation {
    pub source_port: u32,
    pub local_port: u32,
    pub observed_at: Option<u64>,
}

#[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfQrtrHeader {
    // Version, packet type and actual padded byte length, high to low.
    pub meta: u32,
    pub source_port: u32,
    pub destination_port: u32,
}

/// Bounded post-failure inventory; phase 1 is receive and 2 is publish.
/// Record metadata is captured only for an already-copied inbound record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacityDiagnostic {
    pub phase: u32,
    pub command: u32,
    pub parameter: u32,
    pub rx_free: u32,
    pub tx_free: u32,
    pub remote_free: u32,
    pub remote_busy: u32,
    pub requests: u32,
    pub intent_ack_pending: bool,
    pub flow_used: u32,
    pub flow_active: u32,
    pub retired_peers: u32,
}

/// Metadata captured before quarantining a withdrawal with outstanding work.
/// No request payload or storage contents are exposed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientGoneDiagnostic {
    pub command: u32,
    pub port: u32,
    pub service: Service,
    pub transaction: u16,
    pub message_id: u16,
    pub receive_state: u32,
    pub done_pending: bool,
    pub matching_requests: u32,
    pub reply_count: u32,
    pub reply_len: u32,
    pub reply_sent: u32,
    pub reply_glink_done: bool,
    pub reply_qrtr_done: bool,
}

/// Header-only evidence retained when a closing peer has deferred work.
#[cfg(feature = "sunfish-qrtr-deferred-requests")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeferredCloseDiagnostic {
    pub command: u32,
    pub peer_node: u32,
    pub peer_port: u32,
    pub destination_port: u32,
    pub opcode: u32,
    pub matching: u32,
    pub block_mask: u32,
}

/// Metadata for the first queued OACK. Flags: queued=1, started=2,
/// fully published=4, GLINK RX_DONE=8, QRTR confirm requested=16.
#[cfg(feature = "sunfish-tftp-endpoint-observer")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TftpEndpointTrace {
    pub source: Address,
    pub destination: Address,
    pub payload_bytes: u32,
    pub flags: u32,
    pub remote_intent: u32,
}

/// First DATA or ERROR from the admitted write's peer. No payload is retained.
#[cfg(feature = "sunfish-tftp-mcfg-trace")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McfgDataTrace {
    pub opcode: u16,
    pub block_or_error: Option<u16>,
    pub packet_bytes: u32,
    pub local_port: u32,
}

/// First locally completed server-check write's final ACK, not the first OACK.
/// Flags: final DATA accepted=1, queued=2, started=4, fully published=8,
/// matching GLINK RX_DONE=16. GLINK completion is not application acceptance.
#[cfg(feature = "sunfish-tftp-final-ack-observer")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TftpFinalAckTrace {
    pub source: Address,
    pub destination: Address,
    pub block: u16,
    pub flags: u32,
    pub remote_intent: u32,
    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    pub accepted_at: Option<u64>,
    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    pub published_at: Option<u64>,
    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    pub done_at: Option<u64>,
}

#[cfg(feature = "sunfish-tftp-oack-resources")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OackIntentObservation {
    pub id: u32,
    pub available_at: Option<u64>,
    pub consumed_at: Option<u64>,
}

#[cfg(feature = "sunfish-tftp-oack-resources")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OackResources {
    pub initial: [Option<OackIntentObservation>; RX_SLOTS],
    pub added: [Option<OackIntentObservation>; RX_SLOTS],
    pub omitted: u32,
    pub data_intent: Option<u32>,
    pub close_intent: Option<u32>,
    pub oack_confirm: bool,
    pub peer_resume_at: Option<u64>,
    pub peer_resumes: u32,
    pub applied_resumes: u32,
    pub flow_pending: Option<u8>,
    #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
    pub primed_intent: Option<u32>,
    #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
    pub prime_released: Option<u32>,
}

#[cfg(feature = "sunfish-tftp-oack-resources")]
impl OackResources {
    fn add(&mut self, id: u32, at: Option<u64>) {
        if let Some(entry) = self.added.iter_mut().find(|entry| entry.is_none()) {
            *entry = Some(OackIntentObservation { id, available_at: at, consumed_at: None });
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }
}

#[cfg(feature = "sunfish-tftp-oack-followup")]
#[derive(Clone, Copy)]
pub(crate) enum OackFollowup {
    #[cfg(feature = "sunfish-first-oack-grant-window")]
    FirstGrantAd,
    #[cfg(feature = "sunfish-first-oack-grant-window")]
    FirstGrantInit,
    #[cfg(feature = "sunfish-first-oack-grant-window")]
    FirstGrantAck,
    #[cfg(feature = "sunfish-first-oack-grant-window")]
    FirstGrantReply,
    #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
    CreditReplay,
    Advertise,
    #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
    EarlyReserve,
    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
    Grant,
    Reply,
}

/// First use of the physical wire slot freed for the port 0x49 OACK.
#[cfg(feature = "sunfish-tftp-oack-slot-follow")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OackSlotFollow {
    pub slot: usize,
    pub released: u32,
    pub released_at: Option<u64>,
    pub next_intent: Option<u32>,
    pub available_at: Option<u64>,
    // 1 routine/prime advertisement, 2 grant backing, 3 RX_DONE reuse.
    pub availability_kind: u32,
    pub started: bool,
    pub started_at: Option<u64>,
    pub received_at: Option<u64>,
    pub packet: Option<OackSlotPacket>,
    #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
    pub exact_repeat_done_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-oack-followup")]
    pub followup_ad: u32,
    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
    pub followup_grant: u32,
    #[cfg(feature = "sunfish-tftp-oack-followup")]
    pub followup_reply: u32,
    #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
    pub followup_choice: u32,
    #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
    pub followup_candidate: Option<u32>,
    #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
    pub followup_selected: Option<u32>,
    #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
    pub followup_gates: u32,
    #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
    pub followup_slots: u32,
    #[cfg(feature = "sunfish-tftp-oack-guard")]
    pub advertisement_guard: Option<OackAdvertisementGuard>,
}
#[cfg(feature = "sunfish-tftp-oack-guard")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OackAdvertisementGuard {
    pub oack_at: Option<u64>,
    pub first_record: Option<OackGuardRecord>,
    pub matching: u8,
    pub unsent: u8,
    pub ready_unsent: u8,
    pub grant_pending: bool,
    pub peer_closed: bool,
    pub allowed: bool,
}

#[cfg(feature = "sunfish-tftp-oack-guard")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OackGuardRecord {
    pub slot: u8,
    pub kind: u8,
    pub len: u32,
    pub sent: u32,
}

#[cfg(feature = "sunfish-tftp-oack-slot-follow")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OackSlotPacket {
    pub bytes: u32,
    pub version: u32,
    pub kind: Option<u32>,
    pub source: Option<Address>,
    pub destination: Option<Address>,
    pub opcode: Option<u32>,
}

/// One arbitration sample for the original OACK. The timestamp uses the same
/// counter as the FIFO close bounds. Commands 0x109 and 0x209 distinguish the
/// original OACK and QRTR credit from other GLINK DATA.
#[cfg(feature = "sunfish-post-reg-oack-turn")]
#[derive(Clone, Copy, Debug, Default)]
pub struct PostRegOackTurn {
    pub at: u64,
    pub mask: u16,
    pub command: u16,
    pub path: u8,
    // 0 no selection, 1 selected without a write, 2 committed,
    // 3 known blocked write, 4 uncertain write.
    pub result: u8,
}

#[cfg(feature = "sunfish-post-reg-oack-turn")]
impl PostRegOackTurn {
    fn written(&mut self, wire_command: u16, result: Result<(), WriteError>) {
        self.command = (self.command & 0xff00) | wire_command;
        self.result = match result {
            Ok(()) => 2,
            Err(WriteError::WouldBlock) => 3,
            Err(WriteError::Uncertain) => 4,
        };
    }
}

#[cfg(feature = "sunfish-post-reg-oack-turn")]
#[derive(Default)]
pub struct PostRegOackTurns {
    turns: [PostRegOackTurn; 12],
    len: usize,
    pub omitted: u32,
}

#[cfg(feature = "sunfish-post-reg-oack-turn")]
impl PostRegOackTurns {
    pub fn entries(&self) -> &[PostRegOackTurn] { &self.turns[..self.len] }
}

/// Ordinary writer opportunities for the first accepted WRQ. Masks contain
/// simultaneous blockers. Only passes with the exact IPA reply in flight
/// contribute to the union and intersection.
#[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstOackEligibility {
    pub queued_at: Option<u64>,
    pub passes: u32,
    pub ipa_passes: u32,
    pub ready_passes: u32,
    pub seen: u16,
    pub common: u16,
    pub close_mask: Option<u16>,
    pub ready_command: Option<u16>,
}

/// Host observations for the original accepted WRQ on one transfer port.
#[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerCheckOackTrace {
    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    pub early_oack_attempted: bool,
    intent: u32,
    pub peer: Address,
    pub transfer_port: u32,
    pub wrq_at: Option<u64>,
    pub confirm_requested: bool,
    pub wrq_done_at: Option<u64>,
    #[cfg(feature = "sunfish-wlan-first-wrq-rxdone-turn")]
    pub wrq_priority_published: bool,
    #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
    pub wrq_reused: bool,
    pub resume_at: Option<u64>,
    pub oack_at: Option<u64>,
    pub oack_done_at: Option<u64>,
    /// Advertised local intents still available in the host's receive state.
    pub ready_at_oack: Option<u8>,
    pub first_data_at: Option<u64>,
    pub first_repeat_at: Option<u64>,
    pub first_close_at: Option<u64>,
}

/// Header metadata for a QRTR datagram near the first server-check request.
#[cfg(feature = "sunfish-wlan-first-wrq-window")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WrqNeighbor {
    pub source_port: u32,
    pub destination_port: u32,
    /// QMI wire kind in bits 24..31 and message ID in bits 0..15.
    pub qmi_header: Option<u32>,
    pub at: Option<u64>,
    pub done_at: Option<u64>,
    pub intent: u32,
    tx_slot: Option<usize>,
}

#[cfg(feature = "sunfish-wlan-first-wrq-window")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstWrqWindow {
    pub rx_before: Option<WrqNeighbor>,
    pub rx_after: Option<WrqNeighbor>,
    pub tx_before: Option<WrqNeighbor>,
    pub tx_after: Option<WrqNeighbor>,
    pub rx_after_count: u16,
    pub tx_after_count: u16,
}

#[cfg(feature = "sunfish-wlan-first-wrq-window")]
impl FirstWrqWindow {
    fn neighbor(
        bytes: &[u8], expected_bytes: usize, source: u32, destination: u32,
        clock: Option<fn() -> u64>, intent: u32, tx_slot: Option<usize>,
    ) -> Option<WrqNeighbor> {
        let header = routing::QRTR_HEADER_BYTES;
        if bytes.len() != expected_bytes
            || word(bytes, 0) != 1 || word(bytes, 4) != 1
            || word(bytes, 8) != source || word(bytes, 24) != destination
        {
            return None;
        }
        let size = word(bytes, 20) as usize;
        if size > bytes.len() - header || align(header + size, 4) != bytes.len() {
            return None;
        }
        let body = &bytes[header..header + size];
        let qmi_header = Message::decode(body).ok()
            .map(|message| (u32::from(body[0]) << 24) | u32::from(message.message_id));
        Some(WrqNeighbor {
            source_port: word(bytes, 12),
            destination_port: word(bytes, 28),
            qmi_header,
            at: clock.map(|clock| clock()),
            done_at: None,
            intent,
            tx_slot,
        })
    }

    fn receive(&mut self, after: bool, neighbor: WrqNeighbor) {
        if after {
            self.rx_after_count = self.rx_after_count.saturating_add(1);
            self.rx_after.get_or_insert(neighbor);
        } else {
            self.rx_before = Some(neighbor);
        }
    }

    fn publish(&mut self, after: bool, neighbor: WrqNeighbor) {
        if after {
            self.tx_after_count = self.tx_after_count.saturating_add(1);
            self.tx_after.get_or_insert(neighbor);
        } else {
            self.tx_before = Some(neighbor);
        }
    }

    fn complete(&mut self, slot: usize, intent: u32, clock: Option<fn() -> u64>) {
        for neighbor in [&mut self.tx_before, &mut self.tx_after].into_iter().flatten() {
            if neighbor.tx_slot == Some(slot) && neighbor.intent == intent {
                neighbor.tx_slot = None;
                neighbor.done_at = clock.map(|clock| clock());
            }
        }
    }
}

/// Accumulated scheduler conditions while the first OACK is unpublished.
/// Mask: suitable intent=1, OACK selected=2, another TX selected=4,
/// outbound intent request waiting=8, receive advertisement=16,
/// intent ACK pending=32, receive completion pending=64, writer blocked=128.
#[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TftpScheduleTrace { pub calls: u32, pub mask: u32 }

/// Read-only evidence for the deliberately limited post-auth host profile.
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeProfileReport {
    pub remote_storage_registered: bool,
    pub remote_storage_ready: bool,
    pub service_registry_registered: bool,
    pub ipa_server_registered: bool,
    pub linux_ipa_client_enabled: bool,
    pub tftp_port: Option<u32>,
    pub tftp_server_prepared: bool,
}

/// Identifies the next runtime-profile field being sampled. The diagnostic
/// caller flushes a checkpoint before each read and its value afterward.
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeProfileField {
    RemoteStorageRegistered,
    RemoteStorageReady,
    ServiceRegistryRegistered,
    IpaServerRegistered,
    LinuxIpaClientEnabled,
    TftpPort,
    TftpServerPrepared,
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn align(value: usize, boundary: usize) -> usize {
    (value + boundary - 1) & !(boundary - 1)
}
fn header(
    bytes: &mut [u8],
    kind: u32,
    source: Address,
    destination: Address,
    size: usize,
    confirm: bool,
) {
    for (index, value) in [
        1,
        kind,
        source.node,
        source.port,
        u32::from(confirm),
        size as u32,
        destination.node,
        destination.port,
    ]
    .iter()
    .enumerate()
    {
        put(bytes, index * 4, *value);
    }
}
fn stage_resume_tx(tx: &mut Transmit, local: Address, peer: Address) {
    header(&mut tx.bytes, 7, local, peer, 20, false);
    put(&mut tx.bytes, 32, 7);
    put(&mut tx.bytes, 36, local.node);
    put(&mut tx.bytes, 40, local.port);
    tx.bytes[44..52].fill(0);
    tx.len = 52;
    tx.sent = 0;
    tx.kind = TxKind::Resume;
    tx.remote_slot = None;
    tx.glink_done = false;
    tx.qrtr_done = true;
}
fn glink_header(bytes: &mut [u8], command: u16, channel: u16, parameter: u32) {
    bytes[..2].copy_from_slice(&command.to_le_bytes());
    bytes[2..4].copy_from_slice(&channel.to_le_bytes());
    put(bytes, 4, parameter);
}

impl Session {
    pub fn new(config: Config, generation: u64) -> Result<Self, Error> {
        if config.local_channel == 0 || config.remote_channel == 0 {
            return Err(Error::InvalidChannel);
        }
        let router = Router::new(config.local_node, config.peer_node, generation)?;
        let owner = NEXT_SESSION
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Error::IdExhausted)?;
        Ok(Self {
            #[cfg(feature = "sunfish-linux-ipa-qmi")]
            linux_ipa: None,
            owner,
            generation,
            #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
            runtime_profile: false,
            config,
            router,
            control: Controller::new(config.local_node, config.peer_node),
            receive: [const { Receive::empty() }; REQUEST_SLOTS],
            transmit: [const { Transmit::empty() }; TX_STORAGE_SLOTS],
            #[cfg(feature = "sunfish-tftp-oack-reserve")]
            reserved_oack: None,
            qrtr_flows: [None; TX_STORAGE_SLOTS],
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
            efs_overlap: efs_overlap::EfsOverlap::EMPTY,
/* EFS_REPLY_OVERLAP_END */
            #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
            rmtfs_reference_flows: crate::qcom_qrtr_flow::Table::new(generation).map_err(|_| Error::IdExhausted)?,
            #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
            rmtfs_flow_destinations: [None; 64],
            retired_rmtfs_peers: [None; 64],
            remote: [None; REMOTE_INTENTS],
            next_local_intent: 1,
            intent_request_ack: None,
            #[cfg(feature = "sunfish-glink-retry-full-intent")]
            retry_full_intent: false,
            #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
            intent_ack_backing: None,
            #[cfg(feature = "sunfish-glink-announcement-reuse")]
            intent_ack_reused: None,
            #[cfg(feature = "sunfish-registry-converted-grant-turn")]
            registry_converted_grant: None,
            #[cfg(feature = "sunfish-grant-ad-writer-trace")]
            grant_backing: GrantBackingTrace::default(),
            #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
            intent_ack_yield: false,
            #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
            intent_ack_summary: (0, None),
            #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
            intent_ack_clock: None,
            #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
            intent_ack_started: None,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_tx_progress: [0; 3],
            #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
            tftp_queue_block: TftpQueueBlock::default(),
            #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
            registry_pressure: None,
            #[cfg(feature = "sunfish-registry-window-trace")]
            registry_window: RegistryWindowTrace::default(),
            #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
            elf_exchange: None,
            #[cfg(feature="sunfish-post-elf-mcfg-trace")]
            post_elf_mcfg: None,
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
            startup_prerequisites: StartupPrerequisites::default(),
/* STARTUP_PREREQ_END */
            #[cfg(feature="sunfish-elf-window-trace")]
            elf_window:None,
            #[cfg(feature="sunfish-elf-tx-slots")]
            elf_tx_slots:None,
            #[cfg(feature="sunfish-elf-first-slot-turn")]
            elf_slot_receipt:None,
            #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
            first_server_check: None,
            #[cfg(feature = "sunfish-ipa-announcement-timeline")]
            ipa_announcement: None,
            #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
            ipa_endpoint_ready_at: None,
            #[cfg(feature = "sunfish-wlan-first-wrq-window")]
            first_wrq_window: FirstWrqWindow::default(),
            #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
            server_check_transfers: [None; crate::qcom_tftp_server_check::TRANSFER_COUNT],
            #[cfg(feature = "sunfish-tftp-oack-resources")]
            server_check_resources: [OackResources::default(); crate::qcom_tftp_server_check::TRANSFER_COUNT],
            #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
            oack_slot_follow: None,
            #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
            completed_server_check: None,
            #[cfg(feature = "sunfish-ipa-before-first-oack")]
            ipa_before_first_oack: false,
            #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
            first_oack_after_ipa_publish: false,
            #[cfg(feature = "sunfish-first-wrq-return-oack")]
            wrq_return_writer: false,
            #[cfg(feature = "sunfish-first-wrq-spare-oack")]
            first_wrq_spare_oack: false,
            #[cfg(feature = "sunfish-first-oack-spare-buffer")]
            first_oack_108_seen: false,
            #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
            first_oack_eligibility: FirstOackEligibility::default(),
            #[cfg(feature = "sunfish-post-reg-oack-turn")]
            post_reg_turns: PostRegOackTurns::default(),
            #[cfg(feature = "sunfish-tftp-first-oack-unheld")]
            first_tftp_unheld: false,
            #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
            elf_host_credit: None,
            #[cfg(feature = "sunfish-tftp-endpoint-observer")]
            first_tftp_endpoint: None,
            #[cfg(feature = "sunfish-tftp-endpoint-observer")]
            first_tftp_slot: None,
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            mcfg_oack: None,
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            mcfg_oack_slot: None,
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            mcfg_first_data: None,
            #[cfg(feature = "sunfish-tftp-final-ack-observer")]
            final_tftp_ack: None,
            #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
            missing_tftp_reply: None,
            #[cfg(feature = "sunfish-glink-credit-fairness-experiment")]
            credit_sent_since_data: false,
            #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
            other_sent_since_tftp: false,
            #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
            first_tftp_schedule: TftpScheduleTrace::default(),
            #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
            tftp_control_turn_due: false,
            #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
            registry_turn_used: false,
            #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
            receive_done_since_data: false,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            outbound_intent_request: OutboundIntentRequest::Idle,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            outbound_intent_attempts: 0,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            outbound_intent_blocked: 0,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            intent_gate_calls: 0,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            intent_gate_pending_calls: 0,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            intent_gate_mask: 0,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            intent_gate_combinations: 0,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            intent_gate_last: u32::MAX,
            intent_denial: None,
            #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
            pending_denial_slots: None,
            #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
            last_denial_window: None,
            routing_attempt: None,
            ipa_client: None,
            wlfw_client: None,
            wlfw_power_gate: false,
            wlfw_power_ready: false,
            servreg_notif_client: None,
            completed_ipa_indication: None,
            completed_ipa_service_response: None,
            tftp_server: false,
            last_completion_error: None,
            last_credit_diagnostic: None,
            client_gone_diagnostic: None,
            #[cfg(feature = "sunfish-qrtr-deferred-requests")]
            deferred_close_diagnostic: None,
            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
            pending_tftp_close: None,
            #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
            test_fail_close_stage: 0,
            #[cfg(feature = "sunfish-tftp-deferred-close-experiment")]
            tftp_owner_busy: true,
            #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
            tftp_owner_peers: [None; TFTP_OWNER_PEER_SLOTS],
            #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
            tftp_owner_snapshot_valid: false,
            #[cfg(feature = "sunfish-tftp-close-owner-fence")]
            tftp_firmware_peers: [None; TFTP_OWNER_PEER_SLOTS],
            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
            pending_firmware_close: None,
            #[cfg(feature="sunfish-mcfg-close-cleanup")]
            pending_mcfg_close: None,
            #[cfg(feature="sunfish-mcfg-close-cleanup")]
            mcfg_close_transfers: [None; crate::qcom_tftp_server_check::TRANSFER_COUNT],
            capacity_diagnostic: None,
            #[cfg(feature = "sunfish-qrtr-deferred-requests")]
            next_deferred_sequence: 1,
            quarantined: false,
        })
    }

    /// Construct the post-auth GLINK/QRTR profile. RemoteStorage is only a
    /// decoder until the real retained RAM server is bound; SERVREG is staged
    /// by its owner later, and neither IPA-server nor TFTP is installed.
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn native_runtime_profile(config: Config, generation: u64) -> Result<Self, Error> {
        let mut session = Self::new(config, generation)?;
        session.register(generation, Service::RemoteStorage, 0x4000)?;
        session.enable_linux_ipa(generation, 0x4004)?;
        session.runtime_profile = true;
        Ok(session)
    }

    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn runtime_profile_report(&self) -> RuntimeProfileReport {
        self.runtime_profile_report_with(|_, _| {})
    }

    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn runtime_profile_report_with(
        &self,
        mut checkpoint: impl FnMut(RuntimeProfileField, Option<u32>),
    ) -> RuntimeProfileReport {
        checkpoint(RuntimeProfileField::RemoteStorageRegistered, None);
        let remote_storage_registered =
            self.router.endpoint(Service::RemoteStorage).is_some();
        checkpoint(
            RuntimeProfileField::RemoteStorageRegistered,
            Some(u32::from(remote_storage_registered)),
        );

        checkpoint(RuntimeProfileField::RemoteStorageReady, None);
        let ready = self.control.status().ready_services;
        let remote_storage_ready = ready & 1 != 0;
        checkpoint(
            RuntimeProfileField::RemoteStorageReady,
            Some(u32::from(remote_storage_ready)),
        );

        checkpoint(RuntimeProfileField::ServiceRegistryRegistered, None);
        let service_registry_registered =
            self.router.endpoint(Service::ServiceRegistry).is_some();
        checkpoint(
            RuntimeProfileField::ServiceRegistryRegistered,
            Some(u32::from(service_registry_registered)),
        );

        checkpoint(RuntimeProfileField::IpaServerRegistered, None);
        let ipa_server_registered = self.router.endpoint(Service::IpaServer).is_some();
        checkpoint(
            RuntimeProfileField::IpaServerRegistered,
            Some(u32::from(ipa_server_registered)),
        );

        checkpoint(RuntimeProfileField::LinuxIpaClientEnabled, None);
        let linux_ipa_client_enabled = self.linux_ipa.is_some();
        checkpoint(
            RuntimeProfileField::LinuxIpaClientEnabled,
            Some(u32::from(linux_ipa_client_enabled)),
        );

        checkpoint(RuntimeProfileField::TftpPort, None);
        let tftp_port = self.control.tftp_port();
        checkpoint(
            RuntimeProfileField::TftpPort,
            Some(tftp_port.unwrap_or(u32::MAX)),
        );

        checkpoint(RuntimeProfileField::TftpServerPrepared, None);
        let tftp_server_prepared = self.tftp_server;
        checkpoint(
            RuntimeProfileField::TftpServerPrepared,
            Some(u32::from(tftp_server_prepared)),
        );

        RuntimeProfileReport {
            remote_storage_registered,
            remote_storage_ready,
            service_registry_registered,
            ipa_server_registered,
            linux_ipa_client_enabled,
            tftp_port,
            tftp_server_prepared,
        }
    }

    pub(crate) fn check(&self, generation: u64) -> Result<(), Error> {
        if generation != self.generation {
            return Err(Error::StaleGeneration);
        }
        if self.quarantined {
            return Err(Error::Quarantined);
        }
        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
        if self.pending_tftp_close.is_some() { return Err(Error::FlowBusy); }
        #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
        if self.pending_firmware_close.is_some() { return Err(Error::FlowBusy); }
        #[cfg(feature="sunfish-mcfg-close-cleanup")]
        if self.pending_mcfg_close.is_some() { return Err(Error::FlowBusy); }
        Ok(())
    }

    /// Generation assigned before the transport attaches. A pre-MPSS session
    /// may be prepared in bounded local memory and later transferred to the
    /// live GLINK owner; it must not cross boot generations.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Local protocol progress only; neither a grant nor an intent proves that
    /// a queued payload reached the peer.
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn outbound_intent_diagnostic(&self) -> (u32, u32) {
        match self.outbound_intent_request {
            OutboundIntentRequest::Idle => (0, 0),
            OutboundIntentRequest::Waiting { size, granted: false } => (1, size),
            OutboundIntentRequest::Waiting { size, granted: true } => (2, size),
            OutboundIntentRequest::Done { size, granted: false } => (3, size),
            OutboundIntentRequest::Done { size, granted: true } => (4, size),
        }
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn outbound_intent_attempt_diagnostic(&self) -> (u32, u32) {
        (self.outbound_intent_attempts, self.outbound_intent_blocked)
    }
    /// Accumulated conditions observed at publication turns with unsent data.
    /// Mask: 1 sendable TX, 2 pending peer ACK, 4 advertising receive,
    /// 8 request already used. Bits can come from different turns.
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_diagnostic(&self) -> (u32, u32, u32) {
        (self.intent_gate_calls, self.intent_gate_pending_calls, self.intent_gate_mask)
    }
    /// For unsent-data turns, bit N records one simultaneous gate combination
    /// with four-bit value N. Last is u32::MAX until such a turn occurs.
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_combinations(&self) -> (u32, u32) {
        (self.intent_gate_combinations, self.intent_gate_last)
    }
    /// Current software queue state: queued unsent TFTP count, the selected
    /// ready TX kind (0 none, 1 TFTP, 2 credit, 3 other), and unused intents.
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_queue_snapshot(&self) -> (u32, u32, u32) {
        let tftp = self.transmit.iter().filter(|tx| tx.len != 0 && tx.sent < tx.len
            && matches!(tx.kind, TxKind::TftpReply { .. })).count() as u32;
        let kind = self.next_transmit().map_or(0, |(index, _)| match self.transmit[index].kind {
            TxKind::TftpReply { .. } => 1, TxKind::Resume => 2, _ => 3,
        });
        let free = self.remote.iter().flatten().filter(|intent| !intent.busy).count() as u32;
        (tftp, kind, free)
    }

    /// Whether a new FIFO owner can attach without importing live wire state.
    pub fn transport_is_pristine(&self) -> bool {
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa.as_ref().is_some_and(|ipa| !ipa.pristine()) {
            return false;
        }
        self.receive.iter().all(|rx| rx.state == RxState::Free)
            && self.transmit.iter().all(|tx| tx.len == 0)
            && self.remote.iter().all(Option::is_none)
            && self.intent_request_ack.is_none()
            && {
                #[cfg(feature = "sunfish-glink-intent-request-experiment")]
                { self.outbound_intent_request == OutboundIntentRequest::Idle && self.outbound_intent_attempts == 0 }
                #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
                { true }
            }
            && self.completed_ipa_indication.is_none()
            && self.completed_ipa_service_response.is_none()
            && self.router.in_flight() == 0
            && !self
                .ipa_client
                .as_ref()
                .is_some_and(|client| client.state() == IpaState::Pending)
            && !self.servreg_notif_client.as_ref().is_some_and(|client| {
                matches!(
                    client.state(),
                    servreg_notif::State::Pending
                        | servreg_notif::State::ListenerPending
                        | servreg_notif::State::AckPending
                        | servreg_notif::State::AckFailed
                        | servreg_notif::State::RestartPending
                )
            })
    }

    fn local_client_port_in_use(&self, port: u32) -> bool {
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa_port_in_use(port) {
            return true;
        }
        [
            Service::RemoteStorage,
            Service::HeapMemory,
            Service::ServiceRegistry,
            Service::IpaServer,
        ]
        .iter()
        .any(|service| {
            self.router
                .endpoint(*service)
                .is_some_and(|endpoint| endpoint.address.port == port)
        }) || self
            .ipa_client
            .as_ref()
            .is_some_and(|client| client.local_address().port == port)
            || self
                .wlfw_client
                .as_ref()
                .is_some_and(|client| client.local_address().port == port)
            || self
                .servreg_notif_client
                .as_ref()
                .is_some_and(|client| client.local_address().port == port)
    }

    /// Decoder registration only. Backend readiness must be asserted separately
    /// before the QRTR controller announces or accepts requests for this service.
    pub fn register(&mut self, generation: u64, service: Service, port: u32) -> Result<(), Error> {
        self.check(generation)?;
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa_port_in_use(port) {
            return Err(Error::InvalidReply);
        }
        if self
            .ipa_client
            .as_ref()
            .is_some_and(|client| client.local_address().port == port)
        {
            return Err(ipa_client::Error::EndpointConflict.into());
        }
        if self
            .wlfw_client
            .as_ref()
            .is_some_and(|client| client.local_address().port == port)
        {
            return Err(Error::Wlfw(wlfw::Error::EndpointConflict));
        }
        if self
            .servreg_notif_client
            .as_ref()
            .is_some_and(|client| client.local_address().port == port)
        {
            return Err(servreg_notif::Error::EndpointConflict.into());
        }
        self.router.register(service, port)?;
        Ok(())
    }

    /// Prepare a raw QRTR TFTP observer before control starts. It intentionally
    /// does not register a QMI endpoint or provide any file bytes.
    pub fn prepare_tftp_observer(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.prepare_tftp(generation, local_port, false)
    }

    /// Prepare a raw QRTR TFTP server endpoint before control starts. Unlike
    /// the observer profile, complete TFTP datagrams are held for an explicit
    /// bounded server dispatcher; this method does not attach an image source.
    pub fn prepare_tftp_server(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.prepare_tftp(generation, local_port, true)
    }

    /// Confirm that the TFTP server endpoint was staged before QRTR control
    /// starts, at the expected port, for this session generation.
    pub fn tftp_server_prepared(&self, generation: u64, local_port: u32) -> bool {
        self.check(generation).is_ok()
            && self.transport_is_pristine()
            && self.tftp_server
            && self.control.tftp_server_prepared(local_port)
    }

    fn prepare_tftp(
        &mut self,
        generation: u64,
        local_port: u32,
        server: bool,
    ) -> Result<(), Error> {
        self.check(generation)?;
        if self.control.started() || !self.transport_is_pristine() {
            return Err(control::Error::Busy.into());
        }
        if self.local_client_port_in_use(local_port) {
            return Err(control::Error::Address.into());
        }
        self.control.prepare_tftp_available(local_port)?;
        self.tftp_server = server;
        Ok(())
    }

    /// Reserve one trusted local QRTR client port for the first WLFW
    /// transaction. This must happen before control starts and before any
    /// transport work is queued; it does not advertise a local service.
    pub fn enable_wlfw_client(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.check(generation)?;
        if self.control.started() || !self.transport_is_pristine() {
            return Err(control::Error::Busy.into());
        }
        if self.wlfw_client.is_some() {
            return Err(Error::Wlfw(wlfw::Error::Busy));
        }
        if self.local_client_port_in_use(local_port) {
            return Err(Error::Wlfw(wlfw::Error::EndpointConflict));
        }
        self.wlfw_client = Some(WlfwClient::new(
            Address {
                node: self.config.local_node,
                port: local_port,
            },
            generation,
        )?);
        Ok(())
    }

    /// Reserve the WLFW endpoint but defer every outbound WLFW request until
    /// the ICNSS power sequencer supplies a generation-matched permit. The
    /// native Sunfish runtime uses this path; diagnostic profiles that only
    /// model the existing Linux client may continue to opt into the ungated
    /// method above.
    pub fn enable_wlfw_client_power_gated(
        &mut self,
        generation: u64,
        local_port: u32,
    ) -> Result<(), Error> {
        self.enable_wlfw_client(generation, local_port)?;
        self.wlfw_power_gate = true;
        Ok(())
    }

    /// Authorize the first WLFW indication-registration request only after a
    /// trusted ICNSS power sequencer completed this generation's source-checked
    /// plan. A server must already have been discovered.
    pub fn authorize_wlfw_power(
        &mut self,
        generation: u64,
        permit: PowerReadyPermit,
    ) -> Result<(), Error> {
        self.check(generation)?;
        if !self.wlfw_power_gate
            || permit.generation() != generation
            || self.wlfw_client.as_ref().map(WlfwClient::state) != Some(WlfwState::ReadyToRegister)
        {
            return Err(wlfw::Error::PowerPrerequisite.into());
        }
        self.wlfw_power_ready = true;
        Ok(())
    }

    fn check_wlfw_power(&self) -> Result<(), Error> {
        if self.wlfw_power_gate && !self.wlfw_power_ready {
            Err(wlfw::Error::PowerPrerequisite.into())
        } else {
            Ok(())
        }
    }

    /// Reserve one local QRTR port for the exact IPA `INIT_MODEM_DRIVER`
    /// client. This is opt-in and does not send a request; the caller must
    /// later provide a validated live IPA profile explicitly.
    pub fn enable_ipa_client(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.check(generation)?;
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa.is_some() {
            return Err(Error::InvalidReply);
        }
        if self.control.started() || !self.transport_is_pristine() {
            return Err(control::Error::Busy.into());
        }
        if self.ipa_client.is_some() {
            return Err(ipa_client::Error::Busy.into());
        }
        if self.local_client_port_in_use(local_port) {
            return Err(ipa_client::Error::EndpointConflict.into());
        }
        self.ipa_client = Some(IpaClient::new(
            Address {
                node: self.config.local_node,
                port: local_port,
            },
            generation,
        )?);
        Ok(())
    }

    pub fn ipa_state(&self) -> Option<IpaState> {
        self.ipa_client.as_ref().map(IpaClient::state)
    }

    pub fn ipa_outcome(&self) -> Option<IpaOutcome> {
        self.ipa_client.as_ref().and_then(IpaClient::outcome)
    }

    /// Queue the one-shot IPA request only after the caller has obtained and
    /// validated the live runtime fields. This method performs no discovery,
    /// register access, service startup, or ownership handoff.
    pub fn begin_ipa_init_modem_driver(
        &mut self,
        generation: u64,
        fields: &InitRequestFields,
    ) -> Result<(), Error> {
        self.check(generation)?;
        if !self.control.started() || !self.control.status().peer_hello {
            return Err(control::Error::PeerNotReady.into());
        }
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .ipa_client
            .as_mut()
            .ok_or(ipa_client::Error::NoEndpoint)?
            .begin_init_modem_driver(generation, fields, ipa_client::MAX_DATAGRAM_BYTES)?;
        self.stage_ipa_request(slot, request)
    }

    /// Reserve a local QRTR client port for the single read-only
    /// SERVREG_NOTIF `msm/modem/wlan_pd` state query before peer control starts.
    pub fn enable_servreg_notif_client(
        &mut self,
        generation: u64,
        local_port: u32,
    ) -> Result<(), Error> {
        self.check(generation)?;
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa_port_in_use(local_port) {
            return Err(Error::InvalidReply);
        }
        if self.control.started() || !self.transport_is_pristine() {
            return Err(control::Error::Busy.into());
        }
        if self.wlfw_client.is_some()
            && self.wlfw_client.as_ref().unwrap().local_address().port == local_port
        {
            return Err(servreg_notif::Error::EndpointConflict.into());
        }
        if self.ipa_client.is_some()
            && self.ipa_client.as_ref().unwrap().local_address().port == local_port
        {
            return Err(servreg_notif::Error::EndpointConflict.into());
        }
        for service in [
            Service::RemoteStorage,
            Service::HeapMemory,
            Service::ServiceRegistry,
            Service::IpaServer,
        ] {
            if self
                .router
                .endpoint(service)
                .is_some_and(|endpoint| endpoint.address.port == local_port)
            {
                return Err(servreg_notif::Error::EndpointConflict.into());
            }
        }
        if self.servreg_notif_client.is_some() {
            return Err(servreg_notif::Error::Busy.into());
        }
        self.servreg_notif_client = Some(ServregNotifClient::new(
            Address {
                node: self.config.local_node,
                port: local_port,
            },
            generation,
        )?);
        Ok(())
    }

    pub fn servreg_notif_state(&self) -> Option<servreg_notif::State> {
        self.servreg_notif_client
            .as_ref()
            .map(ServregNotifClient::state)
    }

    pub fn servreg_notif_outcome(&self) -> Option<ServregNotifOutcome> {
        self.servreg_notif_client
            .as_ref()
            .and_then(ServregNotifClient::outcome)
    }

    /// Queue one QMI QUERY_STATE for `msm/modem/wlan_pd` to the validated SERVREG_NOTIF
    /// endpoint. GLINK publication remains in `publish_next`.
    pub fn begin_servreg_notif_query(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .servreg_notif_client
            .as_mut()
            .ok_or(servreg_notif::Error::NoEndpoint)?
            .begin_query(generation, servreg_notif::QUERY_DATAGRAM_BYTES)?;
        self.stage_servreg_notif_request(slot, request)
    }

    /// Queue the one-shot QMI RESTART_PD only after the same client has
    /// accepted a successful current-generation UNINIT state response.
    pub fn begin_servreg_notif_restart_pd(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .servreg_notif_client
            .as_mut()
            .ok_or(servreg_notif::Error::NoEndpoint)?
            .begin_restart_pd(generation, servreg_notif::QUERY_DATAGRAM_BYTES)?;
        self.stage_servreg_notif_request(slot, request)
    }

    /// Register a SERVREG state listener after a successful UNINIT query. The
    /// pinned Lineage recovery client performs this before RESTART_PD.
    pub fn begin_servreg_notif_listener(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .servreg_notif_client
            .as_mut()
            .ok_or(servreg_notif::Error::NoEndpoint)?
            .begin_register_listener(generation, servreg_notif::MAX_DATAGRAM_BYTES)?;
        self.stage_servreg_notif_request(slot, request)
    }

    pub fn wlfw_state(&self) -> Option<WlfwState> {
        self.wlfw_client.as_ref().map(WlfwClient::state)
    }

    pub fn wlfw_outcome(&self) -> Option<WlfwOutcome> {
        self.wlfw_client.as_ref().and_then(WlfwClient::outcome)
    }

    pub fn wlfw_msa_info_outcome(&self) -> Option<WlfwMsaInfoOutcome> {
        self.wlfw_client
            .as_ref()
            .and_then(WlfwClient::msa_info_outcome)
    }

    pub fn wlfw_capability_summary(&self) -> Option<WlfwCapabilitySummary> {
        self.wlfw_client
            .as_ref()
            .and_then(WlfwClient::capability_summary)
    }

    pub fn wlfw_dynamic_feature_mask_done(&self) -> bool {
        self.wlfw_client
            .as_ref()
            .is_some_and(WlfwClient::dynamic_feature_mask_done)
    }

    pub fn wlfw_last_indication(&self) -> Option<WlfwIndicationOutcome> {
        self.wlfw_client
            .as_ref()
            .and_then(WlfwClient::last_indication)
    }

    pub fn wlfw_firmware_ready_seen(&self) -> bool {
        self.wlfw_client
            .as_ref()
            .is_some_and(WlfwClient::firmware_ready_seen)
    }

    /// Queue one source-backed MSA_INFO exchange against the fixed, validated
    /// reservation. The token proves profile geometry only; SCM transfer,
    /// SMMU isolation, and live ownership are separate required gates.
    pub fn begin_wlfw_msa_info(
        &mut self,
        generation: u64,
        reservation: MsaReservation,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_msa_info(generation, reservation, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Continue only after the MSA handoff coordinator has minted a permit
    /// from exact ownership readback of both ranges.
    pub fn begin_wlfw_msa_ready(
        &mut self,
        generation: u64,
        permit: &mut MsaReadyPermit,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_msa_ready(generation, permit, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Start the bounded capability exchange only after WLFW accepts MSA_READY.
    pub fn begin_wlfw_capability_request(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_capability_request(generation, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Send ICNSS's best-effort dynamic-feature-mask request after CAP has
    /// been accepted. This advertises host policy; it does not start WLAN.
    pub fn begin_wlfw_dynamic_feature_mask(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_dynamic_feature_mask(
                generation,
                wlfw::ICNSS_DEFAULT_DYNAMIC_FEATURE_MASK,
                wlfw::MAX_DATAGRAM_BYTES,
            )?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Complete ICNSS's best-effort dynamic-mask step only after the transport
    /// reports its bounded QMI timeout; elapsed time is not inferred here.
    pub fn complete_wlfw_dynamic_feature_mask_timeout(
        &mut self,
        generation: u64,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .complete_dynamic_feature_mask_timeout(generation)?;
        Ok(())
    }

    /// Queue one capability-matched BDF segment through the existing GLINK
    /// transmit slots. A later segment is allowed only after its QMI response
    /// has been parsed and accepted by the WLFW client.
    pub fn begin_wlfw_bdf_download(
        &mut self,
        generation: u64,
        image: &VerifiedBdfImage<'_>,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_bdf_download(generation, image, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    pub fn continue_wlfw_bdf_download(
        &mut self,
        generation: u64,
        image: &VerifiedBdfImage<'_>,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .continue_bdf_download(generation, image, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Queue the exact empty calibration metadata report after every BDF
    /// segment has an accepted status response.
    pub fn begin_wlfw_empty_cal_report(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_empty_cal_report(generation, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Queue a caller-supplied, bounded CE profile after calibration and the
    /// firmware-ready indication. Profile provenance remains platform policy.
    pub fn begin_wlfw_config(
        &mut self,
        generation: u64,
        config: crate::wlfw_protocol::WlanConfig<'_>,
    ) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_wlan_config(generation, config, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Queue mission mode only after the QMI WLAN_CFG acknowledgement.
    pub fn begin_wlfw_mission_mode(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        self.check_wlfw_power()?;
        let slot = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        let request = self
            .wlfw_client
            .as_mut()
            .ok_or(wlfw::Error::NoEndpoint)?
            .begin_wlan_mode(generation, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)?;
        Ok(())
    }

    /// Enable the direct-peer controller after trusted channel attachment.
    /// Queues one HELLO lazily; no native write occurs here. Legacy codec-only
    /// sessions remain available for explicit transport simulation.
    pub fn start_control(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        if !self.transport_is_pristine() {
            return Err(control::Error::Busy.into());
        }
        self.control.start()?;
        Ok(())
    }
    #[cfg(feature = "sunfish-ipa-announcement-timeline")]
    pub(crate) fn first_ipa_announcement(&self) -> Option<(u32, Option<u64>)> {
        self.ipa_announcement
    }
    pub fn control_status(&self) -> control::Status {
        self.control.status()
    }
    pub fn remote_server(&self, index: usize) -> Option<control::Server> {
        self.control.server(index)
    }
    /// Unique WLFW endpoint passively announced by the authenticated peer.
    pub fn wlfw_endpoint(&self) -> Option<routing::Address> {
        self.control.wlfw_endpoint()
    }
    pub fn set_service_available(
        &mut self,
        generation: u64,
        service: Service,
        ready: bool,
    ) -> Result<(), Error> {
        self.check(generation)?;
        let endpoint = self
            .router
            .endpoint(service)
            .ok_or(routing::Error::NoEndpoint)?;
        self.control
            .set_available(service, endpoint.address.port, ready)?;
        Ok(())
    }

    /// Mark one registered local server ready before QRTR control begins.
    /// This changes only bounded local state; it performs no FIFO/GLINK write.
    /// The QRTR controller publishes it after HELLO once control starts.
    pub fn prepare_service_available(
        &mut self,
        generation: u64,
        service: Service,
    ) -> Result<(), Error> {
        self.check(generation)?;
        let endpoint = self
            .router
            .endpoint(service)
            .ok_or(routing::Error::NoEndpoint)?;
        self.control
            .prepare_available(service, endpoint.address.port)?;
        Ok(())
    }

    /// Call ONLY after the native transport has stopped the old peer, drained
    /// old FIFO records/writers, and renegotiated this same channel configuration.
    /// This is a logical reset, not hardware quiescence or SCM revocation.
    pub fn reset_after_transport_reset(&mut self, generation: u64) -> Result<(), Error> {
        if generation <= self.generation {
            return Err(routing::Error::InvalidGeneration.into());
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let new_reference_flows = crate::qcom_qrtr_flow::Table::new(generation).map_err(|_| Error::IdExhausted)?;
        if let Some(client) = self.wlfw_client.as_mut() {
            client.begin_generation(generation)?;
        }
        if let Some(client) = self.ipa_client.as_mut() {
            client.begin_generation(generation)?;
        }
        if let Some(client) = self.servreg_notif_client.as_mut() {
            client.begin_generation(generation)?;
        }
        self.router.begin_generation(generation)?;
        self.generation = generation;
        self.control = Controller::new(self.config.local_node, self.config.peer_node);
        self.tftp_server = false;
        self.receive = [const { Receive::empty() }; REQUEST_SLOTS];
        self.transmit = [const { Transmit::empty() }; TX_STORAGE_SLOTS];
        #[cfg(feature = "sunfish-tftp-oack-reserve")]
        { self.reserved_oack = None; }
        #[cfg(feature = "sunfish-wlan-first-wrq-window")]
        { self.first_wrq_window = FirstWrqWindow::default(); }
        #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
        { self.elf_exchange = None; }
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        { self.post_elf_mcfg = None; }
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
        { self.startup_prerequisites = StartupPrerequisites::default(); }
/* STARTUP_PREREQ_END */
        #[cfg(feature="sunfish-elf-window-trace")]
        {self.elf_window=None;}
        #[cfg(feature="sunfish-elf-tx-slots")]
        {self.elf_tx_slots=None;}
        #[cfg(feature="sunfish-elf-first-slot-turn")]
        {self.elf_slot_receipt=None;}
        #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
        { self.first_server_check = None; }
        #[cfg(feature = "sunfish-ipa-announcement-timeline")]
        { self.ipa_announcement = None; }
        #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
        { self.ipa_endpoint_ready_at = None; }
        #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
        { self.first_oack_eligibility = FirstOackEligibility::default(); }
        #[cfg(feature = "sunfish-post-reg-oack-turn")]
        { self.post_reg_turns = PostRegOackTurns::default(); }
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        {
            self.server_check_transfers = [None; crate::qcom_tftp_server_check::TRANSFER_COUNT];
            #[cfg(feature = "sunfish-tftp-oack-resources")]
            self.server_check_resources.fill(OackResources::default());
            #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
            { self.oack_slot_follow = None; }
            self.completed_server_check = None;
        }
        #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
        { self.elf_host_credit = None; }
        #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
        { self.registry_pressure = None; }
        #[cfg(feature = "sunfish-registry-window-trace")]
        { self.registry_window = RegistryWindowTrace::default(); }
        self.qrtr_flows = [None; TX_STORAGE_SLOTS];
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
        { self.efs_overlap=efs_overlap::EfsOverlap::EMPTY; }
/* EFS_REPLY_OVERLAP_END */
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { self.rmtfs_reference_flows = new_reference_flows; self.rmtfs_flow_destinations = [None; 64]; }
        self.retired_rmtfs_peers = [None; 64];
        self.remote = [None; REMOTE_INTENTS];
        self.intent_request_ack = None;
        #[cfg(feature = "sunfish-glink-announcement-reuse")]
        { self.intent_ack_reused = None; }
        #[cfg(feature = "sunfish-glink-retry-full-intent")]
        { self.retry_full_intent = false; }
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        { self.registry_converted_grant = None; }
        #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
        {
            self.intent_ack_backing = None;
            self.intent_ack_yield = false;
            self.intent_ack_summary = (0, None);
            #[cfg(feature = "sunfish-grant-ad-writer-trace")]
            { self.grant_backing = GrantBackingTrace::default(); }
        }
        #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
        { self.intent_ack_started = None; }
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        { self.tftp_tx_progress = [0; 3]; }
        #[cfg(feature = "sunfish-tftp-endpoint-observer")]
        { self.first_tftp_endpoint = None; self.first_tftp_slot = None; }
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        {
            self.mcfg_oack = None;
            self.mcfg_oack_slot = None;
            self.mcfg_first_data = None;
        }
        #[cfg(feature = "sunfish-tftp-final-ack-observer")]
        { self.final_tftp_ack = None; }
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        { self.missing_tftp_reply = None; }
        #[cfg(feature = "sunfish-glink-credit-fairness-experiment")]
        { self.credit_sent_since_data = false; }
        #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
        { self.other_sent_since_tftp = false; }
        #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
        { self.first_tftp_schedule = TftpScheduleTrace::default(); }
        #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
        { self.tftp_control_turn_due = false; }
        #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
        { self.registry_turn_used = false; }
        #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
        { self.receive_done_since_data = false; }
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        { self.outbound_intent_request = OutboundIntentRequest::Idle; self.outbound_intent_attempts = 0; self.outbound_intent_blocked = 0; self.intent_gate_calls = 0; self.intent_gate_pending_calls = 0; self.intent_gate_mask = 0; self.intent_gate_combinations = 0; self.intent_gate_last = u32::MAX; }
        self.intent_denial = None;
        #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
        {
            self.pending_denial_slots = None;
            self.last_denial_window = None;
        }
        self.routing_attempt = None;
        self.completed_ipa_indication = None;
        self.completed_ipa_service_response = None;
        self.last_completion_error = None;
        self.last_credit_diagnostic = None;
        self.client_gone_diagnostic = None;
        #[cfg(feature = "sunfish-qrtr-deferred-requests")]
        { self.deferred_close_diagnostic = None; }
        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
        { self.pending_tftp_close = None; }
        #[cfg(feature = "sunfish-tftp-deferred-close-experiment")]
        { self.tftp_owner_busy = true; }
        #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
        { self.tftp_owner_peers = [None; TFTP_OWNER_PEER_SLOTS]; self.tftp_owner_snapshot_valid = false; }
        #[cfg(feature = "sunfish-tftp-close-owner-fence")]
        { self.tftp_firmware_peers = [None; TFTP_OWNER_PEER_SLOTS]; }
        #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
        { self.pending_firmware_close = None; }
        #[cfg(feature="sunfish-mcfg-close-cleanup")]
        {self.pending_mcfg_close=None;self.mcfg_close_transfers.fill(None);}
        self.capacity_diagnostic = None;
        #[cfg(feature = "sunfish-qrtr-deferred-requests")]
        { self.next_deferred_sequence = 1; }
        // Local IDs never repeat, including across logical reset.
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if let Some(ipa) = self.linux_ipa.as_mut() {
            ipa.reset();
        }
        self.quarantined = false;
        Ok(())
    }

    pub fn in_flight(&self) -> usize {
        self.router.in_flight()
    }
    #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
    pub fn intent_denial_window(&self) -> Option<IntentDenialWindow> {
        self.last_denial_window
    }

    #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
    fn intent_denial_slots(&self) -> IntentDenialSlots {
        let wire = self.receive_snapshot();
        IntentDenialSlots {
            at: self.intent_ack_clock.map(|clock| clock()),
            slots: core::array::from_fn(|index| {
                let rx = &self.receive[index];
                let (service, reply) = if index < RX_SLOTS {
                    let shift = index * 4;
                    ((wire.services >> shift) & 15, (wire.replies >> shift) & 15)
                } else {
                    let service = rx.context.map_or(0, |context| context.service.index() as u32 + 1);
                    let reply = self.transmit.iter().find(|tx| tx.len != 0
                        && matches!(tx.kind, TxKind::Reply { receive, .. } if receive == index))
                        .map_or(0, |tx| 1 | (u32::from(tx.sent == tx.len) << 1)
                            | (u32::from(tx.glink_done) << 2) | (u32::from(tx.qrtr_done) << 3));
                    (service, reply)
                };
                let flags = rx.state.diagnostic_code() << 12 | service << 8 | reply << 4
                    | u32::from(rx.done_pending);
                (rx.id, flags)
            }),
        }
    }

    pub fn intent_denial_diagnostic(&self) -> Option<IntentDenialDiagnostic> {
        self.intent_denial
    }
    pub fn routing_diagnostic(&self) -> Option<RoutingDiagnostic> {
        let rx = self.receive.get(self.routing_attempt?)?;
        if rx.used < 39 { return None; }
        let body_bytes = word(&rx.bytes, 20) as usize;
        let end = 32usize.checked_add(body_bytes)?;
        if end > rx.used || end < 39 { return None; }
        let port = word(&rx.bytes, 28);
        let service = [Service::RemoteStorage, Service::HeapMemory, Service::ServiceRegistry, Service::IpaServer]
            .into_iter().find(|s| self.router.endpoint(*s).is_some_and(|ep| ep.address.port == port))
            .map_or(u32::MAX, |s| s.descriptor().0);
        let message_id = u16::from_le_bytes([rx.bytes[35], rx.bytes[36]]) as u32;
        let transaction = u16::from_le_bytes([rx.bytes[33], rx.bytes[34]]) as u32;
        let mut tlvs = [u32::MAX; 2];
        let mut at = 39usize;
        for entry in &mut tlvs {
            if at.checked_add(3)? > end { break; }
            let size = u16::from_le_bytes([rx.bytes[at + 1], rx.bytes[at + 2]]) as usize;
            *entry = (u32::from(rx.bytes[at]) << 16) | size as u32;
            at = at.checked_add(3)?.checked_add(size)?;
            if at > end { break; }
        }
        Some(RoutingDiagnostic { service, source: word(&rx.bytes, 12), destination: port, body_bytes: body_bytes as u32,
            kind: u32::from(rx.bytes[32]), message_id, transaction, tlvs })
    }
    #[cfg(feature = "sunfish-grant-buffer-reads")]
    pub(crate) fn grant_available_ids(&self) -> [Option<u32>; RX_SLOTS] {
        ::core::array::from_fn(|i| (self.receive[i].state == RxState::Available).then_some(self.receive[i].id))
    }

    pub fn receive_snapshot(&self) -> ReceiveSnapshot {
        let mut result = ReceiveSnapshot { states: 0, services: 0, replies: 0 };
        for (index, rx) in self.receive[..RX_SLOTS].iter().enumerate() {
            let shift = index * 4;
            result.states |= rx.state.diagnostic_code() << shift;
            let service = rx.context.map(|c| c.service).or_else(|| {
                #[cfg(feature = "sunfish-qrtr-deferred-requests")]
                if rx.state == RxState::Deferred && rx.used >= 32 {
                    let port = word(&rx.bytes, 28);
                    return [Service::RemoteStorage, Service::HeapMemory,
                        Service::ServiceRegistry, Service::IpaServer].into_iter().find(|service|
                        self.router.endpoint(*service).is_some_and(|endpoint| endpoint.address.port == port));
                }
                None
            });
            result.services |= service.map_or(0, |s| s.index() as u32 + 1) << shift;
            let flags = self.transmit.iter().find(|tx| tx.len != 0 && match tx.kind {
                TxKind::Reply { receive, .. } => receive == index,
                #[cfg(feature = "sunfish-linux-ipa-qmi")]
                TxKind::LinuxIpa { receive: Some(receive), .. } => receive == index,
                _ => false,
            }).map_or(0, |tx| 1 | (u32::from(tx.sent == tx.len) << 1)
                | (u32::from(tx.glink_done) << 2) | (u32::from(tx.qrtr_done) << 3));
            result.replies |= flags << shift;
        }
        result
    }
    #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
    pub(crate) fn tftp_queue_block(&self) -> TftpQueueBlock { self.tftp_queue_block }

    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
    pub(crate) fn tftp_tx_occupied(&self) -> u8 {
        self.transmit[..TX_SLOTS].iter().enumerate().fold(0, |mask, (index, tx)| {
            mask | (u8::from(tx.len != 0) << index)
        })
    }

    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
    pub(crate) fn tftp_tx_slots(&self) -> [TftpTxSlot; TX_SLOTS] {
        core::array::from_fn(|index| {
            let tx = &self.transmit[index];
            TftpTxSlot {
                kind: if tx.len == 0 { 0 } else { tx.kind.diagnostic_kind() },
                opcode: if matches!(tx.kind, TxKind::TftpReply { .. }) {
                    tx.bytes[routing::QRTR_HEADER_BYTES + 1]
                } else { 0 },
                flags: u8::from(tx.sent != 0) | (u8::from(tx.len != 0 && tx.sent == tx.len) << 1)
                    | (u8::from(tx.glink_done) << 2) | (u8::from(tx.qrtr_done) << 3),
                peer_port: tx.kind.qrtr_addresses().map_or_else(
                    || if tx.len >= routing::QRTR_HEADER_BYTES { word(&tx.bytes, 28) } else { u32::MAX },
                    |(_, peer)| peer.port),
                remote_intent: tx.remote_slot.and_then(|slot| self.remote[slot])
                    .map_or(u32::MAX, |intent| intent.id),
            }
        })
    }

    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_queue_snapshot(&self) -> TftpQueueSnapshot {
        let mut result = TftpQueueSnapshot { metadata: [u32::MAX; 5], peers: [u32::MAX; 5], total: 0, tx_flags: 0, tx_count: 0,
            all_tx_used: self.transmit.iter().filter(|tx| tx.len != 0).count() as u32,
            flow_used: self.qrtr_flows.iter().flatten().count() as u32 };
        for rx in self.receive.iter().filter(|rx| rx.state == RxState::Tftp) {
            let index = result.total as usize;
            result.total = result.total.saturating_add(1);
            if index >= result.metadata.len() { continue; }
            let len = (word(&rx.bytes, 20) as usize).min(MAX_DATAGRAM_BYTES - routing::QRTR_HEADER_BYTES);
            let packet = &rx.bytes[routing::QRTR_HEADER_BYTES..routing::QRTR_HEADER_BYTES + len];
            let opcode = if len >= 2 { u16::from_be_bytes([packet[0], packet[1]]) as u32 } else { 0xff };
            let port = word(&rx.bytes, 28);
            result.metadata[index] = (len as u32) << 16 | opcode.min(0xff)
                | (u32::from(port == crate::qcom_tftp_server_check::SERVICE_PORT) << 8)
                | (u32::from(crate::qcom_tftp_server_check::is_transfer_port(port)) << 9)
                | (u32::from(packet == crate::qcom_tftp_server_check::REQUEST) << 10);
            result.peers[index] = word(&rx.bytes, 12);
        }
        for tx in self.transmit.iter().filter(|tx| tx.len != 0 && matches!(tx.kind, TxKind::TftpReply { .. })) {
            result.tx_count = result.tx_count.saturating_add(1);
            result.tx_flags |= 1 | (u32::from(tx.sent != 0) << 1)
                | (u32::from(tx.sent == tx.len) << 2) | (u32::from(tx.glink_done) << 3)
                | (u32::from(tx.qrtr_done) << 4);
        }
        result
    }
    /// Successfully started, fully published, and GLINK-completed TFTP TXs.
    /// Completion is a transport receipt, not proof of TFTP acceptance.
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_tx_progress(&self) -> [u32; 3] { self.tftp_tx_progress }
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    pub fn tftp_endpoint_trace(&self) -> Option<TftpEndpointTrace> { self.first_tftp_endpoint }
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    pub fn tftp_final_ack_trace(&self) -> Option<TftpFinalAckTrace> { self.final_tftp_ack }
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    pub fn mcfg_trace(&self) -> (Option<TftpEndpointTrace>, Option<McfgDataTrace>) {
        (self.mcfg_oack, self.mcfg_first_data)
    }

    /// Flags record admission, queueing, start, publication and GLINK completion.
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    pub(crate) fn observe_mcfg_oack(&mut self, route: TftpReplyRoute) {
        if self.mcfg_oack.is_none() {
            self.mcfg_oack = Some(TftpEndpointTrace {
                source: route.source, destination: route.destination,
                payload_bytes: crate::qcom_tftp_server_check::MCFG_WRITE_OACK.len() as u32,
                flags: 1, remote_intent: u32::MAX,
            });
        }
    }

    /// Missing-file trace uses the final-ACK flag meanings (1/2/4/8/16),
    /// not the first-OACK trace's flags. No QRTR confirmation flag is added.
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub fn tftp_missing_reply_trace(&self) -> Option<TftpEndpointTrace> { self.missing_tftp_reply }
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub(crate) fn observe_tftp_missing_reply(&mut self, route: TftpReplyRoute) {
        if self.missing_tftp_reply.is_none() {
            self.missing_tftp_reply = Some(TftpEndpointTrace { source: route.source, destination: route.destination,
                payload_bytes: crate::qcom_tftp_server_check::MISSING_REPLY.len() as u32, flags: 1, remote_intent: u32::MAX });
        }
    }


    /// Called only after the restricted RAM server accepts a final DATA block.
    /// Metadata only: never changes flow control, packets or ownership.
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    pub(crate) fn observe_tftp_final_ack(&mut self, route: TftpReplyRoute, ack: &[u8]) {
        #[cfg(feature = "sunfish-tftp-first-oack-unheld")]
        if self.first_tftp_unheld && !self.server_check_oacks()[0].is_some_and(|trace|
            trace.peer == route.destination && trace.transfer_port == route.source.port)
        {
            return;
        }
        if self.final_tftp_ack.is_none() && ack.len() == 4 && ack[..2] == [0, 4] {
            self.final_tftp_ack = Some(TftpFinalAckTrace {
                source: route.source, destination: route.destination,
                block: u16::from_be_bytes([ack[2], ack[3]]), flags: 1,
                remote_intent: u32::MAX,
                #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
                accepted_at: self.intent_ack_clock.map(|clock| clock()),
                #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
                published_at: None,
                #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
                done_at: None,
            });
        }
    }

    #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
    pub fn tftp_schedule_trace(&self) -> TftpScheduleTrace { self.first_tftp_schedule }
    #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
    fn note_tftp_writer_blocked(&mut self) {
        if self.first_tftp_slot.is_some_and(|i| self.transmit[i].len != 0 && self.transmit[i].sent < self.transmit[i].len) {
            self.first_tftp_schedule.mask |= 128;
        }
    }
    pub fn capacity_diagnostic(&self) -> Option<CapacityDiagnostic> {
        self.capacity_diagnostic
    }
    #[allow(dead_code)] // Standalone session tests omit the FIFO adapter.
    pub(crate) fn record_capacity(&mut self, phase: u32, record: Option<&[u8]>) {
        if self.capacity_diagnostic.is_some() { return; }
        let (command, parameter) = match record {
            Some(bytes) if bytes.len() >= 8 => (u32::from(u16::from_le_bytes([bytes[0], bytes[1]])), word(bytes, 4)),
            _ => (u32::MAX, u32::MAX),
        };
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let (flow_used, flow_active) = self.rmtfs_reference_flows.usage();
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        let (flow_used, flow_active) = (usize::MAX, usize::MAX);
        self.capacity_diagnostic = Some(CapacityDiagnostic {
            phase, command, parameter,
            rx_free: self.receive[..RX_SLOTS].iter().filter(|rx| rx.state == RxState::Free).count() as u32,
            tx_free: self.transmit[..TX_SLOTS].iter().filter(|tx| tx.len == 0).count() as u32,
            remote_free: self.remote.iter().filter(|entry| entry.is_none()).count() as u32,
            remote_busy: self.remote.iter().flatten().filter(|entry| entry.busy).count() as u32,
            requests: self.router.in_flight() as u32,
            intent_ack_pending: self.intent_request_ack.is_some(),
            flow_used: flow_used as u32, flow_active: flow_active as u32,
            retired_peers: self.retired_rmtfs_peers.iter().flatten().count() as u32,
        });
    }
    #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
    pub(crate) fn elf_exchange(&self) -> Option<ElfExchangeTrace> {
        #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
        return self.elf_exchange.map(|exchange| ElfExchangeTrace {
            host_credit: self.elf_host_credit.filter(|credit| credit.request == exchange.request),
            ..exchange
        });
        #[cfg(not(feature = "sunfish-wlan-elf-host-credit-trace"))]
        self.elf_exchange
    }

    /// Inspect the already-queued reply for the owner's exact first ELF request.
    #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
    pub(crate) fn observe_elf_reply(&mut self, handle: &RequestHandle) {
        if self.elf_exchange.is_some() || handle.owner != self.owner || handle.generation != self.generation {
            return;
        }
        let Some(receive) = self.receive.iter().position(|rx|
            rx.id == handle.intent && rx.state == RxState::ReplyQueued)
        else { return };
        let Some(tx) = self.transmit.iter().find(|tx| tx.len != 0
            && matches!(tx.kind, TxKind::Reply { receive: index, route, .. }
                if index == receive && route.service == Service::ServiceRegistry))
        else { return };
        let TxKind::Reply { route, .. } = tx.kind else { return };
        // Linux 2267e248 qcom_pd_mapper.c and qcom_pdr_msg.c describe
        // success, revision 1, and one wlan_pd row with no service data.
        const LINUX_ELF_BODY: &[u8] = b"\x02\x04\x00\x00\x00\x00\x00\x10\x02\x00\x01\x00\x11\x02\x00\x01\x00\x12\x1c\x00\x01\x11msm/modem/wlan_pd\xb4\x00\x00\x00\x00\x00\x00\x00\x00";
        let bytes = &tx.bytes[..tx.len];
        let qrtr_body = word(bytes, 20) as usize;
        let qmi_matches_reference = bytes.get(32..32 + qrtr_body)
            .and_then(|packet| Message::decode(packet).ok())
            .is_some_and(|message|
                message.kind == qmi::Kind::Response
                && message.transaction == route.transaction
                && message.message_id == crate::qcom_servreg::GET_DOMAIN_LIST
                && message.as_bytes().get(7..) == Some(LINUX_ELF_BODY));
        self.elf_exchange = Some(ElfExchangeTrace {
            request: *handle,
            source: Address { node: word(bytes, 8), port: word(bytes, 12) },
            destination: Address { node: word(bytes, 24), port: word(bytes, 28) },
            transaction: u16::from_le_bytes([bytes[33], bytes[34]]),
            confirm_rx: word(bytes, 16),
            wire_bytes: tx.len as u16,
            qmi_matches_reference,
            completed: false,
            following: [None; 4],
            #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
            credit: ElfCreditTrace::default(),
            #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
            host_credit: None,
            #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
            first_tftp_request: None,
            #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
            server_check: None,
        });
    }

    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
    fn server_check_traces_mut(&mut self) -> impl Iterator<Item = &mut ElfServerCheckTrace> {
        let post_elf = self.elf_exchange.as_mut().and_then(|trace| trace.server_check.as_mut());
        #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
        return [self.first_server_check.as_mut(), post_elf].into_iter().flatten();
        #[cfg(not(feature = "sunfish-wlan-first-server-check-trace"))]
        post_elf.into_iter()
    }

    #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
    pub(crate) fn original_1f_close_count(&self) -> Option<u32> {
        self.first_server_check.as_ref()
            .filter(|first| first.exact_wrq && first.peer.node == 0 && first.peer.port == 0x1f)
            .map(|first| first.timing.closes)
    }

    /// Correlate the just-ingested wire intent with the retained first repeat's
    /// per-datagram identity before its receive state can be finished or reused.
    #[cfg(feature = "sunfish-oack-retry-close")]
    pub(crate) fn first_tftp_repeat_on_intent(&self, intent: u32) -> bool {
        self.first_server_check.and_then(|write| write.repeat).and_then(|repeat| repeat.request)
            .is_some_and(|handle| self.tftp_index(&handle).is_ok_and(|index| self.receive[index].id == intent))
    }

    /// Original accepted transfer only; never substitute another completed peer.
    /// Read-only metadata: no clock, validation token or transport operation.
    #[cfg(feature = "sunfish-first-data-ack-elf-trace")]
    pub(crate) fn original_tftp_write_trace(&self) -> Option<ElfServerCheckTrace> {
        let first = self.server_check_oacks()[0]?;
        self.first_server_check.filter(|write|
            write.request.owner == self.owner && write.request.generation == self.generation
                && write.request.intent == first.intent && write.peer == first.peer
                && write.transfer_port == Some(first.transfer_port))
    }

    /// Scheduling proof for the observed single-DATA original server check.
    /// Resolve its immutable transfer slot directly: observation timestamps,
    /// inventory sort order and the first completed peer are not policy inputs.
    #[cfg(feature = "sunfish-original-final-ack-turn")]
    pub(crate) fn original_final_ack_queued(&self) -> bool {
        let Some(write) = self.first_server_check else { return false; };
        let Some(port) = write.transfer_port else { return false; };
        let Some(index) = port.checked_sub(crate::qcom_tftp_server_check::TRANSFER_PORT)
            .and_then(|n| usize::try_from(n).ok()) else { return false; };
        let Some(Some(first)) = self.server_check_transfers.get(index) else { return false; };
        if write.request.owner != self.owner || write.request.generation != self.generation
            || write.request.intent != first.intent || write.peer != first.peer
            || port != first.transfer_port || write.ack.phase != 2 { return false; }
        let Some(data) = write.data else { return false; };
        if data.request.owner != self.owner || data.request.generation != self.generation
            || data.result != Some(0) || write.final_block != Some(data.block)
            || !data.stages.final_prepared || !data.stages.final_queued { return false; }
        let Some(tx) = write.ack.slot.and_then(|slot| self.transmit.get(slot)) else { return false; };
        let source = Address { node: self.config.local_node, port };
        matches!(tx.kind, TxKind::TftpReply { source: from, destination } if from == source && destination == write.peer)
            && tx.len == 36 && tx.sent < tx.len && word(&tx.bytes, 20) == 4
            && tx.bytes[32..36] == [0, 4, (data.block >> 8) as u8, data.block as u8]
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    pub(crate) fn first_server_check(&self) -> Option<ElfServerCheckTrace> {
        self.first_server_check
    }

    #[cfg(feature = "sunfish-wlan-first-wrq-window")]
    pub(crate) fn first_wrq_window(&self) -> FirstWrqWindow {
        self.first_wrq_window
    }

    #[cfg(feature = "sunfish-wlan-first-wrq-window")]
    fn wrq_window_phase(&self) -> Option<bool> {
        if !self.tftp_server {
            return None;
        }
        match self.first_server_check {
            None => Some(false),
            Some(write) if write.before_elf && write.exact_wrq && write.timing.closes == 0 => Some(true),
            Some(_) => None,
        }
    }

    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    pub(crate) fn server_check_oacks(&self) -> [Option<ServerCheckOackTrace>; 2] {
        let first = self.server_check_transfers.iter().flatten()
            .min_by_key(|trace| trace.wrq_at.unwrap_or(u64::MAX)).copied();
        let completed = self.completed_server_check.and_then(|slot| self.server_check_transfers[slot]);
        [first, completed]
    }

    #[cfg(feature = "sunfish-tftp-server-inventory")]
    pub(crate) fn server_check_inventory(&self) -> &[Option<ServerCheckOackTrace>] {
        &self.server_check_transfers
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    pub(crate) fn server_check_49(&self) -> Option<ServerCheckOackTrace> {
        self.server_check_transfers.iter().flatten()
            .find(|trace| trace.peer.port == 0x49).copied()
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    pub(crate) fn oack_resources(&self, transfer_port: u32) -> Option<&OackResources> {
        let slot = transfer_port.checked_sub(crate::qcom_tftp_server_check::TRANSFER_PORT)?;
        self.server_check_resources.get(slot as usize)
    }

    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
    pub(crate) fn oack_slot_follow(&self) -> Option<OackSlotFollow> {
        self.oack_slot_follow
    }

    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
    fn follow_oack_slot_available(&mut self, slot: usize, kind: u32) {
        if let Some(trace) = self.oack_slot_follow.as_mut()
            && trace.slot == slot && trace.next_intent.is_none()
        {
            trace.next_intent = Some(self.receive[slot].id);
            trace.available_at = self.intent_ack_clock.map(|clock| clock());
            trace.availability_kind = kind;
        }
    }

    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
    fn follow_oack_slot_packet(&mut self, slot: usize, first: bool, last: bool) {
        let Some(trace) = self.oack_slot_follow.as_mut() else { return };
        let rx = &self.receive[slot];
        if trace.slot != slot || trace.next_intent != Some(rx.id) || trace.packet.is_some() {
            return;
        }
        if first {
            trace.started = true;
            trace.started_at = self.intent_ack_clock.map(|clock| clock());
        }
        if !last || !trace.started {
            return;
        }

        let bytes = &rx.bytes[..rx.used];
        let mut packet = OackSlotPacket {
            bytes: rx.used as u32, version: u32::from(bytes[0]),
            kind: None, source: None, destination: None, opcode: None,
        };
        if bytes.len() >= 32 && word(bytes, 0) == 1 {
            let size = word(bytes, 20) as usize;
            if size <= bytes.len() - 32 && align(32 + size, 4) == bytes.len() {
                let kind = word(bytes, 4);
                let source = Address { node: word(bytes, 8), port: word(bytes, 12) };
                let destination = Address { node: word(bytes, 24), port: word(bytes, 28) };
                packet.kind = Some(kind);
                packet.source = Some(source);
                packet.destination = Some(destination);
                if kind != 1 && size >= 4 {
                    packet.opcode = Some(word(bytes, 32));
                } else if kind == 1 && size >= 2
                    && (crate::qcom_tftp_server_check::is_transfer_port(destination.port)
                        || destination.port == crate::qcom_tftp_server_check::SERVICE_PORT)
                {
                    packet.opcode = Some(u32::from(u16::from_be_bytes([bytes[32], bytes[33]])));
                }
            }
        }
        trace.packet = Some(packet);
        trace.received_at = self.intent_ack_clock.map(|clock| clock());
    }

    #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
    fn exact_followed_wrq(&self) -> Option<(usize, Address)> {
        let trace = self.oack_slot_follow?;
        let packet = trace.packet?;
        let peer = packet.source?;
        let rx = &self.receive[trace.slot];
        if trace.next_intent != Some(rx.id) || peer != (Address { node: self.config.peer_node, port: 0x49 })
            || packet.destination != Some(Address { node: self.config.local_node,
                port: crate::qcom_tftp_server_check::SERVICE_PORT })
            || packet.kind != Some(1)
            || !matches!(rx.state, RxState::Tftp | RxState::Control)
        {
            return None;
        }
        let payload = rx.bytes.get(32..32 + word(&rx.bytes, 20) as usize)?;
        (payload == crate::qcom_tftp_server_check::REQUEST).then_some((trace.slot, peer))
    }

    #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
    pub(crate) fn coalesce_followed_wrq(&self, handle: &TftpHandle) -> bool {
        let Some((slot, peer)) = self.exact_followed_wrq() else { return false };
        let Some(original) = self.server_check_transfers.iter().flatten()
            .find(|trace| trace.peer == peer && trace.first_close_at.is_none()) else { return false };
        self.tftp_index(handle) == Ok(slot) && self.transmit.iter().any(|tx|
            tx.server_check_wrq == Some(original.intent) && tx.sent == 0 && tx.len != 0
                && matches!(tx.kind, TxKind::TftpReply { destination, .. } if destination == peer))
    }

    // Limit the intervention to the original peer 0x49 OACK.
    #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
    fn server_check_prime_target(&self, next: Option<(usize, usize)>,
        #[cfg(feature = "sunfish-first-oack-spare-buffer")] ordinary: bool) -> Option<usize>
    {
        let tx = &self.transmit[next?.0];
        let intent = tx.server_check_wrq?;
        #[cfg(feature = "sunfish-first-oack-spare-buffer")]
        if ordinary && self.is_unpublished_first_oack(tx)
            && matches!(tx.kind, TxKind::TftpReply { source, destination }
                if source.port == 0x5001 && destination == (Address { node: self.config.peer_node, port: 0x1f }))
            && !self.first_oack_108_seen
            && self.first_server_check.is_some_and(|first| first.before_elf && first.exact_wrq
                && first.peer.port == 0x1f && first.timing.first_close_at.is_none())
        {
            // This first-client experiment can use only unowned capacity.
            // In particular it must never enter the older Control-slot release.
            if !self.receive[..RX_SLOTS].iter().any(|rx|
                matches!(rx.state, RxState::Free | RxState::Advertising)) { return None; }
            let transfer = self.server_check_transfers.iter().position(|trace|
                trace.is_some_and(|trace| trace.intent == intent && trace.first_close_at.is_none()
                    && trace.ready_at_oack.is_none()))?;
            return self.server_check_resources[transfer].primed_intent.is_none().then_some(transfer);
        }
        if tx.sent != 0 || !matches!(tx.kind, TxKind::TftpReply { destination, .. }
                if destination.port == 0x49)
            || self.receive[..RX_SLOTS].iter().any(|rx| rx.state == RxState::Available)
        {
            return None;
        }
        let transfer = self.server_check_transfers.iter().position(|trace|
            trace.is_some_and(|trace| trace.intent == intent && trace.first_close_at.is_none()
                && trace.ready_at_oack.is_none()))?;
        self.server_check_resources[transfer].primed_intent.is_none().then_some(transfer)
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    fn observe_oack_intent_available(&mut self, id: u32) {
        let clock = self.intent_ack_clock;
        for (_, resources) in self.server_check_transfers.iter().zip(&mut self.server_check_resources)
            .filter_map(|(trace, resources)| trace.as_ref().map(|trace| (trace, resources)))
            .filter(|(trace, _)| trace.oack_at.is_some() && trace.first_data_at.is_none()
                && trace.first_close_at.is_none())
        {
            resources.add(id, clock.map(|clock| clock()));
        }
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    fn observe_oack_intent_consumed(&mut self, id: u32) {
        let clock = self.intent_ack_clock;
        for (_, resources) in self.server_check_transfers.iter().zip(&mut self.server_check_resources)
            .filter_map(|(trace, resources)| trace.as_ref().map(|trace| (trace, resources)))
            .filter(|(trace, _)| trace.oack_at.is_some() && trace.first_data_at.is_none()
                && trace.first_close_at.is_none())
        {
            if let Some(entry) = resources.added.iter_mut().rev()
                .chain(resources.initial.iter_mut()).flatten()
                .find(|entry| entry.id == id && entry.consumed_at.is_none())
            {
                entry.consumed_at = clock.map(|clock| clock());
            }
        }
    }

    #[cfg(feature = "sunfish-tftp-oack-reserve")]
    pub fn reserved_oack(&self) -> Option<(ReservedOack, Option<ServerCheckOackTrace>)> {
        let oack = self.reserved_oack?;
        let transfer = self.server_check_transfers.iter().flatten()
            .find(|trace| trace.peer == oack.peer && trace.transfer_port == oack.transfer_port).copied();
        Some((oack, transfer))
    }

    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    fn observe_server_check_rx(&mut self, index: usize, peer: Address, local: Address, len: usize) {
        use crate::qcom_tftp_server_check::{SERVICE_PORT, TRANSFER_PORT, matches_request};

        let rx = &mut self.receive[index];
        let packet = &rx.bytes[32..len];
        let clock = self.intent_ack_clock;
        if local.port == SERVICE_PORT && rx.server_check_wrq.is_none() && matches_request(packet) {
            let at = clock.map(|clock| clock());
            let intent = rx.tftp_datagram_id();
            rx.server_check_wrq = Some(ServerCheckOackTrace {
                #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
                early_oack_attempted: false,
                intent, peer, transfer_port: 0, wrq_at: at,
                confirm_requested: word(&rx.bytes, 16) == 1,
                wrq_done_at: None,
                #[cfg(feature = "sunfish-wlan-first-wrq-rxdone-turn")]
                wrq_priority_published: false,
                #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
                wrq_reused: false,
                resume_at: None, oack_at: None, oack_done_at: None,
                ready_at_oack: None, first_data_at: None, first_repeat_at: None, first_close_at: None,
            });
            if let Some(trace) = self.server_check_transfers.iter_mut().flatten()
                .find(|trace| trace.peer == peer && trace.intent != intent)
                && trace.first_repeat_at.is_none()
            {
                trace.first_repeat_at = at;
            }
        } else if let Some(transfer) = local.port.checked_sub(TRANSFER_PORT)
            && let Some(Some(trace)) = self.server_check_transfers.get_mut(transfer as usize)
            && trace.peer == peer && trace.first_data_at.is_none()
            && matches!(packet, [0, 3, _, _, ..])
        {
            trace.first_data_at = clock.map(|clock| clock());
            #[cfg(feature = "sunfish-tftp-oack-resources")]
            if trace.oack_at.is_some() && trace.first_close_at.is_none() {
                self.server_check_resources[transfer as usize].data_intent = Some(rx.id);
            }
        }
    }

    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    fn admit_server_check_trace(&mut self, handle: &TftpHandle, port: u32) {
        use crate::qcom_tftp_server_check::TRANSFER_PORT;

        let Ok(index) = self.tftp_index(handle) else { return };
        let Some(mut trace) = self.receive[index].server_check_wrq else { return };
        let Some(transfer) = port.checked_sub(TRANSFER_PORT)
            .and_then(|slot| self.server_check_transfers.get_mut(slot as usize))
        else { return };
        trace.transfer_port = port;
        transfer.get_or_insert(trace);
    }

    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    fn server_check_trace_for_intent(&mut self, intent: u32) -> Option<&mut ServerCheckOackTrace> {
        self.server_check_transfers.iter_mut().flatten().find(|trace| trace.intent == intent)
    }

    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    fn observe_server_check_publish(&mut self, index: usize) {
        let tx = &self.transmit[index];
        let Some(intent) = tx.server_check_wrq else { return };
        let clock = self.intent_ack_clock;
        if matches!(tx.kind, TxKind::Resume) {
            let trace = if let Some(trace) = self.server_check_transfers.iter_mut().flatten()
                .find(|trace| trace.intent == intent)
            {
                Some(trace)
            } else {
                self.receive.iter_mut().find(|rx| rx.tftp_datagram_id() == intent)
                    .and_then(|rx| rx.server_check_wrq.as_mut())
            };
            if let Some(trace) = trace { trace.resume_at = clock.map(|clock| clock()); }
        } else {
            let ready = self.receive.iter().filter(|rx| rx.state == RxState::Available).count() as u8;
            #[cfg(feature = "sunfish-tftp-oack-resources")]
            let initial = core::array::from_fn(|slot| {
                let rx = &self.receive[slot];
                (rx.state == RxState::Available).then_some(OackIntentObservation {
                    id: rx.id, available_at: None, consumed_at: None,
                })
            });
            #[cfg(feature = "sunfish-tftp-oack-resources")]
            let confirm = word(&tx.bytes, 16) == 1;
            #[cfg(all(feature = "sunfish-tftp-oack-resources", feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
            let flow_pending = tx.reference_flow.as_ref()
                .and_then(|flow| self.rmtfs_reference_flows.pending_for_diagnostic(flow.destination()));
            #[cfg(all(feature = "sunfish-tftp-oack-resources", not(feature = "sunfish-qrtr-rmtfs-flow-experiment")))]
            let flow_pending = None;
            if let Some(trace) = self.server_check_trace_for_intent(intent)
                && trace.ready_at_oack.is_none()
            {
                trace.oack_at = clock.map(|clock| clock());
                trace.ready_at_oack = Some(ready);
                #[cfg(feature = "sunfish-tftp-oack-resources")]
                {
                    let slot = (trace.transfer_port - crate::qcom_tftp_server_check::TRANSFER_PORT) as usize;
                    let resources = &mut self.server_check_resources[slot];
                    resources.initial = initial;
                    resources.oack_confirm = confirm;
                    resources.flow_pending = flow_pending;
                }
            }
        }
    }

    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
    pub(crate) fn observe_elf_server_check_coalesced(&mut self, handle: &TftpHandle) {
        for write in self.server_check_traces_mut().filter(|write| write.request == *handle) {
            write.coalesced = true;
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
        if let Some(repeat) = self.server_check_repeat_mut(handle) {
            repeat.coalesced = true;
        }
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
    fn server_check_repeat_mut(&mut self, handle: &TftpHandle) -> Option<&mut ServerCheckRepeat> {
        self.first_server_check.as_mut()?.repeat.as_mut()
            .filter(|repeat| repeat.request == Some(*handle))
    }

    #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
    pub(crate) fn observe_elf_server_check_dispatch_start(&mut self, handle: &TftpHandle) {
        let clock = self.intent_ack_clock;
        for write in self.server_check_traces_mut()
            .filter(|write| write.request == *handle && write.timing.dispatch_at.is_none())
        {
            write.timing.dispatch_at = clock.map(|clock| clock());
        }
        #[cfg(feature = "sunfish-ack-elf-phase-trace")]
        for write in self.server_check_traces_mut() {
            if let Some(data) = write.data.as_mut().filter(|d| d.request == *handle)
                && data.stages.dispatch_at.is_none()
            { data.stages.dispatch_at = clock.map(|clock| clock()); }
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
        if let Some(repeat) = self.server_check_repeat_mut(handle) {
            repeat.dispatch_at = clock.map(|clock| clock());
        }
    }

    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
    pub(crate) fn observe_elf_server_check_dispatch(
        &mut self,
        handle: &TftpHandle,
        route: TftpReplyRoute,
        server_check_wrq: bool,
        dispatch: Result<crate::qcom_tftp_server_check::Reply, crate::qcom_tftp_server_check::Error>,
        response: &[u8],
    ) {
        use crate::qcom_tftp_server_check::{OACK, SERVICE_PORT, is_transfer_port};
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        if server_check_wrq && route.source.port == SERVICE_PORT
            && let Ok(reply) = dispatch && response.get(..reply.len) == Some(OACK)
        {
            self.admit_server_check_trace(handle, reply.route.source.port);
        }
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        if self.completed_server_check.is_none()
            && dispatch.is_ok_and(|reply| reply.completed)
            && let Some(slot) = route.source.port.checked_sub(crate::qcom_tftp_server_check::TRANSFER_PORT)
            && let Some(Some(trace)) = self.server_check_transfers.get(slot as usize)
            && trace.peer == route.destination
        {
            self.completed_server_check = Some(slot as usize);
        }
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        let clock = self.intent_ack_clock;
        let result = dispatch.as_ref().err().map_or(0, |error| error.code());
        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
        if let Some(repeat) = self.server_check_repeat_mut(handle) {
            repeat.result = Some(result);
            if let Ok(reply) = dispatch && response.get(..reply.len) == Some(OACK) {
                repeat.oack.phase = 1;
            }
        }
        for write in self.server_check_traces_mut().filter(|write| write.peer == route.destination) {
            let exact_request = write.request == *handle;
            if exact_request {
                write.wrq_result = Some(result);
            }
            let first_data = write.data.as_mut().filter(|data| data.request == *handle);
            if let Some(data) = first_data {
                data.result = Some(result);
            }
            let Ok(reply) = dispatch else { continue };
            if (exact_request || write.coalesced) && server_check_wrq && route.source.port == SERVICE_PORT
                && is_transfer_port(reply.route.source.port)
                && response.get(..reply.len) == Some(OACK)
                && write.oack.phase == 0
            {
                write.transfer_port = Some(reply.route.source.port);
                write.oack.phase = 1;
                #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
                { write.timing.ready_at = clock.map(|clock| clock()); }
            }
            if reply.completed && write.transfer_port == Some(route.source.port)
                && let Some([0, 4, high, low]) = response.get(..reply.len)
                && write.final_block.is_none()
            {
                write.final_block = Some(u16::from_be_bytes([*high, *low]));
                write.ack.phase = 1;
                #[cfg(feature = "sunfish-ack-elf-phase-trace")]
                if let Some(data) = write.data.as_mut().filter(|d| d.request == *handle
                    && Some(d.block) == write.final_block)
                {
                    data.stages.final_prepared = true;
                    data.stages.prepared_at = clock.map(|clock| clock());
                }
            }
        }
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
    pub(crate) fn queue_traced_tftp_response(
        &mut self,
        generation: u64,
        handle: &TftpHandle,
        route: TftpReplyRoute,
        packet: &[u8],
        #[cfg(feature = "sunfish-wlan-first-server-check-retry")] original_consumed: bool,
    ) -> Result<(), Error> {
        #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
        let overlap = self.first_server_check.and_then(|write| {
            let repeat = write.repeat?;
            (original_consumed && repeat.eligible && repeat.request == Some(*handle)
                && repeat.close_phase.is_none() && repeat.oack.phase == 1
                && write.oack.phase == 3 && write.transfer_port == Some(route.source.port)
                && write.peer == route.destination && packet == crate::qcom_tftp_server_check::OACK)
                .then_some(write.oack.slot).flatten()
        });
        #[cfg(not(feature = "sunfish-wlan-first-server-check-retry"))]
        let overlap = None;
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
        let overlap=overlap.or_else(||self.efs_overlap_candidate(handle,route,packet));
/* EFS_REPLY_OVERLAP_END */
        #[cfg(feature = "sunfish-tftp-oack-reserve")]
        let reserve = self.reserved_oack.is_none()
            && packet == crate::qcom_tftp_server_check::OACK
            && self.first_server_check().is_some_and(|first| first.peer != route.destination)
            && self.ipa_registration_trace().is_some_and(|trace|
                trace.published_at.is_some() && trace.completed_at.is_none());
        let result = self.queue_tftp_transmit(generation, route, packet, overlap,
            #[cfg(feature = "sunfish-tftp-oack-reserve")]
            reserve,
        );
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        self.observe_post_mcfg_queue(handle,route,result);
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
        self.observe_startup_queue(handle,route,result);
/* STARTUP_PREREQ_END */
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
        self.bind_efs_overlap_queue(handle,route,packet,result,overlap);
/* EFS_REPLY_OVERLAP_END */
        #[cfg(feature = "sunfish-tftp-oack-reserve")]
        if result == Ok(TX_SLOTS) {
            self.reserved_oack = Some(ReservedOack {
                peer: route.destination,
                transfer_port: route.source.port,
                queued_at: self.intent_ack_clock.map(|clock| clock()),
                published_at: None,
                done_at: None,
                #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
                wire_bytes: [0; 72],
                #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
                wire_len: 0,
                #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
                wire_intent: u32::MAX,
                #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
                writer_trace: ReservedWriterTrace::default(),
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                early_status: 0,
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                early_command: 0,
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                early_guards: 0,
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                retry_state: 0,
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                retry_result: 0,
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                retry_ack_at: None,
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                retry_credit_at: None,
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                retry_attempt_at: None,
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                credit_pass_at: None,
                #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
                credit_counts: [0; 7],
                #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                credit_turn: 0,
                #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                retry_choice: 0,
                #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                retry_choice_id: u32::MAX,
                #[cfg(feature = "sunfish-tftp-oack-credit-early")]
                retry_gates: 0,
                #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
                after_done_state: 0,
                #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
                after_done_result: 0,
                #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
                after_done_at: None,
                #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
                after_done_retry_at: None,
                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                after_done_first_check: None,
                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                after_done_grant_at: None,
                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                after_done_pending_checks: 0,
                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                after_done_origin: 0,
                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                after_done_choice: 0,
                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                after_done_choice_id: u32::MAX,
                #[cfg(feature = "sunfish-tftp-after-done-trace")]
                after_done_gates: 0,
                #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
                after_done_ad: 0,
                #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
                after_done_ack: 0,
            });
        }
        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
        if let Ok(slot) = result && packet == crate::qcom_tftp_server_check::OACK {
            self.transmit[slot].traced_oack_request = Some(*handle);
        }
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        if let Ok(slot) = result && packet == crate::qcom_tftp_server_check::OACK
            && let Some(trace) = self.server_check_trace_for_intent(handle.intent)
            && trace.transfer_port == route.source.port && trace.peer == route.destination
        {
            self.transmit[slot].server_check_wrq = Some(handle.intent);
            #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
            if self.first_oack_after_ipa_publish && self.first_oack_eligibility.queued_at.is_none()
                && self.first_oack_eligibility.close_mask.is_none()
                && self.first_oack_transfer().is_some_and(|first| first.intent == handle.intent)
            {
                self.first_oack_eligibility.queued_at = self.intent_ack_clock.map(|clock| clock());
            }
        }
        let clock = self.intent_ack_clock;
        #[cfg(feature = "sunfish-ack-elf-phase-trace")]
        if let Ok(slot) = result && route.source.node == self.config.local_node
            && let [0, 4, high, low] = packet
        {
            let block = u16::from_be_bytes([*high, *low]);
            for write in self.server_check_traces_mut().filter(|w| w.peer == route.destination
                && w.transfer_port == Some(route.source.port) && w.final_block == Some(block)
                && w.ack.phase == 2 && w.ack.slot == Some(slot))
            {
                if let Some(data) = write.data.as_mut().filter(|d| d.request == *handle
                    && d.block == block && d.result == Some(0) && d.stages.final_prepared
                    && !d.stages.final_queued)
                {
                    data.stages.final_queued = true;
                    data.stages.queued_at = clock.map(|clock| clock());
                }
            }
        }
        if let Some(repeat) = self.server_check_repeat_mut(handle).filter(|repeat| repeat.oack.phase == 1) {
            match result {
                Ok(slot) => {
                    repeat.oack.slot = Some(slot);
                    repeat.oack.phase = 2;
                    repeat.queued_at = clock.map(|clock| clock());
                    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
                    { repeat.overlapped = overlap.is_some(); }
                }
                Err(Error::FlowBusy) => repeat.flow_busy = repeat.flow_busy.saturating_add(1),
                Err(Error::NoCapacity) => repeat.no_capacity = repeat.no_capacity.saturating_add(1),
                Err(_) => {}
            }
        }
        result.map(|_| ())
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    pub(crate) fn cancel_closed_tftp_retry(&mut self, handle: &TftpHandle) -> bool {
        let Some(repeat) = self.server_check_repeat_mut(handle)
            .filter(|repeat| repeat.eligible && repeat.close_phase.is_some() && repeat.oack.phase == 1)
        else { return false; };
        repeat.cancelled = true;
        true
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
    pub(crate) fn observe_retry_turn(&mut self) {
        if self.retry_oack_slot().is_some()
            && let Some(repeat) = self.first_server_check.as_mut().and_then(|write| write.repeat.as_mut())
            && repeat.tx.turn_at.is_none()
        {
            repeat.tx.turn_at = self.intent_ack_clock.map(|clock| clock());
        }
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
    pub(crate) fn observe_retry_poll_end(&mut self) {
        if let Some(repeat) = self.first_server_check.as_mut().and_then(|write| write.repeat.as_mut())
            && repeat.tx.turn_at.is_some() && repeat.tx.first_poll_end.is_none()
        {
            repeat.tx.first_poll_end = self.intent_ack_clock.map(|clock| clock());
        }
    }

    /// Read-only diagnostic: the exact reply associated with this request has
    /// been fully published to the GLINK transport. This is not peer receipt.
    #[cfg(feature = "sunfish-linux-rmtfs")]
    /// Pure live ownership metadata. Normal publication still owns all gates.
    #[cfg(feature="sunfish-elf-queued-handoff")]
    pub(crate) fn unpublished_registry_reply(&self, handle:&RequestHandle)->bool {
        if handle.owner!=self.owner || handle.generation!=self.generation {return false;}
        let Some(index)=self.receive.iter().position(|rx|rx.id==handle.intent && rx.state==RxState::ReplyQueued)else{return false;};
        self.transmit.iter().any(|tx|tx.len!=0 && tx.sent<tx.len && !tx.glink_done
            && matches!(tx.kind,TxKind::Reply{receive,route,..} if receive==index && route.service==Service::ServiceRegistry))
    }
    pub fn reply_fully_published(&self, handle: &RequestHandle) -> bool {
        if handle.owner != self.owner || handle.generation != self.generation {
            return false;
        }
        let Some(index) = self.receive.iter().position(|rx|
            rx.id == handle.intent && rx.state == RxState::ReplyQueued) else { return false };
        self.transmit.iter().any(|tx| tx.len != 0 && tx.sent == tx.len
            && matches!(tx.kind, TxKind::Reply { receive, .. } if receive == index))
    }
    pub fn configuration(&self) -> Config {
        self.config
    }
    /// Only the selected native negotiator may bind a prepared, unpublished
    /// session to the IDs learned during this actual channel opening.
    #[allow(dead_code)] // The standalone QMI harness has no GLINK negotiator.
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    pub(crate) fn bind_native_channel(
        &mut self,
        g: u64,
        local: u16,
        remote: u16,
    ) -> Result<(), Error> {
        self.check(g)?;
        if self.control.started() || !self.transport_is_pristine() || local == 0 || remote == 0 {
            return Err(Error::InvalidChannel);
        }
        self.config.local_channel = local;
        self.config.remote_channel = remote;
        Ok(())
    }
    pub fn last_completion_error(&self) -> Option<CompletionDiagnostic> {
        self.last_completion_error
    }
    pub fn last_credit_diagnostic(&self) -> Option<CreditDiagnostic> {
        self.last_credit_diagnostic
    }
    pub fn client_gone_diagnostic(&self) -> Option<ClientGoneDiagnostic> {
        self.client_gone_diagnostic
    }
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    pub fn deferred_close_diagnostic(&self) -> Option<DeferredCloseDiagnostic> {
        self.deferred_close_diagnostic
    }
    /// Rejection never consumes either held receive or publishes RX_DONE.
    /// Recovery requires the ordinary trusted reset of this session.
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pub(crate) fn reject_tftp_close(&mut self, generation: u64, peer: Address) -> Result<(), Error> {
        if generation != self.generation { return Err(Error::StaleGeneration); }
        if !self.pending_tftp_close.is_some_and(|(held, _, _)| held == peer) { return Err(Error::InvalidHandle); }
        self.quarantined = true;
        Ok(())
    }
    #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
    pub(crate) fn test_fail_close_stage(&mut self, stage: u8) { self.test_fail_close_stage = stage; }
    /// Host-only prototype. The caller must first validate the matching owner
    /// reply and empty server transfer while holding exclusive access. Any
    /// failed commit quarantines this session; it cannot be retried.
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pub(crate) fn commit_tftp_close(&mut self, generation: u64, peer: Address) -> Result<control::Event, Error> {
        if generation != self.generation { return Err(Error::StaleGeneration); }
        if self.quarantined { return Err(Error::Quarantined); }
        let Some((held, deferred, closing)) = self.pending_tftp_close else { return Err(Error::InvalidHandle); };
        if held != peer { return Err(Error::InvalidHandle); }
        let result = (|| {
            let deferred_rx = self.receive.get(deferred).ok_or(Error::InvalidHandle)?;
            let close_rx = self.receive.get(closing).ok_or(Error::InvalidHandle)?;
            let request = crate::qcom_tftp_server_check::REQUEST;
            if deferred_rx.state != RxState::Deferred || deferred_rx.used < 32 + request.len()
                || word(&deferred_rx.bytes, 8) != peer.node || word(&deferred_rx.bytes, 12) != peer.port
                || word(&deferred_rx.bytes, 28) != crate::qcom_tftp_server_check::SERVICE_PORT
                || word(&deferred_rx.bytes, 20) as usize != request.len()
                || &deferred_rx.bytes[32..32 + request.len()] != request
                || close_rx.used < 32 || close_rx.state != RxState::Held {
                return Err(Error::InvalidHandle);
            }
            let len = 32usize.checked_add(word(&close_rx.bytes, 20) as usize).ok_or(Error::InvalidHandle)?;
            if len > close_rx.used || self.control.decode(&close_rx.bytes[..len])? != control::Command::DelClient(peer.port) {
                return Err(Error::InvalidHandle);
            }
            let tombstone = self.retired_rmtfs_peers.iter().position(Option::is_none).ok_or(Error::NoCapacity)?;
            let key = crate::qcom_qrtr_flow::Destination { node: peer.node, port: peer.port };
            if self.rmtfs_reference_flows.contains(key) {
                self.rmtfs_reference_flows.close(key).map_err(|_| Error::FlowBusy)?;
                #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
                if self.test_fail_close_stage == 1 { return Err(Error::InvalidHandle); }
            }
            let event = self.control.receive(&self.receive[closing].bytes[..len])?;
            if event != control::Event::ClientRemoved(peer.port) { return Err(Error::InvalidHandle); }
            self.observe_client_control(event)?;
            #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
            if self.test_fail_close_stage == 2 { return Err(Error::InvalidHandle); }
            self.retired_rmtfs_peers[tombstone] = Some(peer);
            self.receive[deferred].state = RxState::Control;
            self.receive[deferred].done_pending = true;
            self.receive[closing].state = RxState::Control;
            self.receive[closing].done_pending = true;
            self.pending_tftp_close = None;
            self.deferred_close_diagnostic = None;
            Ok(event)
        })();
        if result.is_err() { self.quarantined = true; }
        result
    }
    #[cfg(feature = "sunfish-tftp-deferred-close-experiment")]
    pub fn set_tftp_owner_busy(&mut self, generation: u64, busy: bool) -> Result<(), Error> {
        self.check(generation)?;
        self.tftp_owner_busy = busy;
        Ok(())
    }
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    pub fn set_tftp_owner_peers(&mut self, generation: u64, peers: [Option<Address>; TFTP_OWNER_PEER_SLOTS]) -> Result<(), Error> {
        self.check(generation)?;
        self.tftp_owner_peers = peers;
        self.tftp_owner_snapshot_valid = true;
        Ok(())
    }
    #[cfg(all(test, feature = "sunfish-tftp-peer-metadata-batch"))]
    pub(crate) fn test_peer_metadata(&self) -> ([Option<Address>; TFTP_OWNER_PEER_SLOTS], [Option<Address>; TFTP_OWNER_PEER_SLOTS]) {
        (self.tftp_owner_peers, self.tftp_firmware_peers)
    }
    #[cfg(feature = "sunfish-tftp-close-owner-fence")]
    pub fn set_tftp_firmware_peers(&mut self, generation: u64, peers: [Option<Address>; TFTP_OWNER_PEER_SLOTS]) -> Result<(), Error> {
        self.check(generation)?;
        self.tftp_firmware_peers = peers;
        Ok(())
    }
    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
    pub(crate) fn commit_firmware_close(&mut self, generation: u64, peer: Address) -> Result<(), Error> {
        if generation != self.generation { return Err(Error::StaleGeneration); }
        if self.quarantined { return Err(Error::Quarantined); }
        let Some((held, index)) = self.pending_firmware_close else { return Err(Error::InvalidHandle); };
        if held != peer { return Err(Error::InvalidHandle); }
        let result = (|| {
            let rx = self.receive.get(index).ok_or(Error::InvalidHandle)?;
            if rx.state != RxState::Held || rx.used < 52 { return Err(Error::InvalidHandle); }
            let len = 32usize.checked_add(word(&rx.bytes, 20) as usize).ok_or(Error::InvalidHandle)?;
            if len > rx.used || self.control.decode(&rx.bytes[..len])? != control::Command::DelClient(peer.port) {
                return Err(Error::InvalidHandle);
            }
            let mut published = false;
            for tx in self.transmit.iter().filter(|tx| tx.len != 0 && tx.kind.qrtr_addresses().is_some_and(|(_, to)| to == peer)) {
                if !matches!(tx.kind, TxKind::TftpReply { source, .. } if (0x5100..=0x51ff).contains(&source.port))
                    || tx.glink_done || !((tx.sent == 0 && tx.remote_slot.is_none()) || tx.sent == tx.len) {
                    return Err(Error::FlowBusy);
                }
                published |= tx.sent == tx.len;
            }
            if self.retired_rmtfs_peers.contains(&Some(peer)) { return Err(Error::FlowBusy); }
            let tombstone = self.retired_rmtfs_peers.iter().position(Option::is_none).ok_or(Error::NoCapacity)?;
            let key = crate::qcom_qrtr_flow::Destination { node: peer.node, port: peer.port };
            for index in 0..self.transmit.len() {
                let tx = &self.transmit[index];
                if tx.len == 0 || tx.sent != 0 || !matches!(tx.kind, TxKind::TftpReply { destination, .. } if destination == peer) { continue; }
                let (source, destination) = tx.kind.qrtr_addresses().ok_or(Error::InvalidHandle)?;
                let token = tx.reference_flow.as_ref().ok_or(Error::InvalidHandle)?;
                self.rmtfs_reference_flows.fail_unpublished(token).map_err(|_| Error::FlowBusy)?;
                self.transmit[index] = Transmit::empty();
                #[cfg(feature="sunfish-elf-tx-slots")]
                self.observe_elf_tx_freed(index,3);
                self.forget_qrtr_flow_if_idle(source, destination);
            }
            if !published && self.rmtfs_reference_flows.contains(key) { self.rmtfs_reference_flows.close(key).map_err(|_| Error::FlowBusy)?; }
            let event = self.control.receive(&self.receive[index].bytes[..len])?;
            if event != control::Event::ClientRemoved(peer.port) { return Err(Error::InvalidHandle); }
            self.observe_client_control(event)?;
            self.retired_rmtfs_peers[tombstone] = Some(peer);
            self.receive[index].state = RxState::Control;
            self.receive[index].done_pending = true;
            self.pending_firmware_close = None;
            Ok(())
        })();
        if result.is_err() { self.quarantined = true; }
        result
    }
    /// Closed RMTFS peer ports fenced since the last trusted transport reset.
    pub fn retired_rmtfs_peer_count(&self) -> usize {
        self.retired_rmtfs_peers.iter().flatten().count()
    }
    pub fn endpoint(&self, service: Service) -> Option<routing::Endpoint> {
        self.router.endpoint(service)
    }

    /// Queue one raw TFTP reply on its negotiated QRTR transfer endpoint.
    ///
    /// The TFTP server owns request policy and transfer state; this method
    /// validates the direct-peer route, wraps the supplied TFTP packet in a
    /// QRTR DATA datagram and the bounded GLINK intent path. The experimental
    /// server-check endpoint uses per-destination QRTR credit accounting and
    /// releases its transmit only after GLINK completion; other endpoints use
    /// the legacy per-reply QRTR completion rule. No QMI transaction is reserved.
    pub fn queue_tftp_response(
        &mut self,
        generation: u64,
        route: TftpReplyRoute,
        tftp_packet: &[u8],
    ) -> Result<(), Error> {
        self.queue_tftp_transmit(generation, route, tftp_packet, None,
            #[cfg(feature = "sunfish-tftp-oack-reserve")]
            false,
        ).map(|_| ())
    }

    #[cfg(feature="sunfish-elf-capacity-retry")]
    pub(crate) fn normal_tx_free(&self)->Option<usize>{self.free_tx_slot()}

    fn free_tx_slot(&self) -> Option<usize> {
        self.transmit[..TX_SLOTS].iter().position(|tx| tx.len == 0)
    }

    fn queue_tftp_transmit(
        &mut self,
        generation: u64,
        route: TftpReplyRoute,
        tftp_packet: &[u8],
        overlap: Option<usize>,
        #[cfg(feature = "sunfish-tftp-oack-reserve")] reserve: bool,
    ) -> Result<usize, Error> {
        #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
        { self.tftp_queue_block = TftpQueueBlock::default(); }
        self.check(generation)?;
        if !self.control.accepts_tftp() {
            return Err(control::Error::Unavailable.into());
        }
        if route.source.node != self.config.local_node
            || route.destination.node != self.config.peer_node
            || route.source.port == 0
            || route.source.port >= control::CONTROL_PORT
            || route.destination.port == 0
            || route.destination.port >= control::CONTROL_PORT
        {
            return Err(control::Error::Address.into());
        }
        let conflicts = self
            .router
            .endpoint(Service::RemoteStorage)
            .is_some_and(|endpoint| endpoint.address.port == route.source.port)
            || self
                .router
                .endpoint(Service::HeapMemory)
                .is_some_and(|endpoint| endpoint.address.port == route.source.port)
            || self
                .router
                .endpoint(Service::ServiceRegistry)
                .is_some_and(|endpoint| endpoint.address.port == route.source.port)
            || self
                .router
                .endpoint(Service::IpaServer)
                .is_some_and(|endpoint| endpoint.address.port == route.source.port)
            || self
                .wlfw_client
                .as_ref()
                .is_some_and(|client| client.local_address().port == route.source.port)
            || self
                .servreg_notif_client
                .as_ref()
                .is_some_and(|client| client.local_address().port == route.source.port)
            || self.control.tftp_port() == Some(route.source.port);
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa_port_in_use(route.source.port) {
            return Err(control::Error::Address.into());
        }
        if conflicts {
            return Err(control::Error::Address.into());
        }
        if tftp_packet.len() < 4 {
            return Err(Error::InvalidReply);
        }
        if tftp_packet.len() > MAX_DATAGRAM_BYTES - routing::QRTR_HEADER_BYTES {
            return Err(qmi::Error::TooLarge.into());
        }
        let source = route.source;
        let destination = route.destination;
        if let Some((_, tx)) = self.transmit.iter().enumerate().find(|(index, tx)| {
            tx.len != 0
                && tx
                    .kind
                    .qrtr_addresses()
                    .is_some_and(|(_, peer_destination)| peer_destination == destination)
                && !(overlap == Some(*index) && tx.sent == tx.len && !tx.glink_done
                    && matches!(tx.kind, TxKind::TftpReply { source: from, .. } if from == source)
                    && word(&tx.bytes, 20) as usize == tftp_packet.len()
                    && tx.bytes.get(routing::QRTR_HEADER_BYTES..routing::QRTR_HEADER_BYTES + tftp_packet.len())
                        == Some(tftp_packet))
        }) {
            #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
            {
                self.tftp_queue_block = TftpQueueBlock {
                    check: 1,
                    kind: tx.kind.diagnostic_kind(),
                    opcode: if matches!(tx.kind, TxKind::TftpReply { .. }) {
                        tx.bytes[routing::QRTR_HEADER_BYTES + 1]
                    } else { 0 },
                    flags: u8::from(tx.sent != 0) | (u8::from(tx.sent == tx.len) << 1)
                        | (u8::from(tx.glink_done) << 2) | (u8::from(tx.qrtr_done) << 3),
                };
            }
            let _ = tx;
            return Err(Error::FlowBusy);
        }
        let slot = self.free_tx_slot();
        #[cfg(feature = "sunfish-tftp-oack-reserve")]
        let slot = slot.or_else(|| (reserve && self.transmit[TX_SLOTS].len == 0).then_some(TX_SLOTS));
        let Some(index) = slot else {
            #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
            { self.tftp_queue_block.check = 2; }
            return Err(Error::NoCapacity);
        };
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_tftp = {
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            { crate::qcom_tftp_server_check::is_transfer_port(route.source.port)
                || (cfg!(feature = "sunfish-tftp-wlan-mux") && (0x5100..=0x51ff).contains(&route.source.port)) }
            #[cfg(not(feature = "sunfish-native-tftp-server-check"))]
            { false }
        };
        if let Err(error) = self.remember_qrtr_flow_with_limit(source, destination, TX_SLOTS.max(index + 1)) {
            #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
            { self.tftp_queue_block.check = if error == Error::FlowBusy { 4 } else { 3 }; }
            return Err(error);
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_flow = match self.reserve_remembered_reference(source, destination, !reference_tftp) {
            Ok(token) => token,
            Err(error) => {
                #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
                { self.tftp_queue_block.check = match error {
                    Error::FlowBusy => 5,
                    Error::NoCapacity => 6,
                    _ => 0,
                }; }
                return Err(error);
            }
        };
        let tx = &mut self.transmit[index];
        let len = routing::QRTR_HEADER_BYTES + tftp_packet.len();
        header(
            &mut tx.bytes,
            1,
            source,
            destination,
            tftp_packet.len(),
            {
                #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
                { reference_flow.confirm_rx() }
                #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
                { true }
            },
        );
        tx.bytes[routing::QRTR_HEADER_BYTES..len].copy_from_slice(tftp_packet);
        tx.bytes[len..align(len, 4)].fill(0);
        tx.kind = TxKind::TftpReply {
            source,
            destination,
        };
        tx.remote_slot = None;
        tx.len = align(len, 4);
        tx.sent = 0;
        tx.glink_done = false;
        tx.qrtr_done = false;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { tx.qrtr_done = reference_tftp; }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { tx.reference_flow = Some(reference_flow); }
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        if let Some(trace) = self.missing_tftp_reply.as_mut() {
            if trace.flags == 1 && trace.source == source && trace.destination == destination
                && tftp_packet == crate::qcom_tftp_server_check::MISSING_REPLY {
                trace.flags |= 2;
                tx.missing_reply_observed = true;
            }
        }
        #[cfg(feature = "sunfish-tftp-final-ack-observer")]
        if let Some(trace) = self.final_tftp_ack.as_mut() {
            if trace.flags == 1 && trace.source == source && trace.destination == destination
                && tftp_packet.len() == 4 && tftp_packet[..2] == [0, 4]
                && u16::from_be_bytes([tftp_packet[2], tftp_packet[3]]) == trace.block {
                trace.flags |= 2;
                tx.final_ack_observed = true;
            }
        }
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        if let Some(trace) = self.mcfg_oack.as_mut() {
            if trace.flags == 1 && trace.source == source && trace.destination == destination
                && tftp_packet == crate::qcom_tftp_server_check::MCFG_WRITE_OACK
            {
                trace.flags |= 2;
                self.mcfg_oack_slot = Some(index);
            }
        }
        #[cfg(feature = "sunfish-tftp-endpoint-observer")]
        if self.first_tftp_endpoint.is_none() && tftp_packet.starts_with(&[0, 6]) {
            self.first_tftp_endpoint = Some(TftpEndpointTrace {
                source, destination, payload_bytes: tftp_packet.len() as u32,
                flags: 1 | (word(&tx.bytes, 16) << 4), remote_intent: u32::MAX,
            });
            self.first_tftp_slot = Some(index);
        }
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        let clock = self.intent_ack_clock;
        #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
        for write in self.server_check_traces_mut().filter(|write| write.peer == destination)
        {
            use crate::qcom_tftp_server_check::{OACK, is_transfer_port};
            if (write.oack.phase == 1 || (write.oack.phase == 0 && write.coalesced))
                && tftp_packet == OACK && is_transfer_port(source.port)
                && write.transfer_port.is_none_or(|port| port == source.port)
            {
                write.transfer_port = Some(source.port);
                write.oack.phase = 2;
                write.oack.slot = Some(index);
                #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
                { write.timing.queued_at = clock.map(|clock| clock()); }
            }
            if write.ack.phase == 1 && write.transfer_port == Some(source.port)
                && let [0, 4, high, low] = tftp_packet
                && write.final_block == Some(u16::from_be_bytes([*high, *low]))
            {
                write.ack.phase = 2;
                write.ack.slot = Some(index);
            }
        }
        Ok(index)
    }

    /// Queue the AP IPA service's prepared master-init indication to the
    /// unique modem IPA endpoint. Completion is reported only after both the
    /// GLINK receive-done and QRTR resume-tx acknowledgements arrive.
    pub fn queue_ipa_master_init_indication(
        &mut self,
        generation: u64,
        indication: &PreparedIndication,
    ) -> Result<(), Error> {
        self.check(generation)?;
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa.is_some() {
            return Err(Error::InvalidReply);
        }
        if indication.generation() != generation {
            return Err(Error::StaleGeneration);
        }
        if !self.control.accepts(Service::IpaServer) {
            return Err(control::Error::Unavailable.into());
        }
        if self.completed_ipa_indication.is_some()
            || self
                .transmit
                .iter()
                .any(|tx| tx.len != 0 && matches!(tx.kind, TxKind::IpaIndication { .. }))
        {
            return Err(Error::FlowBusy);
        }
        let endpoint = self
            .router
            .endpoint(Service::IpaServer)
            .ok_or(routing::Error::NoEndpoint)?;
        let source = endpoint.address;
        let destination = self
            .control
            .ipa_modem_endpoint()
            .ok_or(control::Error::Unavailable)?;
        if indication.peer() != destination
            || source.node != self.config.local_node
            || destination.node != self.config.peer_node
            || source.port == 0
            || source.port >= control::CONTROL_PORT
            || destination.port == 0
            || destination.port >= control::CONTROL_PORT
        {
            return Err(control::Error::Address.into());
        }

        let qmi_bytes = indication.bytes();
        let message = Message::decode(qmi_bytes)?;
        if qmi_bytes.len() != ipa_service::QMI_MESSAGE_BYTES
            || message.kind != qmi::Kind::Indication
            || message.transaction != 0
            || message.message_id != ipa_service::MASTER_DRIVER_INIT_COMPLETE_MESSAGE_ID
            || message.tlv_count() != 1
            || message.required(ipa_service::QMI_COMMON_RESPONSE_TLV)? != [0, 0, 0, 0]
        {
            return Err(Error::InvalidReply);
        }
        if self.transmit.iter().any(|tx| {
            tx.len != 0
                && tx
                    .kind
                    .qrtr_addresses()
                    .is_some_and(|(_, peer_destination)| peer_destination == destination)
        }) {
            return Err(Error::FlowBusy);
        }
        let index = self.free_tx_slot()
            .ok_or(Error::NoCapacity)?;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_flow = self.remember_and_reserve_reference(source, destination, true)?;
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        self.remember_qrtr_flow(source, destination)?;
        let tx = &mut self.transmit[index];
        let datagram_len = routing::QRTR_HEADER_BYTES + qmi_bytes.len();
        header(&mut tx.bytes, 1, source, destination, qmi_bytes.len(), true);
        tx.bytes[routing::QRTR_HEADER_BYTES..datagram_len].copy_from_slice(qmi_bytes);
        tx.bytes[datagram_len..align(datagram_len, 4)].fill(0);
        tx.kind = TxKind::IpaIndication {
            source,
            destination,
            token: indication.token(),
        };
        tx.remote_slot = None;
        tx.len = align(datagram_len, 4);
        tx.sent = 0;
        tx.glink_done = false;
        tx.qrtr_done = false;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { tx.reference_flow = Some(reference_flow); }
        Ok(())
    }

    /// Consume the token for a master-init indication fully acknowledged by
    /// both transports. The caller then commits it to the IPA service state.
    pub fn take_ipa_indication_completion(
        &mut self,
        generation: u64,
    ) -> Result<Option<u64>, Error> {
        self.check(generation)?;
        Ok(self.completed_ipa_indication.take())
    }

    /// Consume the token for an AP IPA QMI response fully acknowledged by
    /// both transports. The caller then commits it to the IPA service state.
    pub fn take_ipa_service_response_completion(
        &mut self,
        generation: u64,
    ) -> Result<Option<u64>, Error> {
        self.check(generation)?;
        Ok(self.completed_ipa_service_response.take())
    }

    /// The FIFO writer must yield to visible RX before ACKing existing backing.
    #[cfg(feature = "sunfish-glink-announcement-reuse")]
    pub(crate) fn reusable_intent_ack_pending(&self) -> bool {
        self.intent_request_ack == Some(true) && self.intent_ack_reused.is_some()
    }

    /// Queue a one-shot receive intent backed by a private 4096-byte slot.
    /// The peer cannot use it until publish_next successfully advertises it.
    pub fn offer_receive(&mut self, generation: u64) -> Result<u32, Error> {
        self.check(generation)?;
        let slot = self
            .receive[..RX_SLOTS]
            .iter()
            .position(|rx| rx.state == RxState::Free)
            .ok_or(Error::NoCapacity)?;
        let id = self.next_local_intent;
        let next = id.checked_add(1).ok_or(Error::IdExhausted)?;
        self.receive[slot].state = RxState::Advertising;
        self.receive[slot].id = id;
        self.next_local_intent = next;
        Ok(id)
    }

    pub fn ingest(&mut self, generation: u64, record: &[u8]) -> Result<Event, Error> {
        #[cfg(feature="sunfish-elf-first-slot-turn")]
        {self.elf_slot_receipt=None;}
        self.check(generation)?;
        if record.len() < 8 {
            return Err(Error::TruncatedRecord);
        }
        let command = u16::from_le_bytes([record[0], record[1]]);
        let channel = u16::from_le_bytes([record[2], record[3]]);
        if channel != self.config.remote_channel {
            return Err(Error::WrongChannel);
        }
        let parameter = word(record, 4);
        match command {
            5 => self.receive_intents(parameter, record),
            7 => {
                if record.len() != 8 {
                    return Err(Error::RecordLength);
                }
                if self.intent_request_ack.is_some() {
                    return Err(Error::NoCapacity);
                }
                #[cfg(feature = "sunfish-glink-announcement-reuse")]
                let reusable = if parameter != 0 && parameter as usize <= MAX_DATAGRAM_BYTES {
                    self.receive[..RX_SLOTS].iter().find(|rx|
                        rx.state == RxState::Available && rx.announcement_reuse).map(|rx|rx.id)
                } else { None };
                #[cfg(not(feature = "sunfish-glink-announcement-reuse"))]
                let reusable: Option<u32> = None;
                let granted = parameter != 0
                    && parameter as usize <= MAX_DATAGRAM_BYTES
                    && (reusable.is_some() || self.receive[..RX_SLOTS].iter().any(|rx| rx.state == RxState::Free));
                #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
                if !granted && self.ipa_registration_trace().is_some_and(|trace|
                    trace.published_at.is_some() && trace.completed_at.is_none())
                {
                    self.pending_denial_slots = Some((parameter, self.intent_denial_slots()));
                }
                if !granted && self.intent_denial.is_none() {
                    let mut states = 0u32;
                    for (i, rx) in self.receive[..RX_SLOTS].iter().enumerate() {
                        states |= rx.state.diagnostic_code() << (4 * i);
                    }
                    self.intent_denial = Some(IntentDenialDiagnostic { requested: parameter, states });
                }
                #[cfg(feature = "sunfish-first-oack-spare-buffer")]
                if parameter == 108 && self.first_server_check.is_some() {
                    self.first_oack_108_seen = true;
                }
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                { self.grant_backing = GrantBackingTrace { requested: parameter, ..Default::default() }; }
                #[cfg(feature = "sunfish-glink-announcement-reuse")]
                { self.intent_ack_reused = reusable; }
                if granted && reusable.is_none() {
                    let _id = self.offer_receive(generation)?;
                    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
                    { self.intent_ack_backing = Some(_id); }
                    #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                    {
                        self.grant_backing.intent = Some(_id);
                        self.grant_backing.reserved_at = self.intent_ack_clock.map(|now| now());
                    }
                }
                #[cfg(feature = "sunfish-glink-retry-full-intent")]
                { self.retry_full_intent = !granted && parameter != 0
                    && parameter as usize <= MAX_DATAGRAM_BYTES; }
                self.intent_request_ack = Some(granted);
                #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
                { self.intent_ack_started = self.intent_ack_clock.map(|clock| clock()); }
                Ok(Event::ReceiveIntentRequest { granted })
            }
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            8 => {
                if record.len() != 8 { return Err(Error::RecordLength); }
                let OutboundIntentRequest::Waiting { size, granted: false } = self.outbound_intent_request else {
                    return Err(Error::InvalidIntent);
                };
                if parameter > 1 { return Err(Error::InvalidIntent); }
                if parameter == 0 {
                    self.outbound_intent_request = OutboundIntentRequest::Done { size, granted: false };
                } else {
                    self.outbound_intent_request = OutboundIntentRequest::Waiting { size, granted: true };
                }
                Ok(Event::RemoteIntentRequestAck { granted: parameter == 1 })
            }
            6 | 14 => {
                if record.len() != 8 {
                    return Err(Error::RecordLength);
                }
                self.receive_done(parameter, command == 14)
            }
            9 | 12 => self.receive_data(command, parameter, record),
            _ => Err(Error::UnsupportedCommand),
        }
    }

    fn receive_intents(&mut self, count: u32, record: &[u8]) -> Result<Event, Error> {
        let count = count as usize;
        if count == 0 || count > REMOTE_INTENTS {
            return Err(Error::IntentCapacity);
        }
        if record.len() != 8 + count * 8 {
            return Err(Error::RecordLength);
        }
        if self.remote.iter().filter(|entry| entry.is_none()).count() < count {
            return Err(Error::NoCapacity);
        }
        // GLINK intent IDs are opaque per-channel IDs. The pinned Linux peer
        // allocates them cyclically, so an ID may reappear after RX_DONE has
        // retired its previous non-reusable intent. Validate the whole batch
        // before installing any entries and reject only collisions with live
        // intents or duplicates within this record.
        for index in 0..count {
            let capacity = word(record, 8 + index * 8);
            let id = word(record, 12 + index * 8);
            if capacity == 0 {
                return Err(Error::IntentCapacity);
            }
            if self.remote.iter().flatten().any(|intent| intent.id == id)
                || (0..index).any(|earlier| word(record, 12 + earlier * 8) == id)
            {
                return Err(Error::IntentAlreadyActive);
            }
        }
        for index in 0..count {
            let slot = self.remote.iter().position(Option::is_none).unwrap();
            self.remote[slot] = Some(RemoteIntent {
                capacity: word(record, 8 + index * 8),
                id: word(record, 12 + index * 8),
                busy: false,
            });
        }
        Ok(Event::RemoteIntents)
    }

    fn receive_data(&mut self, command: u16, intent: u32, record: &[u8]) -> Result<Event, Error> {
        if record.len() < 16 {
            return Err(Error::TruncatedRecord);
        }
        let chunk = word(record, 8) as usize;
        let remaining = word(record, 12) as usize;
        if chunk == 0
            || chunk > MAX_DATAGRAM_BYTES
            || remaining > MAX_DATAGRAM_BYTES
            || chunk + remaining > MAX_DATAGRAM_BYTES
        {
            return Err(Error::FragmentLength);
        }
        if record.len() != align(16 + chunk, 8) {
            return Err(Error::RecordLength);
        }
        let index = self
            .receive[..RX_SLOTS]
            .iter()
            .position(|rx| rx.id == intent && rx.state != RxState::Free)
            .ok_or(Error::InvalidIntent)?;
        let rx = &mut self.receive[index];
        match (command, rx.state) {
            (9, RxState::Available) => {
                // Available reuse never follows this buffer into a new packet
                // unless that packet is itself a completely accepted announcement.
                #[cfg(feature = "sunfish-glink-announcement-reuse")]
                {
                    // A previously returned buffer may be consumed while its
                    // grant ACK is blocked. Never promise that consumed backing.
                    if self.intent_ack_reused == Some(rx.id) && self.intent_request_ack == Some(true) {
                        self.intent_ack_reused = None;
                        self.intent_request_ack = Some(false);
                        self.retry_full_intent = true;
                    }
                    rx.announcement_reuse = false;
                }
                #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
                { rx.elf_wrq_observed = false; }
            }
            (12, RxState::Assembling) if rx.remaining == chunk + remaining => {}
            _ => return Err(Error::FragmentOrder),
        }
        if rx.fragments >= MAX_FRAGMENTS {
            return Err(Error::FragmentLimit);
        }
        if rx.used + chunk + remaining > MAX_DATAGRAM_BYTES {
            return Err(Error::FragmentLength);
        }
        rx.bytes[rx.used..rx.used + chunk].copy_from_slice(&record[16..16 + chunk]);
        rx.used += chunk;
        rx.remaining = remaining;
        rx.fragments += 1;
        rx.state = RxState::Assembling;
        #[cfg(feature = "sunfish-tftp-oack-resources")]
        if command == 9 {
            self.observe_oack_intent_consumed(intent);
        }
        #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
        self.follow_oack_slot_packet(index, command == 9, remaining == 0);
        if remaining != 0 {
            return Ok(Event::Fragment);
        }
        // A rejected complete message is held, with no RX_DONE or automatic
        // error reply. Its receive slot is recovered only on trusted reset.
        self.receive[index].state = RxState::Held;
        #[cfg(feature = "sunfish-wlan-first-wrq-window")]
        if let Some(after) = self.wrq_window_phase() {
            let rx = &self.receive[index];
            if let Some(neighbor) = FirstWrqWindow::neighbor(
                &rx.bytes[..rx.used], 44, self.config.peer_node, self.config.local_node,
                self.intent_ack_clock, rx.id, None,
            ) {
                self.first_wrq_window.receive(after, neighbor);
            }
        }
        #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
        self.observe_elf_oack_peer(index);
        #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
        let rx = &self.receive[index];
        #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
        if let Some(trace) = self.elf_exchange.as_mut()
            && trace.completed
            && rx.used >= routing::QRTR_HEADER_BYTES
        {
            let slot = trace.following.iter_mut().find(|slot| slot.is_none());
            #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
            let first_data = trace.credit.first_data.is_none() && word(&rx.bytes, 4) == 1;
            #[cfg(not(feature = "sunfish-wlan-elf-credit-trace"))]
            let first_data = false;
            if slot.is_some() || first_data {
                let version = word(&rx.bytes, 0).min(255);
                let kind = word(&rx.bytes, 4).min(255);
                let header = ElfQrtrHeader {
                    meta: (version << 24) | (kind << 16) | rx.used as u32,
                    source_port: word(&rx.bytes, 12),
                    destination_port: word(&rx.bytes, 28),
                };
                if let Some(slot) = slot {
                    *slot = Some(header);
                }
                #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
                if first_data {
                    trace.credit.first_data = Some(header);
                }
            }
        }
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        self.observe_post_mcfg_wire(index);
        self.finish_receive(index)
    }

    /// Sample one reassembled QRTR datagram before any destination routing.
    /// Deferred routing never re-enters here. Retain only metadata from the
    /// tracked peer between OACK publication and its first observed close.
    #[cfg(feature = "sunfish-wlan-elf-oack-peer-trace")]
    fn observe_elf_oack_peer(&mut self, index: usize) {
        use crate::qcom_tftp_server_check::REQUEST_PREFIX;

        let rx = &self.receive[index];
        let header_bytes = routing::QRTR_HEADER_BYTES;
        if rx.used < header_bytes || word(&rx.bytes, 0) != 1 || word(&rx.bytes, 4) != 1
            || word(&rx.bytes, 8) != self.config.peer_node || word(&rx.bytes, 24) != self.config.local_node
        {
            return;
        }
        let body_bytes = word(&rx.bytes, 20) as usize;
        if body_bytes > rx.used - header_bytes || align(header_bytes + body_bytes, 4) != rx.used {
            return;
        }
        let packet = &rx.bytes[header_bytes..header_bytes + body_bytes];
        let (opcode, block_or_error) = match packet {
            [0, opcode @ (3 | 5), ..] => (*opcode,
                packet.get(2..4).map(|pair| u16::from_be_bytes([pair[0], pair[1]]))),
            packet if packet.starts_with(REQUEST_PREFIX) => (2, None),
            _ => return,
        };
        let source_port = word(&rx.bytes, 12);
        let destination_port = word(&rx.bytes, 28);
        let clock = self.intent_ack_clock;
        for write in self.server_check_traces_mut()
            .filter(|write| write.peer.port == source_port && write.oack.phase >= 3 && write.timing.closes == 0)
        {
            let receive = match opcode {
                3 => &mut write.timing.peer_rx.data,
                5 => &mut write.timing.peer_rx.error,
                _ => &mut write.timing.peer_rx.repeat,
            };
            receive.count = receive.count.saturating_add(1);
            if receive.first.is_none() {
                receive.first = Some(ElfPeerPacket {
                    destination_port, body_bytes: body_bytes as u16, block_or_error,
                    received_at: clock.map(|clock| clock()),
                });
            }
        }
    }

    fn finish_receive(&mut self, index: usize) -> Result<Event, Error> {
        self.routing_attempt = Some(index);
        let result = self.finish_receive_inner(index);
        if result.is_ok() { self.routing_attempt = None; }
        result
    }

    fn finish_receive_inner(&mut self, index: usize) -> Result<Event, Error> {
        let rx = &self.receive[index];
        if rx.used < 32 {
            return Err(qmi::Error::ShortHeader.into());
        }
        // Linux pads the QRTR body to four bytes, independently of GLINK's
        // eight-byte record padding. Strip ONLY that declared QRTR padding.
        let body = word(&rx.bytes, 20) as usize;
        if body > MAX_DATAGRAM_BYTES - 32 || align(32 + body, 4) != rx.used {
            return Err(qmi::Error::LengthMismatch.into());
        }
        let len = 32 + body;
        if word(&rx.bytes, 4) == 7 {
            return self.receive_credit(index, len);
        }
        if word(&rx.bytes, 4) != 1 {
            return self.receive_control(index, len);
        }
        let peer = Address { node: word(&rx.bytes, 8), port: word(&rx.bytes, 12) };
        if self.retired_rmtfs_peers.contains(&Some(peer)) {
            return Err(Error::FlowBusy);
        }
        if self.control.started() {
            if !self.control.status().peer_hello {
                return Err(control::Error::PeerNotReady.into());
            }
            if [
                Service::RemoteStorage,
                Service::HeapMemory,
                Service::ServiceRegistry,
                Service::IpaServer,
            ]
            .iter()
            .any(|service| {
                self.router.endpoint(*service).is_some_and(|endpoint| {
                    endpoint.address.port == word(&self.receive[index].bytes, 28)
                        && !self.control.accepts(*service)
                })
            }) {
                return Err(control::Error::Unavailable.into());
            }
        }
        let destination = Address {
            node: word(&self.receive[index].bytes, 24),
            port: word(&self.receive[index].bytes, 28),
        };
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa_port_in_use(destination.port) {
            return self.receive_linux_ipa(index, len);
        }
        let local_port_in_use = [
            Service::RemoteStorage,
            Service::HeapMemory,
            Service::ServiceRegistry,
            Service::IpaServer,
        ]
        .iter()
        .any(|service| {
            self.router
                .endpoint(*service)
                .is_some_and(|endpoint| endpoint.address.port == destination.port)
        }) || self
            .ipa_client
            .as_ref()
            .is_some_and(|client| client.local_address().port == destination.port)
            || self
                .wlfw_client
                .as_ref()
                .is_some_and(|client| client.local_address().port == destination.port)
            || self
                .servreg_notif_client
                .as_ref()
                .is_some_and(|client| client.local_address().port == destination.port);
        let service_port = self.control.tftp_port();
        if service_port == Some(destination.port)
            || (self.tftp_server && !local_port_in_use && destination.port != control::CONTROL_PORT)
        {
            let source = Address {
                node: word(&self.receive[index].bytes, 8),
                port: word(&self.receive[index].bytes, 12),
            };
            if !self.control.accepts_tftp() {
                return Err(control::Error::Unavailable.into());
            }
            if word(&self.receive[index].bytes, 0) != 1
                || destination.node != self.config.local_node
                || source.node != self.config.peer_node
                || source.port == 0
                || source.port >= control::CONTROL_PORT
                || destination.port == 0
                || destination.port >= control::CONTROL_PORT
                || word(&self.receive[index].bytes, 16) > 1
            {
                return Err(control::Error::Address.into());
            }
            #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
            self.observe_server_check_rx(index, source, destination, len);
            #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
            if service_port == Some(destination.port)
                && let Some(trace) = self.elf_exchange.as_mut()
                && trace.completed && trace.first_tftp_request.is_none()
                && let [high, low, ..] = &self.receive[index].bytes[32..len]
            {
                use crate::qcom_tftp::{OP_RRQ, OP_WRQ, observe_request};
                let opcode = u16::from_be_bytes([*high, *low]);
                if matches!(opcode, OP_RRQ | OP_WRQ) {
                    let path_kind = match observe_request(&self.receive[index].bytes[32..len]) {
                        Ok(request) => match request.path_text() {
                            "/readwrite/server_check.txt" => "SERVER CHECK",
                            "/readwrite/modem_efs" => "MODEM EFS",
                            "/readwrite/ota_firewall/ruleset" => "OTA FIREWALL RULESET",
                            "/readwrite/mcfg.tmp" => "MCFG TMP",
                            "/readonly/firmware/image/wlanmdsp.mbn"
                            | "/readonly/vendor/firmware_mnt/image/wlanmdsp.mbn"
                            | "/readonly/vendor/firmware/wlanmdsp.mbn" => "WLANMDSP MBN",
                            _ => "OTHER PATH",
                        },
                        Err(_) => "PATH DECODE ERROR",
                    };
                    trace.first_tftp_request = Some(ElfTftpRequest {
                        opcode,
                        path_kind,
                        body_bytes: body as u32,
                        source_port: source.port,
                        destination_port: destination.port,
                    });
                }
            }
            #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
            {
                use crate::qcom_tftp_server_check::REQUEST_PREFIX;
                let rx = &mut self.receive[index];
                let packet = &rx.bytes[32..len];
                let request = TftpHandle {
                    owner: self.owner,
                    generation: self.generation,
                    intent: rx.tftp_datagram_id(),
                };
                let wrq = service_port == Some(destination.port) && packet.starts_with(REQUEST_PREFIX);
                #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
                let exact_wrq = packet == crate::qcom_tftp_server_check::REQUEST;
                #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
                let wrq = wrq && !core::mem::replace(&mut rx.elf_wrq_observed, true);
                let block = match packet { [0, 3, high, low, ..] => Some(u16::from_be_bytes([*high, *low])), _ => None };
                let clock = self.intent_ack_clock;
                let post_elf = self.elf_exchange.as_mut().filter(|trace| trace.completed);
                if wrq {
                    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
                    if self.first_server_check.is_none() {
                        self.first_server_check = Some(ElfServerCheckTrace::new(
                            request, source, clock.map(|clock| clock()), post_elf.is_none(),
                            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
                            exact_wrq,
                        ));
                    }
                    if let Some(trace) = post_elf
                        && trace.server_check.is_none()
                    {
                        trace.server_check = Some(ElfServerCheckTrace::new(
                            request, source, clock.map(|clock| clock()),
                            #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
                            false,
                            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
                            exact_wrq,
                        ));
                    }
                    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
                    if let Some(write) = self.first_server_check.as_mut()
                        && write.request != request && write.peer == source
                        && write.timing.closes == 0 && write.repeat.is_none()
                    {
                        write.repeat = Some(ServerCheckRepeat {
                            request: Some(request),
                            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
                            eligible: write.before_elf && write.exact_wrq && exact_wrq,
                            ..ServerCheckRepeat::default()
                        });
                    }
                }
                for write in self.server_check_traces_mut().filter(|write| write.peer == source) {
                    #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
                    if wrq && write.request != request {
                        if write.timing.repeats == 0 {
                            write.timing.first_repeat_at = clock.map(|clock| clock());
                        }
                        write.timing.repeats = write.timing.repeats.saturating_add(1);
                    }
                    if write.data.is_none() && write.transfer_port == Some(destination.port)
                        && let Some(block) = block
                    {
                        write.data = Some(ElfWriteData {
                            request, block, body_bytes: body as u32,
                            received_at: clock.map(|clock| clock()), result: None,
                            #[cfg(feature = "sunfish-ack-elf-phase-trace")]
                            stages: FinalAckStages::default(),
                        });
                    }
                }
            }
            if word(&self.receive[index].bytes, 16) == 1 {
                let resume_slot = match self.free_tx_slot() {
                    Some(slot) => slot,
                    None => {
                        #[cfg(feature = "sunfish-qrtr-deferred-requests")]
                        return self.defer_request(index);
                        #[cfg(not(feature = "sunfish-qrtr-deferred-requests"))]
                        return Err(Error::NoCapacity);
                    }
                };
                stage_resume_tx(&mut self.transmit[resume_slot], destination, source);
                #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
                { self.transmit[resume_slot].server_check_wrq =
                    self.receive[index].server_check_wrq.map(|trace| trace.intent); }
            }
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            if self.mcfg_first_data.is_none()
                && self.mcfg_oack.is_some_and(|trace| trace.destination == source)
                && body >= 2
            {
                let packet = &self.receive[index].bytes[32..len];
                let opcode = u16::from_be_bytes([packet[0], packet[1]]);
                if matches!(opcode, crate::qcom_tftp::OP_DATA | crate::qcom_tftp::OP_ERROR) {
                    self.mcfg_first_data = Some(McfgDataTrace {
                        opcode,
                        block_or_error: packet.get(2..4).map(|pair| u16::from_be_bytes([pair[0], pair[1]])),
                        packet_bytes: body as u32,
                        local_port: destination.port,
                    });
                }
            }
            if self.tftp_server {
                self.receive[index].state = RxState::Tftp;
                self.receive[index].done_pending = false;
                let handle=TftpHandle {owner:self.owner,generation:self.generation,intent:self.receive[index].tftp_datagram_id()};
                #[cfg(feature="sunfish-post-elf-mcfg-trace")]
                self.observe_post_mcfg_receive(index,handle);
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
                self.observe_startup_read(index,handle);
/* STARTUP_PREREQ_END */
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
                self.capture_efs_overlap_request(index,handle);
/* EFS_REPLY_OVERLAP_END */
                return Ok(Event::TftpRequest(handle));
            }
            let observation =
                crate::qcom_tftp::observe_request(&self.receive[index].bytes[32..len]);
            self.receive[index].state = RxState::Control;
            self.receive[index].done_pending = true;
            return Ok(Event::TftpObserved {
                peer: source,
                observation,
            });
        }
        if self
            .ipa_client
            .as_ref()
            .is_some_and(|client| client.local_address() == destination)
        {
            let confirm_rx = word(&self.receive[index].bytes, 16);
            if confirm_rx > 1 {
                return Err(ipa_client::Error::InvalidQrtr.into());
            }
            let resume_slot = if confirm_rx == 1 {
                Some(
                    self.free_tx_slot()
                        .ok_or(Error::NoCapacity)?,
                )
            } else {
                None
            };
            let (outcome, local, peer) = {
                let client = self.ipa_client.as_mut().unwrap();
                let peer = client.endpoint().ok_or(ipa_client::Error::NoEndpoint)?;
                let local = client.local_address();
                let datagram = &self.receive[index].bytes[..self.receive[index].used];
                let outcome =
                    client.accept_init_modem_driver_response(self.generation, datagram)?;
                (outcome, local, peer)
            };
            if let Some(slot) = resume_slot {
                stage_resume_tx(&mut self.transmit[slot], local, peer);
            }
            self.receive[index].state = RxState::Control;
            self.receive[index].done_pending = true;
            return Ok(Event::IpaInitModemDriver(outcome));
        }
        if self
            .servreg_notif_client
            .as_ref()
            .is_some_and(|client| client.local_address() == destination)
        {
            let confirm_rx = word(&self.receive[index].bytes, 16);
            if confirm_rx > 1 {
                return Err(servreg_notif::Error::InvalidDatagram.into());
            }
            let is_indication = self.receive[index].used > servreg_notif::QRTR_HEADER_BYTES
                && self.receive[index].bytes[servreg_notif::QRTR_HEADER_BYTES] == 4; // QMI message-kind wire value for an indication.
            if is_indication {
                // Reserve all required outbound records before accepting the
                // indication, so lack of transmit capacity cannot strand an
                // accepted notification without its protocol ACK.
                let mut free_slots = self
                    .transmit[..TX_SLOTS]
                    .iter()
                    .enumerate()
                    .filter(|(_, tx)| tx.len == 0)
                    .map(|(slot, _)| slot);
                let resume_slot = if confirm_rx == 1 {
                    Some(free_slots.next().ok_or(Error::NoCapacity)?)
                } else {
                    None
                };
                let ack_slot = free_slots.next().ok_or(Error::NoCapacity)?;
                let (request, outcome, local, peer) = {
                    let client = self.servreg_notif_client.as_mut().unwrap();
                    let peer = client.endpoint().ok_or(servreg_notif::Error::NoEndpoint)?;
                    let local = client.local_address();
                    let datagram = &self.receive[index].bytes[..self.receive[index].used];
                    client.accept_state_updated_indication(self.generation, datagram)?;
                    let request = client.begin_state_updated_ack(
                        self.generation,
                        servreg_notif::MAX_DATAGRAM_BYTES,
                    )?;
                    let outcome = client
                        .outcome()
                        .ok_or(servreg_notif::Error::NoPendingIndication)?;
                    (request, outcome, local, peer)
                };
                self.stage_servreg_notif_request(ack_slot, request)?;
                if let Some(slot) = resume_slot {
                    stage_resume_tx(&mut self.transmit[slot], local, peer);
                }
                self.receive[index].state = RxState::Control;
                self.receive[index].done_pending = true;
                return Ok(Event::ServregNotifState(outcome));
            }
            let resume_slot = if confirm_rx == 1 {
                Some(
                    self.free_tx_slot()
                        .ok_or(Error::NoCapacity)?,
                )
            } else {
                None
            };
            let (outcome, local, peer) = {
                let client = self.servreg_notif_client.as_mut().unwrap();
                let peer = client.endpoint().ok_or(servreg_notif::Error::NoEndpoint)?;
                let local = client.local_address();
                let datagram = &self.receive[index].bytes[..self.receive[index].used];
                let outcome = client.accept_response(self.generation, datagram)?;
                (outcome, local, peer)
            };
            if let Some(slot) = resume_slot {
                stage_resume_tx(&mut self.transmit[slot], local, peer);
            }
            self.receive[index].state = RxState::Control;
            self.receive[index].done_pending = true;
            return Ok(Event::ServregNotifState(outcome));
        }
        if self
            .wlfw_client
            .as_ref()
            .is_some_and(|client| client.local_address() == destination)
        {
            let confirm_rx = word(&self.receive[index].bytes, 16);
            if confirm_rx > 1 {
                return Err(wlfw::Error::InvalidQrtr.into());
            }
            let resume_slot = if confirm_rx == 1 {
                Some(
                    self.free_tx_slot()
                        .ok_or(Error::NoCapacity)?,
                )
            } else {
                None
            };
            let (event, local, peer) = {
                let client = self.wlfw_client.as_mut().unwrap();
                let peer = client.endpoint().ok_or(wlfw::Error::NoEndpoint)?;
                let local = client.local_address();
                let datagram = &self.receive[index].bytes[..self.receive[index].used];
                let event = if datagram.get(wlfw::QRTR_HEADER_BYTES) == Some(&4) {
                    Event::WlfwIndication(
                        client.accept_startup_indication(self.generation, datagram)?,
                    )
                } else {
                    match client.pending_message_id() {
                        Some(crate::wlfw_protocol::WLFW_IND_REGISTER_MESSAGE_ID) => {
                            Event::WlfwRegistration(
                                client.accept_indication_registration_response(
                                    self.generation,
                                    datagram,
                                )?,
                            )
                        }
                        Some(crate::wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID) => Event::WlfwMsaInfo(
                            client.accept_msa_info_response(self.generation, datagram)?,
                        ),
                        Some(crate::wlfw_protocol::WLFW_MSA_READY_MESSAGE_ID) => {
                            Event::WlfwMsaReady(
                                client.accept_msa_ready_response(self.generation, datagram)?,
                            )
                        }
                        Some(crate::wlfw_protocol::WLFW_CAP_MESSAGE_ID) => {
                            client.accept_capability_response(self.generation, datagram)?;
                            Event::WlfwCapabilities(
                                client
                                    .capability_summary()
                                    .ok_or(wlfw::Error::NoPendingRequest)?,
                            )
                        }
                        Some(crate::wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID) => {
                            Event::WlfwDynamicFeatureMask(
                                client.accept_dynamic_feature_mask_response(
                                    self.generation,
                                    datagram,
                                )?,
                            )
                        }
                        Some(crate::wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID) => {
                            Event::WlfwBdfSegment(
                                client.accept_bdf_download_response(self.generation, datagram)?,
                            )
                        }
                        Some(crate::wlfw_protocol::WLFW_CAL_REPORT_MESSAGE_ID) => {
                            Event::WlfwCalReport(
                                client.accept_cal_report_response(self.generation, datagram)?,
                            )
                        }
                        Some(crate::wlfw_protocol::WLFW_WLAN_CFG_MESSAGE_ID) => Event::WlfwConfig(
                            client.accept_wlan_config_response(self.generation, datagram)?,
                        ),
                        Some(crate::wlfw_protocol::WLFW_WLAN_MODE_MESSAGE_ID) => Event::WlfwMode(
                            client.accept_wlan_mode_response(self.generation, datagram)?,
                        ),
                        _ => return Err(wlfw::Error::NoPendingRequest.into()),
                    }
                };
                (event, local, peer)
            };
            if let Some(slot) = resume_slot {
                stage_resume_tx(&mut self.transmit[slot], local, peer);
            }
            self.receive[index].state = RxState::Control;
            self.receive[index].done_pending = true;
            return Ok(event);
        }
        // The pinned Linux IPA server has only 0x20 and 0x35 handlers. The
        // separately opted-in branches discard unhandled requests without a
        // QMI response, after validating the complete transport envelope.
        #[cfg(feature = "sunfish-ipa-ignore-config-reference")]
        if self.linux_ipa.is_some()
            && self.router.endpoint(Service::IpaServer).is_some_and(|ep| ep.address == destination)
            && len >= 39
            && {
                let id = u16::from_le_bytes([self.receive[index].bytes[35], self.receive[index].bytes[36]]);
                id == crate::sunfish_ipa_linux_qmi::CONFIG_REQUEST
                    || (cfg!(feature = "sunfish-ipa-ignore-unhandled-reference")
                        && id != crate::sunfish_ipa_linux_qmi::INDICATION_REGISTER
                        && id != crate::sunfish_ipa_linux_qmi::DRIVER_INIT_COMPLETE)
            }
        {
            if word(&self.receive[index].bytes, 0) != 1
                || peer.node != self.config.peer_node || peer.port == 0 || peer.port >= control::CONTROL_PORT
                || destination.node != self.config.local_node
            {
                return Err(control::Error::Address.into());
            }
            let confirm = word(&self.receive[index].bytes, 16);
            if confirm > 1 { return Err(control::Error::Address.into()); }
            let message = Message::decode(&self.receive[index].bytes[32..len])?;
            #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
            let ignored_init_response = message.kind == qmi::Kind::Response
                && message.message_id == crate::sunfish_ipa_linux_qmi::INIT_COMPLETE
                && self.matches_published_ipa_indication(peer, message.transaction);
            #[cfg(not(feature = "sunfish-ipa-ignore-init-response-reference"))]
            let ignored_init_response = false;
            if message.kind != qmi::Kind::Request && !ignored_init_response { return Err(Error::InvalidReply); }
            let ignored_id = message.message_id;
            let resume_slot = if confirm == 1 {
                match self.free_tx_slot() {
                    Some(slot) => Some(slot),
                    None => {
                        #[cfg(feature = "sunfish-qrtr-deferred-requests")]
                        return self.defer_request(index);
                        #[cfg(not(feature = "sunfish-qrtr-deferred-requests"))]
                        return Err(Error::NoCapacity);
                    }
                }
            } else { None };
            if let Some(slot) = resume_slot {
                stage_resume_tx(&mut self.transmit[slot], destination, peer);
            }
            self.receive[index].state = RxState::Control;
            self.receive[index].done_pending = true;
            #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
            if ignored_init_response { return Ok(Event::LinuxIpaInitResponseIgnored); }
            if ignored_id == crate::sunfish_ipa_linux_qmi::CONFIG_REQUEST {
                return Ok(Event::LinuxIpaConfigIgnored);
            }
            #[cfg(feature = "sunfish-ipa-ignore-unhandled-reference")]
            return Ok(Event::LinuxIpaUnhandledIgnored { message_id: ignored_id });
            #[cfg(not(feature = "sunfish-ipa-ignore-unhandled-reference"))]
            return Err(Error::InvalidReply);
        }
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        self.observe_ipa_registration_receive(index, len, peer, destination);
        if self.router.in_flight() >= MAX_REQUESTS || self.free_tx_slot().is_none() {
            #[cfg(feature = "sunfish-qrtr-deferred-requests")]
            return self.defer_request(index);
            #[cfg(not(feature = "sunfish-qrtr-deferred-requests"))]
            return Err(Error::NoCapacity);
        }
        let spare = self.free_tx_slot().ok_or(Error::NoCapacity)?;
        // A failed route leaves the complete packet held. Retain only its
        // slot index so the failure screen can inspect bounded header metadata.
        self.routing_attempt = Some(index);
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        let routed = if self.linux_ipa.is_some() {
            self.router
                .route_linux_ipa(self.generation, &self.receive[index].bytes[..len])?
        } else {
            self.router
                .route(self.generation, &self.receive[index].bytes[..len])?
        };
        #[cfg(not(feature = "sunfish-linux-ipa-qmi"))]
        let routed = self
            .router
            .route(self.generation, &self.receive[index].bytes[..len])?;
        let context = routed.context;
        #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
        let first_elf = self.elf_host_credit.is_none()
            && matches!(routed.request, Request::ServiceRegistry(query)
                if query.service_name == b"kernel/elf_loader");
        self.routing_attempt = None;
        self.receive[index].context = Some(context);
        self.receive[index].ticket = Some(routed.ticket);
        self.receive[index].state = RxState::Request;
        self.receive[index].done_pending = true;
        if context.confirm_rx {
            let tx = &mut self.transmit[spare];
            stage_resume_tx(tx, context.local, context.peer);
        }
        let handle = RequestHandle {
            owner: self.owner,
            generation: self.generation,
            intent: self.receive[index].id,
        };
        #[cfg(feature = "sunfish-ipa-registration-trace")]
        self.note_ipa_registration_routed(handle);
        #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
        if first_elf {
            self.elf_host_credit = Some(ElfHostCredit {
                request: handle,
                resume_slot: context.confirm_rx.then_some(spare),
                requested: context.confirm_rx,
                remote_intent: None,
                completed: false,
                before_reply: None,
            });
        }
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        if first_elf { self.start_post_elf_mcfg(handle,index); }
        #[cfg(feature="sunfish-elf-window-trace")]
        if first_elf {self.start_elf_window(handle,index);}
        Ok(Event::Request(handle))
    }

    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    fn defer_request(&mut self, index: usize) -> Result<Event, Error> {
        let next = self.next_deferred_sequence.checked_add(1).ok_or(Error::IdExhausted)?;
        let rx = &mut self.receive[index];
        if rx.state != RxState::Held { return Err(Error::FragmentOrder); }
        rx.state = RxState::Deferred;
        rx.deferred_sequence = self.next_deferred_sequence;
        self.next_deferred_sequence = next;
        Ok(Event::RequestDeferred)
    }

    /// A deferred packet already lives in a private RX intent. It is retried
    /// only after capacity exists; no peer acknowledgment is fabricated.
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    pub(crate) fn deferred_ready(&self) -> bool {
        self.router.in_flight() < MAX_REQUESTS && self.free_tx_slot().is_some()
            && self.receive.iter().any(|rx| rx.state == RxState::Deferred)
    }
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    pub(crate) fn retry_deferred(&mut self, generation: u64) -> Result<Option<Event>, Error> {
        self.check(generation)?;
        if !self.deferred_ready() { return Ok(None); }
        let index = self.receive.iter().enumerate().filter(|(_, rx)| rx.state == RxState::Deferred)
            .min_by_key(|(_, rx)| rx.deferred_sequence).map(|(index, _)| index).ok_or(Error::InvalidIntent)?;
        self.receive[index].state = RxState::Held;
        self.finish_receive(index).map(Some)
    }

    fn receive_control(&mut self, index: usize, len: usize) -> Result<Event, Error> {
        let command = self.control.decode(&self.receive[index].bytes[..len])?;
/* EFS_REPLY_OVERLAP_BEGIN */
#[cfg(feature="sunfish-efs-reply-overlap")]
        if let control::Command::DelClient(port)=command { self.close_efs_overlap(port); }
/* EFS_REPLY_OVERLAP_END */
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        if let control::Command::DelClient(port)=command {self.observe_post_mcfg_close(port);}
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
        if let control::Command::DelClient(port)=command {self.observe_startup_close(port);}
/* STARTUP_PREREQ_END */
        #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
        if self.first_oack_after_ipa_publish && self.first_oack_eligibility.close_mask.is_none()
            && let control::Command::DelClient(port) = command
            && let Some(first) = self.first_oack_transfer().filter(|first| first.peer.port == port)
        {
            let mask = self.first_oack_blockers(first);
            self.first_oack_eligibility.close_mask = Some(mask);
        }
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        if let control::Command::DelClient(port) = command
            && let Some(trace) = self.server_check_transfers.iter_mut().flatten()
                .find(|trace| trace.peer.port == port && trace.first_close_at.is_none())
        {
            trace.first_close_at = self.intent_ack_clock.map(|clock| clock());
            #[cfg(feature = "sunfish-tftp-oack-resources")]
            if trace.oack_at.is_some() && trace.first_data_at.is_none() {
                let slot = (trace.transfer_port - crate::qcom_tftp_server_check::TRANSFER_PORT) as usize;
                self.server_check_resources[slot].close_intent = Some(self.receive[index].id);
            }
        }
        #[cfg(feature = "sunfish-ipa-init-open-writer")]
        if command == control::Command::DelClient(0x1f) && self.original_1f_close_count() == Some(0)
        {
            self.observe_init_at_first_close();
        }
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        if let control::Command::DelClient(port) = command {
            let clock = self.intent_ack_clock;
            for write in self.server_check_traces_mut().filter(|write| write.peer.port == port) {
                if write.timing.closes == 0 {
                    write.timing.first_close_at = clock.map(|clock| clock());
                    write.timing.first_close_oack_phase = Some(write.oack.phase);
                    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
                    if let Some(repeat) = write.repeat.as_mut() {
                        repeat.close_phase = Some(repeat.oack.phase);
                    }
                }
                write.timing.closes = write.timing.closes.saturating_add(1);
            }
        }
        let closed = match command {
            control::Command::DelClient(port) => Some(port),
            control::Command::DelServer(server) => Some(server.address.port),
            _ => None,
        };
        if let Some(port) = closed {
            #[cfg(feature="sunfish-mcfg-close-cleanup")]
            if matches!(command, control::Command::DelClient(_)) && self.hold_mcfg_close(Address{node:self.config.peer_node,port},index) {
                return Ok(Event::McfgClosePending{peer:Address{node:self.config.peer_node,port}});
            }
            #[cfg(feature = "sunfish-qrtr-deferred-requests")]
            if let Some(first) = self.receive.iter().position(|rx| rx.state == RxState::Deferred && rx.used >= 32 &&
                word(&rx.bytes, 8) == self.config.peer_node && word(&rx.bytes, 12) == port) {
                let rx = &self.receive[first];
                let matching = self.receive.iter().filter(|rx| rx.state == RxState::Deferred && rx.used >= 32 &&
                    word(&rx.bytes, 8) == self.config.peer_node && word(&rx.bytes, 12) == port).count() as u32;
                self.deferred_close_diagnostic = Some(DeferredCloseDiagnostic {
                    command: match command { control::Command::DelClient(_) => 6, _ => 5 },
                    peer_node: self.config.peer_node,
                    peer_port: port, destination_port: word(&rx.bytes, 28),
                    opcode: if rx.used >= 34 { u32::from(u16::from_be_bytes([rx.bytes[32], rx.bytes[33]])) } else { u32::MAX },
                    matching, block_mask: 0,
                });
                #[cfg(feature = "sunfish-tftp-deferred-close-experiment")]
                {
                    let peer = Address { node: self.config.peer_node, port };
                    let request = crate::qcom_tftp_server_check::REQUEST;
                    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
                    let owner_clear = self.tftp_owner_snapshot_valid && !self.tftp_owner_peers.contains(&Some(peer));
                    #[cfg(not(feature = "sunfish-tftp-peer-close-experiment"))]
                    let owner_clear = !self.tftp_owner_busy;
                    let exact_wrq = matches!(command, control::Command::DelClient(_))
                        && self.tftp_server && matching == 1
                        && word(&rx.bytes, 28) == crate::qcom_tftp_server_check::SERVICE_PORT
                        && word(&rx.bytes, 20) as usize == request.len()
                        && rx.used >= 32 + request.len()
                        && &rx.bytes[32..32 + request.len()] == request;
                    let other_receive = self.receive.iter().enumerate().any(|(i, other)| {
                        i != first && i != index && other.used >= 32
                            && word(&other.bytes, 8) == peer.node && word(&other.bytes, 12) == peer.port
                            && !matches!(other.state, RxState::Free | RxState::Advertising | RxState::Available)
                    });
                    let other_transmit = self.transmit.iter().any(|tx| tx.len != 0
                        && tx.kind.qrtr_addresses().is_some_and(|(_, destination)| destination == peer));
                    let tombstone = self.retired_rmtfs_peers.iter().position(Option::is_none);
                    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
                    let no_storage_history = !self.rmtfs_flow_destinations.contains(&Some(peer));
                    #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
                    let no_storage_history = true;
                    let already_fenced = self.retired_rmtfs_peers.contains(&Some(peer));
                    let block_mask = u32::from(!exact_wrq) | (u32::from(!owner_clear) << 1)
                        | (u32::from(other_receive) << 2) | (u32::from(other_transmit) << 3)
                        | (u32::from(!no_storage_history) << 4) | (u32::from(already_fenced) << 5)
                        | (u32::from(tombstone.is_none()) << 6);
                    if let Some(info) = self.deferred_close_diagnostic.as_mut() { info.block_mask = block_mask; }
                    // Prototype only: retain both receive slots and stop all
                    // further session operations. The owner must validate its
                    // held reply and server state before a future commit path.
                    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
                    if block_mask == 2 && self.tftp_owner_snapshot_valid {
                        self.pending_tftp_close = Some((peer, first, index));
                        return Ok(Event::TftpClosePending { peer });
                    }
                    if exact_wrq && owner_clear && !other_receive && !other_transmit
                        && no_storage_history && !self.retired_rmtfs_peers.contains(&Some(peer)) && tombstone.is_some() {
                        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
                        {
                            let key = crate::qcom_qrtr_flow::Destination { node: peer.node, port: peer.port };
                            if self.rmtfs_reference_flows.contains(key) && self.rmtfs_reference_flows.close(key).is_err() {
                                if let Some(info) = self.deferred_close_diagnostic.as_mut() { info.block_mask |= 1 << 7; }
                                self.quarantined = true;
                                return Err(control::Error::ClientGone.into());
                            }
                        }
                        // The peer has explicitly closed. Fence its port for
                        // this generation, then return this consumed GLINK
                        // intent through normal RX_DONE publication.
                        self.retired_rmtfs_peers[tombstone.unwrap()] = Some(peer);
                        self.receive[first].state = RxState::Control;
                        self.receive[first].done_pending = true;
                        self.deferred_close_diagnostic = None;
                    } else {
                        self.quarantined = true;
                        return Err(control::Error::ClientGone.into());
                    }
                }
                #[cfg(not(feature = "sunfish-tftp-deferred-close-experiment"))]
                {
                    self.quarantined = true;
                    return Err(control::Error::ClientGone.into());
                }
            }
            #[cfg(feature = "sunfish-tftp-close-owner-fence")]
            if matches!(command, control::Command::DelClient(_)) {
                let peer = Address { node: self.config.peer_node, port };
                let owner_busy = self.tftp_firmware_peers.contains(&Some(peer));
                let tx_busy = self.transmit.iter().any(|tx| tx.len != 0
                    && matches!(tx.kind, TxKind::TftpReply { source, destination } if destination == peer
                        && (0x5100..=0x51ff).contains(&source.port)));
                #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
                if owner_busy {
                    let other_rx = self.receive.iter().enumerate().any(|(i, rx)| i != index && rx.used >= 32
                        && word(&rx.bytes, 8) == peer.node && word(&rx.bytes, 12) == peer.port
                        && !matches!(rx.state, RxState::Free | RxState::Advertising | RxState::Available));
                    let other_tx = self.transmit.iter().any(|tx| tx.len != 0
                        && tx.kind.qrtr_addresses().is_some_and(|(_, to)| to == peer)
                        && (!matches!(tx.kind, TxKind::TftpReply { source, .. } if (0x5100..=0x51ff).contains(&source.port))
                            || tx.glink_done || !((tx.sent == 0 && tx.remote_slot.is_none()) || tx.sent == tx.len)));
                    if !other_rx && !other_tx {
                        self.pending_firmware_close = Some((peer, index));
                        return Ok(Event::FirmwareClosePending { peer });
                    }
                }
                if owner_busy || tx_busy {
                    self.quarantined = true;
                    return Err(control::Error::ClientGone.into());
                }
            }
            if matches!(command, control::Command::DelClient(_)) {
                let mut matching = self.receive.iter().enumerate().filter(|(_, rx)|
                    rx.context.is_some_and(|context| context.peer.port == port));
                let candidate = matching.next().and_then(|(rx_index, rx)| {
                    let context = rx.context?;
                    if matching.next().is_some() || context.service != Service::RemoteStorage
                        || rx.state != RxState::ReplyQueued || rx.done_pending || rx.ticket.is_none() {
                        return None;
                    }
                    let mut txs = self.transmit.iter().enumerate().filter(|(_, tx)|
                        tx.len != 0 && tx.kind.qrtr_addresses().is_some_and(|(_, dest)| dest == context.peer));
                    let (tx_index, tx) = txs.next()?;
                    if txs.next().is_some() || tx.sent != tx.len || !tx.glink_done
                        || !matches!(tx.kind, TxKind::Reply { receive, ipa_token: None, .. } if receive == rx_index) {
                        return None;
                    }
                    Some((rx_index, tx_index, context.peer))
                });
                if let Some((rx_index, tx_index, peer)) = candidate {
                    if let Some(tombstone) = self.retired_rmtfs_peers.iter().position(Option::is_none) {
                        let event = self.control.receive(&self.receive[index].bytes[..len])?;
                        if let Err(error) = self.observe_client_control(event) {
                            self.quarantined = true;
                            return Err(error);
                        }
                        self.retired_rmtfs_peers[tombstone] = Some(peer);
                        let ticket = self.receive[rx_index].ticket.take().ok_or(Error::InvalidHandle)?;
                        if let Err(error) = self.router.complete(ticket) {
                            self.quarantined = true;
                            return Err(error.into());
                        }
                        let flow = self.transmit[tx_index].kind.qrtr_addresses();
                        self.receive[rx_index] = Receive::empty();
                        self.transmit[tx_index] = Transmit::empty();
                        #[cfg(feature="sunfish-elf-tx-slots")]
                        self.observe_elf_tx_freed(tx_index,4);
                        if let Some((source, destination)) = flow {
                            self.forget_qrtr_flow_if_idle(source, destination);
                        }
                        self.receive[index].state = RxState::Control;
                        self.receive[index].done_pending = true;
                        return Ok(Event::QrtrControl(event));
                    }
                }
            }
            let mut matching = self.receive.iter().enumerate().filter(|(_, rx)| {
                rx.context.is_some_and(|context| context.peer.port == port)
            });
            if let Some((rx_index, rx)) = matching.next() {
                let mut replies = self.transmit.iter().filter(|tx| {
                    matches!(tx.kind, TxKind::Reply { receive, .. } if receive == rx_index)
                        && tx.len != 0
                });
                let reply = replies.next();
                let reply_count = u32::from(reply.is_some()) + replies.count() as u32;
                let context = rx.context.ok_or(Error::InvalidHandle)?;
                let receive_state = match rx.state {
                    RxState::Free => 0,
                    RxState::Advertising => 1,
                    RxState::Available => 2,
                    RxState::Assembling => 3,
                    RxState::Request => 4,
                    RxState::ReplyQueued => 5,
                    RxState::Control => 6,
                    RxState::Tftp => 7,
                    RxState::Held => 8,
                    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
                    RxState::Deferred => 10,
                    #[cfg(feature = "sunfish-linux-ipa-qmi")]
                    RxState::LinuxIpa => 9,
                };
                self.client_gone_diagnostic = Some(ClientGoneDiagnostic {
                    command: match command { control::Command::DelClient(_) => 6, _ => 5 },
                    port,
                    service: context.service,
                    transaction: context.transaction,
                    message_id: context.message_id,
                    receive_state,
                    done_pending: rx.done_pending,
                    matching_requests: 1 + matching.count() as u32,
                    reply_count,
                    reply_len: reply.map_or(0, |tx| tx.len as u32),
                    reply_sent: reply.map_or(0, |tx| tx.sent as u32),
                    reply_glink_done: reply.is_some_and(|tx| tx.glink_done),
                    reply_qrtr_done: reply.is_some_and(|tx| tx.qrtr_done),
                });
                self.quarantined = true;
                return Err(control::Error::ClientGone.into());
            }
        }
        let event = match self.control.receive(&self.receive[index].bytes[..len]) {
            Ok(event) => event,
            Err(error @ control::Error::PeerGone) => {
                self.quarantined = true;
                return Err(error.into());
            }
            Err(error) => return Err(error.into()),
        };
        #[cfg(feature = "sunfish-ipa-before-first-oack")]
        if let control::Event::ClientRemoved(port) = event {
            if let Err(error) = self.cancel_unpublished_first_oack(port) {
                self.quarantined = true;
                return Err(error);
            }
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
        if let control::Event::ClientRemoved(port) = event {
            self.cancel_queued_tftp_retry(port)?;
        }
        #[cfg(feature = "sunfish-tftp-oack-reserve")]
        if let control::Event::ClientRemoved(port) = event {
            self.cancel_unpublished_reserved_oack(port)?;
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        if let control::Event::ClientRemoved(port) = event {
            let peer = Address { node: self.config.peer_node, port };
            let key = crate::qcom_qrtr_flow::Destination { node: peer.node, port: peer.port };
            if self.rmtfs_flow_destinations.contains(&Some(peer)) && !self.retired_rmtfs_peers.contains(&Some(peer)) {
                let Some(slot) = self.retired_rmtfs_peers.iter().position(Option::is_none) else {
                    self.quarantined = true;
                    return Err(Error::NoCapacity);
                };
                if self.rmtfs_reference_flows.close(key).is_err() {
                    self.quarantined = true;
                    return Err(Error::FlowBusy);
                }
                self.retired_rmtfs_peers[slot] = Some(peer);
            }
        }
        self.observe_client_control(event)?;
        #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
        if let control::Event::Server { server, added: true, .. } = event
            && let Some(trace) = self.elf_exchange.as_mut()
            && trace.completed
            && let Some(slot) = trace.credit.servers.iter_mut().find(|slot| slot.is_none())
        {
            *slot = Some(server);
        }
        #[cfg(feature = "sunfish-glink-announcement-reuse")]
        { self.receive[index].announcement_reuse = matches!(command, control::Command::NewServer(_))
            && self.receive[index].context.is_none() && self.receive[index].ticket.is_none()
            && self.transmit.iter().any(|tx| tx.len != 0 && tx.sent == tx.len && !tx.glink_done
                && matches!(tx.kind, TxKind::Reply { route, .. }
                    if route.service == Service::ServiceRegistry && route.transaction == 3)); }
        self.receive[index].state = RxState::Control;
        self.receive[index].done_pending = true;
        Ok(Event::QrtrControl(event))
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    fn cancel_queued_tftp_retry(&mut self, port: u32) -> Result<(), Error> {
        let Some(repeat) = self.first_server_check.as_mut().filter(|write| write.peer.port == port)
            .and_then(|write| write.repeat.as_mut()).filter(|repeat| repeat.eligible && repeat.oack.phase == 2)
        else { return Ok(()); };
        let slot = repeat.oack.slot.ok_or(Error::InvalidReply)?;
        let tx = &self.transmit[slot];
        let TxKind::TftpReply { source, destination } = tx.kind else { return Err(Error::InvalidReply); };
        // This OACK fits in one GLINK record. A published record keeps its
        // remote intent and accounting token until its real completion.
        if tx.sent != 0 || tx.remote_slot.is_some() || destination.port != port {
            return Err(Error::InvalidReply);
        }
        let reservation = tx.reference_flow.as_ref().ok_or(Error::InvalidReply)?;
        self.rmtfs_reference_flows.fail_unpublished(reservation).map_err(|_| Error::InvalidReply)?;
        self.transmit[slot] = Transmit::empty();
        repeat.oack.slot = None;
        repeat.cancelled = true;
        #[cfg(feature="sunfish-elf-tx-slots")]
        self.observe_elf_tx_freed(slot,5);
        self.forget_qrtr_flow_if_idle(source, destination);
        Ok(())
    }

    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    pub(crate) fn original_server_check(&self) -> Option<(TftpHandle, bool)> {
        self.first_server_check.as_ref().map(|wrq| (wrq.request, wrq.exact_wrq))
    }

    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    pub(crate) fn ipa_endpoint_ready_at(&self) -> Option<u64> {
        self.ipa_endpoint_ready_at
    }

    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    fn observe_ipa_endpoint_ready(&mut self) {
        if self.ipa_endpoint_ready_at.is_none() && self.linux_ipa.is_some()
            && self.control.accepts(Service::IpaServer) && self.control.ipa_modem_endpoint().is_some()
        {
            self.ipa_endpoint_ready_at = self.intent_ack_clock.map(|clock| clock());
        }
    }

    fn observe_client_control(&mut self, event: control::Event) -> Result<(), Error> {
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        self.observe_linux_ipa_control()?;
        #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
        self.observe_ipa_endpoint_ready();
        let control::Event::Server { server, added, .. } = event else {
            return Ok(());
        };
        #[cfg(feature = "sunfish-ipa-announcement-timeline")]
        if added && self.ipa_announcement.is_none()
            && server.service == control::IPA_SERVICE_ID
            && server.instance == control::IPA_MODEM_VERSION_INSTANCE
        {
            self.ipa_announcement = Some((server.address.port, self.intent_ack_clock.map(|clock| clock())));
        }
        if server.service == ipa_wire::QRTR_SERVICE_ID
            && server.instance == ipa_wire::QRTR_VERSION_INSTANCE
        {
            if let Some(client) = self.ipa_client.as_mut() {
                if added {
                    client.observe_server(self.generation, server)?;
                } else {
                    client.remove_server(self.generation, server)?;
                }
            }
        }
        if server.service == control::WLFW_SERVICE_ID
            && server.instance == control::WLFW_VERSION_INSTANCE
        {
            if let Some(client) = self.wlfw_client.as_mut() {
                if added {
                    client.observe_server(self.generation, server)?;
                } else {
                    client.remove_server(self.generation, server)?;
                }
            }
            if !added && self.wlfw_power_gate {
                self.wlfw_power_ready = false;
            }
        }
        if server.service == crate::qcom_servreg_notif::SERVICE_ID
            && server.instance == servreg_notif::VERSION_INSTANCE
        {
            if let Some(client) = self.servreg_notif_client.as_mut() {
                if added {
                    client.observe_server(self.generation, server)?;
                } else {
                    client.remove_server(self.generation, server)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn receive_target(&self) -> usize {
        #[cfg(feature = "sunfish-first-wrq-spare-oack")]
        if self.first_wrq_spare_oack && self.first_server_check.is_none() {
            return control::RECEIVE_TARGET + 1;
        }
        control::RECEIVE_TARGET
    }

    fn maintain_receives(&mut self, generation: u64) -> Result<(), Error> {
        let available = self
            .receive
            .iter()
            .filter(|rx| {
                matches!(
                    rx.state,
                    RxState::Advertising | RxState::Available | RxState::Assembling
                )
            })
            .count();
        if available < self.receive_target()
            && self.receive[..RX_SLOTS].iter().any(|rx| rx.state == RxState::Free)
        {
            self.offer_receive(generation)?;
        }
        Ok(())
    }

    fn stage_control(&mut self) {
        if let Some(token) = self.control.next() {
            if let Some(tx) = self.transmit[..TX_SLOTS].iter_mut().find(|tx| tx.len == 0) {
                let bytes = self.control.encode(token);
                tx.bytes[..bytes.len()].copy_from_slice(&bytes);
                tx.len = bytes.len();
                tx.kind = TxKind::Control(token);
                self.control.queued(token);
            }
        }
    }

    fn stage_wlfw_registration(&mut self) -> Result<(), Error> {
        if self.wlfw_power_gate && !self.wlfw_power_ready {
            return Ok(());
        }
        if self
            .wlfw_client
            .as_ref()
            .is_none_or(|client| client.state() != WlfwState::ReadyToRegister)
        {
            return Ok(());
        }
        let Some(slot) = self.free_tx_slot() else {
            return Ok(());
        };
        let request = self
            .wlfw_client
            .as_mut()
            .unwrap()
            .begin_icnss_indication_registration(self.generation, wlfw::MAX_DATAGRAM_BYTES)?;
        self.stage_wlfw_request(slot, request)
    }

    fn stage_wlfw_dynamic_feature_mask(&mut self) -> Result<(), Error> {
        if self.wlfw_power_gate && !self.wlfw_power_ready {
            return Ok(());
        }
        let should_start = self.wlfw_client.as_ref().is_some_and(|client| {
            client.state() == WlfwState::CapabilitiesAccepted && !client.dynamic_feature_mask_done()
        });
        if should_start {
            // No free GLINK transmit slot is ordinary backpressure; the next
            // publish attempt retries staging after existing records drain.
            if self.free_tx_slot().is_none() {
                return Ok(());
            }
            self.begin_wlfw_dynamic_feature_mask(self.generation)?;
        }
        Ok(())
    }

    fn stage_wlfw_request(
        &mut self,
        slot: usize,
        request: wlfw::OutboundRequest,
    ) -> Result<(), Error> {
        self.check_wlfw_power()?;
        if request.generation != self.generation
            || request.len < 32
            || request.len > wlfw::MAX_DATAGRAM_BYTES
            || request.len & 3 != 0
            || word(&request.bytes, 0) != 1
            || word(&request.bytes, 4) != 1
            || word(&request.bytes, 8) != request.source.node
            || word(&request.bytes, 12) != request.source.port
            || word(&request.bytes, 16) != 0
            || word(&request.bytes, 24) != request.destination.node
            || word(&request.bytes, 28) != request.destination.port
        {
            return Err(wlfw::Error::InvalidQrtr.into());
        }
        let body = word(&request.bytes, 20) as usize;
        if body > request.len - 32 || align(32 + body, 4) != request.len {
            return Err(wlfw::Error::InvalidQrtr.into());
        }
        let message = Message::decode(&request.bytes[32..32 + body])?;
        if message.kind != qmi::Kind::Request
            || message.transaction != request.transaction_id
            || message.message_id != request.message_id
            || request.message_id != message.message_id
            || request.bytes[32 + body..request.len]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(wlfw::Error::InvalidQrtr.into());
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_flow = self.remember_and_reserve_reference(request.source, request.destination, false)?;
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        self.remember_qrtr_flow(request.source, request.destination)?;
        let tx = &mut self.transmit[slot];
        tx.bytes[..request.len].copy_from_slice(&request.bytes[..request.len]);
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { put(&mut tx.bytes, 16, u32::from(reference_flow.confirm_rx())); tx.reference_flow = Some(reference_flow); }
        tx.len = request.len;
        tx.sent = 0;
        tx.kind = TxKind::WlfwRequest;
        tx.remote_slot = None;
        tx.glink_done = false;
        tx.qrtr_done = true;
        Ok(())
    }

    fn stage_ipa_request(
        &mut self,
        slot: usize,
        request: ipa_client::OutboundRequest,
    ) -> Result<(), Error> {
        if slot >= self.transmit.len()
            || self.transmit[slot].len != 0
            || request.generation != self.generation
            || request.len < ipa_client::QRTR_HEADER_BYTES
            || request.len > ipa_client::MAX_DATAGRAM_BYTES
            || request.len & 3 != 0
            || word(&request.bytes, 0) != 1
            || word(&request.bytes, 4) != 1
            || word(&request.bytes, 8) != request.source.node
            || word(&request.bytes, 12) != request.source.port
            || request.source.node != self.config.local_node
            || word(&request.bytes, 16) != 0
            || word(&request.bytes, 24) != request.destination.node
            || word(&request.bytes, 28) != request.destination.port
            || request.destination.node != self.config.peer_node
            || request.message_id != ipa_wire::INIT_MODEM_DRIVER_MESSAGE_ID
        {
            return Err(ipa_client::Error::InvalidQrtr.into());
        }
        let body = word(&request.bytes, 20) as usize;
        if body > ipa_client::MAX_QMI_BYTES
            || body > request.len - ipa_client::QRTR_HEADER_BYTES
            || align(ipa_client::QRTR_HEADER_BYTES + body, 4) != request.len
            || request.bytes[ipa_client::QRTR_HEADER_BYTES + body..request.len]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(ipa_client::Error::InvalidQrtr.into());
        }
        let message = Message::decode(
            &request.bytes[ipa_client::QRTR_HEADER_BYTES..ipa_client::QRTR_HEADER_BYTES + body],
        )?;
        if message.kind != qmi::Kind::Request
            || message.transaction != request.transaction_id
            || message.message_id != ipa_wire::INIT_MODEM_DRIVER_MESSAGE_ID
        {
            return Err(ipa_client::Error::InvalidQrtr.into());
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_flow = self.remember_and_reserve_reference(request.source, request.destination, false)?;
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        self.remember_qrtr_flow(request.source, request.destination)?;
        let tx = &mut self.transmit[slot];
        tx.bytes[..request.len].copy_from_slice(&request.bytes[..request.len]);
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { put(&mut tx.bytes, 16, u32::from(reference_flow.confirm_rx())); tx.reference_flow = Some(reference_flow); }
        tx.len = request.len;
        tx.sent = 0;
        tx.kind = TxKind::IpaRequest;
        tx.remote_slot = None;
        tx.glink_done = false;
        tx.qrtr_done = true;
        Ok(())
    }

    fn stage_servreg_notif_request(
        &mut self,
        slot: usize,
        request: servreg_notif::OutboundQuery,
    ) -> Result<(), Error> {
        if slot >= self.transmit.len()
            || self.transmit[slot].len != 0
            || request.generation != self.generation
            || request.len > servreg_notif::MAX_DATAGRAM_BYTES
            || request.len & 3 != 0
            || word(&request.bytes, 0) != 1
            || word(&request.bytes, 4) != 1
            || word(&request.bytes, 8) != request.source.node
            || word(&request.bytes, 12) != request.source.port
            || word(&request.bytes, 16) != 0
            || word(&request.bytes, 24) != request.destination.node
            || word(&request.bytes, 28) != request.destination.port
        {
            return Err(servreg_notif::Error::InvalidDatagram.into());
        }
        let body = word(&request.bytes, 20) as usize;
        let expected_body = match request.message_id {
            crate::qcom_servreg_notif::QUERY_STATE_MESSAGE_ID => {
                crate::qcom_servreg_notif::WLAN_PD_QUERY_BYTES
            }
            crate::qcom_servreg_notif::REGISTER_LISTENER_MESSAGE_ID => {
                crate::qcom_servreg_notif::WLAN_PD_REGISTER_LISTENER_BYTES
            }
            crate::qcom_servreg_notif::SET_ACK_MESSAGE_ID => {
                crate::qcom_servreg_notif::WLAN_PD_SET_ACK_REQUEST_BYTES
            }
            crate::qcom_servreg_notif::RESTART_PD_MESSAGE_ID => {
                crate::qcom_servreg_notif::WLAN_PD_RESTART_PD_BYTES
            }
            _ => return Err(servreg_notif::Error::InvalidDatagram.into()),
        };
        if body != expected_body
            || body > request.len - 32
            || align(32 + body, 4) != request.len
            || request.bytes[32 + body..request.len]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(servreg_notif::Error::InvalidDatagram.into());
        }
        let message = Message::decode(&request.bytes[32..32 + body])?;
        let exact_fields = match request.message_id {
            crate::qcom_servreg_notif::REGISTER_LISTENER_MESSAGE_ID => {
                message.check_fields(&[0x01, 0x02]).is_ok()
                    && message.tlv_count() == 2
                    && message.required(0x01)? == [1]
                    && message.required(0x02)? == crate::qcom_servreg_notif::WLAN_PD_DOMAIN_NAME
            }
            crate::qcom_servreg_notif::SET_ACK_MESSAGE_ID => {
                message.check_fields(&[0x01, 0x02]).is_ok()
                    && message.tlv_count() == 2
                    && message.required(0x01)? == crate::qcom_servreg_notif::WLAN_PD_DOMAIN_NAME
                    && message.required(0x02)?.len() == 2
            }
            _ => {
                message.check_fields(&[0x01]).is_ok()
                    && message.tlv_count() == 1
                    && message.required(0x01)? == crate::qcom_servreg_notif::WLAN_PD_DOMAIN_NAME
            }
        };
        if message.kind != qmi::Kind::Request
            || message.transaction != request.transaction
            || message.message_id != request.message_id
            || !exact_fields
        {
            return Err(servreg_notif::Error::InvalidDatagram.into());
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_flow = self.remember_and_reserve_reference(request.source, request.destination, false)?;
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        self.remember_qrtr_flow(request.source, request.destination)?;
        let tx = &mut self.transmit[slot];
        tx.bytes[..request.len].copy_from_slice(&request.bytes[..request.len]);
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        { put(&mut tx.bytes, 16, u32::from(reference_flow.confirm_rx())); tx.reference_flow = Some(reference_flow); }
        tx.len = request.len;
        tx.sent = 0;
        tx.kind = TxKind::ServregNotifRequest;
        tx.remote_slot = None;
        tx.glink_done = false;
        tx.qrtr_done = true;
        Ok(())
    }

    fn receive_credit(&mut self, receive: usize, len: usize) -> Result<Event, Error> {
        let bytes = &self.receive[receive].bytes;
        let read = |at: usize| {
            if at.checked_add(4).is_some_and(|end| end <= len) {
                word(bytes, at)
            } else {
                u32::MAX
            }
        };
        let words = core::array::from_fn(|index| read(index * 4));
        let source = Address {
            node: words[2],
            port: words[3],
        };
        let destination = Address {
            node: words[6],
            port: words[7],
        };
        let credit_target = Address {
            node: words[9],
            port: words[10],
        };
        let known_flow = self
            .qrtr_flows
            .iter()
            .flatten()
            .any(|flow| flow.destination == credit_target);
        let mut reply_state = 0;
        for tx in &self.transmit {
            if let Some((route_source, route_destination)) = tx.kind.qrtr_addresses() {
                if tx.len != 0
                    && route_destination == source
                    && route_source == destination
                    && route_destination == credit_target
                {
                    reply_state |= 1;
                    if tx.sent == tx.len && !tx.qrtr_done {
                        reply_state |= 2;
                    }
                    if tx.sent != tx.len && !tx.qrtr_done {
                        reply_state |= 4;
                    }
                }
            }
        }

        // Preserve exact failure detail while keeping the wire checks strict:
        // 0 length != 52; 1 version != 1; 2 type != RESUME_TX; 3 confirm != 0;
        // 4 body length != 20; 5 source node; 6 destination node; 7 command;
        // 8-9 are the flow key; an unknown key is a valid no-op. 10 padding;
        // 11 reserved; 12 a matching reply is not fully published yet.
        let mut bad_fields = 0;
        if len != 52 {
            bad_fields |= 1 << 0;
        }
        if words[0] != 1 {
            bad_fields |= 1 << 1;
        }
        if words[1] != 7 {
            bad_fields |= 1 << 2;
        }
        if words[4] != 0 {
            bad_fields |= 1 << 3;
        }
        if words[5] != 20 {
            bad_fields |= 1 << 4;
        }
        if words[2] != self.config.peer_node {
            bad_fields |= 1 << 5;
        }
        // Linux QRTR dispatches RESUME_TX by destination node; its receive
        // path does not route this control packet by destination port.
        if words[6] != self.config.local_node {
            bad_fields |= 1 << 6;
        }
        if words[8] != 7 {
            bad_fields |= 1 << 7;
        }
        if words[11] != 0 || words[12] != 0 {
            bad_fields |= 1 << 10;
        }
        if reply_state & 4 != 0 {
            bad_fields |= 1 << 12;
        }
        if bad_fields != 0 {
            self.last_credit_diagnostic = Some(CreditDiagnostic {
                packet_len: len as u32,
                words,
                bad_fields,
                known_flow,
                reply_state,
            });
            return Err(Error::InvalidCredit);
        }
        self.last_credit_diagnostic = None;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let _applied = {
            // A control-port RESUME_TX names its peer flow in the body. Use
            // this form only for the validated peer and both control ports.
            let control_resume = cfg!(feature = "sunfish-qrtr-control-port-resume")
                && source.port == control::CONTROL_PORT
                && destination.port == control::CONTROL_PORT
                && credit_target.node == source.node;
            if source == credit_target || control_resume {
                let key = crate::qcom_qrtr_flow::Destination {
                    node: credit_target.node,
                    port: credit_target.port,
                };
                self.rmtfs_reference_flows.resume(key).map_err(|_| Error::InvalidCredit)?
            } else {
                false
            }
        };
        #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
        if let Some(trace) = self.elf_exchange.as_mut()
            && credit_target == trace.destination
        {
            // Keep observations of the body key independent of flow resets.
            let phase = if trace.completed { 2 } else { usize::from(trace.credit.published) };
            let count = &mut trace.credit.resumes[phase];
            *count = count.saturating_add(1);
            trace.credit.applied = trace.credit.applied.saturating_add(u32::from(_applied));
            if trace.credit.first_resume.is_none() {
                trace.credit.first_resume = Some(ElfResumeObservation {
                    source_port: source.port,
                    local_port: destination.port,
                    observed_at: self.intent_ack_clock.map(|clock| clock()),
                });
            }
        }
        #[cfg(feature = "sunfish-tftp-oack-resources")]
        for (_, resources) in self.server_check_transfers.iter().zip(&mut self.server_check_resources)
            .filter_map(|(trace, resources)| trace.as_ref().map(|trace| (trace, resources)))
            .filter(|(trace, _)| trace.peer == credit_target && trace.oack_at.is_some()
                && trace.first_data_at.is_none() && trace.first_close_at.is_none())
        {
            resources.peer_resumes = resources.peer_resumes.saturating_add(1);
            #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
            if _applied {
                resources.applied_resumes = resources.applied_resumes.saturating_add(1);
            }
            if resources.peer_resume_at.is_none() {
                resources.peer_resume_at = self.intent_ack_clock.map(|clock| clock());
            }
        }
        // QRTR RESUME_TX resets the sender's per-destination flow counter; it
        // is not a one-for-one acknowledgement for each data packet. Linux
        // silently ignores a well-formed resume for a flow it no longer
        // tracks (for example, a delayed or duplicate resume after reset), so
        // an untracked but structurally valid credit is also a no-op here.
        // Retire a reply only when this packet matches a published reply whose
        // header route and body flow key identify the same destination and
        // confirm_rx bit requested the resume. Any unmatched resume leaves
        // transmit state untouched, while its GLINK receive intent is reused.
        if let Some(index) = self.transmit.iter().position(|tx| match tx.kind {
            _ if tx.kind.qrtr_addresses().is_some() => {
                let (route_source, route_destination) = tx.kind.qrtr_addresses().unwrap();
                tx.len != 0
                    && tx.sent == tx.len
                    && !tx.qrtr_done
                    && route_destination == source
                    && route_source == destination
                    && route_destination == credit_target
            }
            _ => false,
        }) {
            let completion = match self.transmit[index].kind {
                TxKind::Reply { receive, .. } => Event::ReplyQrtrResume(RequestHandle {
                    owner: self.owner,
                    generation: self.generation,
                    intent: self.receive[receive].id,
                }),
                _ => Event::CreditReceived,
            };
            self.transmit[index].qrtr_done = true;
            self.retire_transmit(index, #[cfg(feature="sunfish-elf-tx-slots")] 2)?;
            self.receive[receive].state = RxState::Control;
            self.receive[receive].done_pending = true;
            return Ok(completion);
        } else if self.transmit.iter().any(|tx| match tx.kind {
            _ if tx.kind.qrtr_addresses().is_some() => {
                let (route_source, route_destination) = tx.kind.qrtr_addresses().unwrap();
                tx.len != 0
                    && route_destination == source
                    && route_source == destination
                    && route_destination == credit_target
                    && tx.sent != tx.len
                    && !tx.qrtr_done
            }
            _ => false,
        }) {
            // A matching reply exists but is not fully published. The peer
            // cannot have acknowledged it yet, so preserve the strict ordering
            // check instead of treating this as a stale flow-control packet.
            return Err(Error::InvalidCredit);
        }
        self.receive[receive].state = RxState::Control;
        self.receive[receive].done_pending = true;
        Ok(Event::CreditReceived)
    }

    fn remember_qrtr_flow(&mut self, source: Address, destination: Address) -> Result<(), Error> {
        self.remember_qrtr_flow_with_limit(source, destination, TX_SLOTS)
    }

    fn remember_qrtr_flow_with_limit(&mut self, source: Address, destination: Address, limit: usize) -> Result<(), Error> {
        if self.retired_rmtfs_peers.contains(&Some(destination)) {
            return Err(Error::FlowBusy);
        }
        if self
            .qrtr_flows
            .iter()
            .flatten()
            .any(|flow| flow.source == source && flow.destination == destination)
        {
            return Ok(());
        }
        let slot = self
            .qrtr_flows[..limit]
            .iter_mut()
            .find(|flow| flow.is_none())
            .ok_or(Error::NoCapacity)?;
        *slot = Some(QrtrFlow {
            source,
            destination,
        });
        Ok(())
    }

    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    fn reserve_reference_flow(&mut self, destination: Address, forced: bool)
        -> Result<crate::qcom_qrtr_flow::Reservation, Error>
    {
        use crate::qcom_qrtr_flow::{Destination, Error as FlowError};
        let key = Destination { node: destination.node, port: destination.port };
        self.rmtfs_reference_flows.reserve_with_legacy_confirmation(
            key, forced,
        ).map_err(|error| match error {
            FlowError::Blocked | FlowError::Closed => Error::FlowBusy,
            FlowError::Capacity => Error::NoCapacity,
            _ => Error::InvalidReply,
        })
    }

    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    fn remember_and_reserve_reference(&mut self, source: Address, destination: Address, forced: bool)
        -> Result<crate::qcom_qrtr_flow::Reservation, Error>
    {
        self.remember_qrtr_flow(source, destination)?;
        self.reserve_remembered_reference(source, destination, forced)
    }

    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    fn reserve_remembered_reference(&mut self, source: Address, destination: Address, forced: bool)
        -> Result<crate::qcom_qrtr_flow::Reservation, Error>
    {
        match self.reserve_reference_flow(destination, forced) {
            Ok(token) => Ok(token),
            Err(error) => {
                self.forget_qrtr_flow_if_idle(source, destination);
                Err(error)
            }
        }
    }

    fn forget_qrtr_flow_if_idle(&mut self, source: Address, destination: Address) {
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        if self.transmit.iter().any(|tx| tx.len != 0 && tx.reference_flow.as_ref().is_some_and(|token|
            token.destination() == crate::qcom_qrtr_flow::Destination { node: destination.node, port: destination.port })) {
            return;
        }
        if self
            .transmit
            .iter()
            .any(|tx| tx.len != 0 && tx.kind.qrtr_addresses() == Some((source, destination)))
        {
            return;
        }
        if let Some(flow) = self.qrtr_flows.iter_mut().find(|flow| {
            flow.is_some_and(|flow| flow.source == source && flow.destination == destination)
        }) {
            *flow = None;
        }
    }

    fn receive_done(&mut self, id: u32, reuse: bool) -> Result<Event, Error> {
        // Linux GLINK encodes reuse in RX_DONE_W_REUSE itself; it is not a
        // VERSION/FEATURE negotiation bit. Still require the exact outstanding
        // peer intent and a fully published transmit before accepting it.
        let Some(remote) = self
            .remote
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.id == id && entry.busy))
        else {
            self.record_completion_error(id, reuse, CompletionFailure::NoBusyRemoteIntent);
            return Err(Error::InvalidCompletion);
        };
        let Some(index) = self.transmit.iter().position(|tx| {
            tx.len != 0 && tx.remote_slot == Some(remote) && tx.sent == tx.len && !tx.glink_done
        }) else {
            self.record_completion_error(id, reuse, CompletionFailure::NoFullyPublishedTransmit);
            return Err(Error::InvalidCompletion);
        };
        #[cfg(feature="sunfish-elf-window-trace")]
        let elf_window_route=match self.transmit[index].kind {TxKind::Reply{route,..} if route.service==Service::ServiceRegistry=>Some(route),_=>None};
        // Clear this link before a reusable remote slot can be reserved again.
        let completion = match self.transmit[index].kind {
            TxKind::Reply { receive, .. } => Event::ReplyRxDone(RequestHandle {
                owner: self.owner,
                generation: self.generation,
                intent: self.receive[receive].id,
            }),
            _ => Event::TransmitDone,
        };
        #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
        if matches!(self.transmit[index].kind, TxKind::TftpReply { .. })
            && let Some(intent) = self.transmit[index].server_check_wrq
        {
            let clock = self.intent_ack_clock;
            if let Some(trace) = self.server_check_trace_for_intent(intent) {
                trace.oack_done_at = clock.map(|clock| clock());
            }
        }
        #[cfg(feature = "sunfish-native-tftp-server-check")]
        if matches!(self.transmit[index].kind, TxKind::TftpReply { .. }) {
            self.tftp_tx_progress[2] = self.tftp_tx_progress[2].saturating_add(1);
        }
        #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
        if self.transmit[index].missing_reply_observed {
            if let Some(trace) = self.missing_tftp_reply.as_mut() { trace.flags |= 16; }
            self.transmit[index].missing_reply_observed = false;
        }
        #[cfg(feature = "sunfish-tftp-final-ack-observer")]
        if self.transmit[index].final_ack_observed {
            if let Some(trace) = self.final_tftp_ack.as_mut() {
                trace.flags |= 16;
                #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
                { trace.done_at = self.intent_ack_clock.map(|clock| clock()); }
            }
            self.transmit[index].final_ack_observed = false;
        }
        #[cfg(feature = "sunfish-tftp-endpoint-observer")]
        if self.first_tftp_slot == Some(index) {
            if let Some(trace) = self.first_tftp_endpoint.as_mut() { trace.flags |= 8; }
            self.first_tftp_slot = None;
        }
        #[cfg(feature = "sunfish-tftp-mcfg-trace")]
        if self.mcfg_oack_slot == Some(index) {
            if let Some(trace) = self.mcfg_oack.as_mut() { trace.flags |= 16; }
            self.mcfg_oack_slot = None;
        }
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        self.observe_post_mcfg_done(index,id);
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
        self.observe_startup_done(index,id);
/* STARTUP_PREREQ_END */
        self.transmit[index].remote_slot = None;
        self.transmit[index].glink_done = true;
        #[cfg(feature = "sunfish-tftp-oack-reserve")]
        if index == TX_SLOTS && let Some(oack) = self.reserved_oack.as_mut() {
            oack.done_at = self.intent_ack_clock.map(|clock| clock());
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        if let Some(reservation) = self.transmit[index].reference_flow.as_ref() {
            if self.rmtfs_reference_flows.finish_after_glink(reservation).is_err() {
                self.quarantined = true;
                return Err(Error::InvalidCompletion);
            }
        }
        if reuse {
            self.remote[remote].as_mut().unwrap().busy = false;
        } else {
            self.remote[remote] = None;
        }
        self.retire_transmit(index, #[cfg(feature="sunfish-elf-tx-slots")] 1)?;
        #[cfg(feature="sunfish-elf-window-trace")]
        if let Some(route)=elf_window_route {self.observe_elf_window_receipt(completion,id,route);}
        #[cfg(feature="sunfish-elf-first-slot-turn")]
        if let (Event::ReplyRxDone(h),Some(route))=(completion,elf_window_route) {self.elf_slot_receipt=Some((h,route));}
        #[cfg(feature = "sunfish-registry-handoff-turn")]
        if let Some(prior) = self.registry_window.predecessor.as_mut()
            && completion == Event::ReplyRxDone(prior.request)
        { prior.done_at = self.intent_ack_clock.map(|clock| clock()); }
        #[cfg(feature = "sunfish-registry-window-trace")]
        if let Some(pair) = self.registry_window.target.as_mut()
            && completion == Event::ReplyRxDone(pair.older) && !pair.older_completed
        {
            pair.older_completed = true;
            pair.older_done_at = self.intent_ack_clock.map(|clock| clock());
        }
        #[cfg(feature = "sunfish-wlan-first-wrq-window")]
        self.first_wrq_window.complete(index, id, self.intent_ack_clock);
        #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
        let clock = self.intent_ack_clock;
        #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
        for write in self.server_check_traces_mut() {
            for reply in [&mut write.oack, &mut write.ack] {
                if reply.slot == Some(index) && reply.phase == 3 {
                    reply.phase = 4;
                    reply.done_at = clock.map(|clock| clock());
                    reply.slot = None;
                }
            }
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
        if let Some(repeat) = self.first_server_check.as_mut().and_then(|write| write.repeat.as_mut())
            && repeat.oack.slot == Some(index) && repeat.oack.phase == 3
        {
            repeat.oack.phase = 4;
            repeat.oack.done_at = clock.map(|clock| clock());
            repeat.oack.slot = None;
        }
        #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
        if let Some(credit) = self.elf_host_credit.as_mut()
            && credit.resume_slot == Some(index)
        {
            credit.completed = true;
            credit.resume_slot = None;
        }
        #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
        if let Some(trace) = self.elf_exchange.as_mut()
            && completion == Event::ReplyRxDone(trace.request)
        {
            trace.completed = true;
        }
        #[cfg(feature="sunfish-post-elf-mcfg-trace")]
        self.observe_post_mcfg_elf_done(completion);
        Ok(completion)
    }

    fn record_completion_error(&mut self, id: u32, reuse: bool, failure: CompletionFailure) {
        self.last_completion_error = Some(CompletionDiagnostic {
            intent_id: id,
            reuse,
            failure,
            busy_remote_intents: self
                .remote
                .iter()
                .filter(|entry| entry.is_some_and(|entry| entry.busy))
                .count() as u8,
            incomplete_transmits: self
                .transmit
                .iter()
                .filter(|tx| tx.len != 0 && tx.sent < tx.len)
                .count() as u8,
            completed_transmits: self
                .transmit
                .iter()
                .filter(|tx| tx.len != 0 && tx.sent == tx.len && !tx.glink_done)
                .count() as u8,
        });
    }

    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
    pub(crate) fn published_oack_request(&self, intent: u32) -> Option<TftpHandle> {
        let remote = self.remote.iter().position(|slot|
            slot.is_some_and(|intent_slot| intent_slot.id == intent && intent_slot.busy))?;
        self.transmit.iter().find(|tx| tx.len != 0 && tx.sent == tx.len
            && tx.remote_slot == Some(remote) && !tx.glink_done)?.traced_oack_request
    }

    fn retire_transmit(&mut self, index: usize, #[cfg(feature="sunfish-elf-tx-slots")] elf_cause:u8) -> Result<(), Error> {
        let tx = &self.transmit[index];
        if !tx.glink_done || !tx.qrtr_done {
            return Ok(());
        }
        let retired_kind = tx.kind;
        let retired_flow = retired_kind.qrtr_addresses();
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let retired_flow = retired_flow.or_else(|| {
            tx.reference_flow.as_ref().filter(|_| tx.len >= 32).map(|_| (
                Address { node: word(&tx.bytes, 8), port: word(&tx.bytes, 12) },
                Address { node: word(&tx.bytes, 24), port: word(&tx.bytes, 28) },
            ))
        });
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if let TxKind::LinuxIpa { receive, id, .. } = retired_kind {
            self.retire_linux_ipa(receive, id)?;
        }
        if matches!(retired_kind, TxKind::IpaIndication { .. })
            && self.completed_ipa_indication.is_some()
        {
            return Err(Error::FlowBusy);
        }
        if matches!(
            retired_kind,
            TxKind::Reply {
                ipa_token: Some(_),
                ..
            }
        ) && self.completed_ipa_service_response.is_some()
        {
            return Err(Error::FlowBusy);
        }
        if let TxKind::Reply {
            receive, ipa_token, ..
        } = retired_kind
        {
            let ticket = self.receive[receive]
                .ticket
                .take()
                .ok_or(Error::InvalidHandle)?;
            self.router.complete(ticket)?;
            self.receive[receive] = Receive::empty();
            if ipa_token.is_some() {
                self.completed_ipa_service_response = ipa_token;
            }
        }
        if let TxKind::IpaIndication { token, .. } = retired_kind {
            self.completed_ipa_indication = Some(token);
        }
        self.transmit[index] = Transmit::empty();
        #[cfg(feature="sunfish-elf-tx-slots")]
        self.observe_elf_tx_freed(index,elf_cause);
        if let Some((source, destination)) = retired_flow {
            self.forget_qrtr_flow_if_idle(source, destination);
        }
        Ok(())
    }

    fn request_index(&self, handle: &RequestHandle) -> Result<usize, Error> {
        self.check(handle.generation)?;
        if handle.owner != self.owner {
            return Err(Error::InvalidHandle);
        }
        self.receive
            .iter()
            .position(|rx| rx.id == handle.intent && rx.state == RxState::Request)
            .ok_or(Error::InvalidHandle)
    }

    fn tftp_index(&self, handle: &TftpHandle) -> Result<usize, Error> {
        self.check(handle.generation)?;
        if handle.owner != self.owner || !self.tftp_server {
            return Err(Error::InvalidHandle);
        }
        self.receive
            .iter()
            .position(|rx| rx.tftp_datagram_id() == handle.intent && rx.state == RxState::Tftp)
            .ok_or(Error::InvalidHandle)
    }

    /// Borrow one exact QRTR DATA payload and its route from the held receive
    /// slot. The borrow cannot outlive the immutable session access.
    pub fn tftp_datagram(&self, handle: &TftpHandle) -> Result<(TftpReplyRoute, &[u8]), Error> {
        let index = self.tftp_index(handle)?;
        let rx = &self.receive[index];
        let len = 32 + word(&rx.bytes, 20) as usize;
        let route = TftpReplyRoute {
            source: Address {
                node: word(&rx.bytes, 24),
                port: word(&rx.bytes, 28),
            },
            destination: Address {
                node: word(&rx.bytes, 8),
                port: word(&rx.bytes, 12),
            },
        };
        Ok((route, &rx.bytes[32..len]))
    }

    /// Release a held TFTP datagram only after the caller has completed its
    /// bounded dispatch and retained any response needed for later retry.
    pub fn finish_tftp_request(&mut self, handle: &TftpHandle) -> Result<(), Error> {
        let index = self.tftp_index(handle)?;
        self.receive[index].state = RxState::Control;
        self.receive[index].done_pending = true;
        Ok(())
    }

    /// The borrowed request cannot outlive the immutable session borrow.
    pub fn request(&self, handle: &RequestHandle) -> Result<(Context, Request<'_>), Error> {
        let index = self.request_index(handle)?;
        let rx = &self.receive[index];
        let context = rx.context.ok_or(Error::InvalidHandle)?;
        let len = 32 + word(&rx.bytes, 20) as usize;
        let message = Message::decode(&rx.bytes[32..len])?;
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa.is_some() && context.service == Service::IpaServer {
            crate::sunfish_ipa_linux_qmi::validate_server_request(&message)?;
            return Ok((
                context,
                Request::IpaServer {
                    message_id: context.message_id,
                },
            ));
        }
        Ok((context, services::decode(context.service, &message)?))
    }

    /// Borrow the original QMI request bytes and validated route for a
    /// source-specific server core. The receive slot remains held until its
    /// response has completed on both transports.
    pub fn request_qmi(&self, handle: &RequestHandle) -> Result<(Context, &[u8]), Error> {
        let index = self.request_index(handle)?;
        let rx = &self.receive[index];
        let context = rx.context.ok_or(Error::InvalidHandle)?;
        let len = 32 + word(&rx.bytes, 20) as usize;
        Ok((context, &rx.bytes[32..len]))
    }

    /// Copy an action result into a bounded reply. This preserves the original
    /// router ticket until BOTH peer GLINK completion and QRTR RESUME_TX arrive.
    /// One reply per destination flow is allowed; every reply requests a credit.
    pub fn queue_reply(&mut self, handle: &RequestHandle, reply: Reply<'_>) -> Result<(), Error> {
        self.queue_reply_with_ipa_token(handle, reply, None,
            #[cfg(feature="sunfish-elf-third-reply")] false)
    }

    /// Queue the exact success response prepared by the bounded AP IPA
    /// service. Its state token is surfaced only when GLINK and QRTR both
    /// acknowledge the response.
    pub fn queue_ipa_service_reply(
        &mut self,
        generation: u64,
        handle: &RequestHandle,
        response: &ipa_service::PreparedResponse,
    ) -> Result<(), Error> {
        self.check(generation)?;
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa.is_some() {
            return Err(Error::InvalidReply);
        }
        if self.completed_ipa_service_response.is_some()
            || self.transmit.iter().any(|tx| {
                tx.len != 0
                    && matches!(
                        tx.kind,
                        TxKind::Reply {
                            ipa_token: Some(_),
                            ..
                        }
                    )
            })
        {
            return Err(Error::FlowBusy);
        }
        let (context, request_bytes) = self.request_qmi(handle)?;
        if context.generation != generation
            || context.service != Service::IpaServer
            || context.peer != response.peer()
            || context.peer.node != self.config.peer_node
            || self.control.ipa_modem_endpoint() != Some(context.peer)
        {
            return Err(Error::InvalidReply);
        }
        let request = Message::decode(request_bytes)?;
        let reply = Message::decode(response.bytes())?;
        if request.kind != qmi::Kind::Request
            || request.message_id != context.message_id
            || request.transaction != context.transaction
            || reply.as_bytes().len() != ipa_service::QMI_MESSAGE_BYTES
            || reply.kind != qmi::Kind::Response
            || reply.transaction != request.transaction
            || reply.message_id != request.message_id
            || reply.tlv_count() != 1
            || reply.required(ipa_service::QMI_COMMON_RESPONSE_TLV)? != [0, 0, 0, 0]
            || response.token() == 0
        {
            return Err(Error::InvalidReply);
        }
        self.queue_reply_with_ipa_token(
            handle,
            Reply::Success { optional_tlvs: &[] },
            Some(response.token()),
            #[cfg(feature="sunfish-elf-third-reply")] false,
        )
    }

    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
    pub fn registry_queue_pressure(&self, handle: &RequestHandle) -> Option<RegistryQueuePressure> {
        self.registry_pressure.filter(|(request, _)| request == handle).map(|(_, pressure)| pressure)
    }

    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
    fn record_registry_pressure(&mut self, handle: &RequestHandle, route: ReplyRoute, reason: u8) {
        if route.service != Service::ServiceRegistry {
            return;
        }
        let mut pressure = RegistryQueuePressure {
            reason, used_slots: 0, unpublished_slots: 0, glink_wait_slots: 0,
            qrtr_wait_slots: 0, kind_mask: 0, peer_kind: 0, peer_flags: 0,
            #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
            peer_publication: None,
            #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
            peer_request: None,
            #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
            pending_ack_age: None,
        };
        for tx in self.transmit.iter().filter(|tx| tx.len != 0) {
            let kind = tx.kind.diagnostic_kind();
            let published = tx.sent == tx.len;
            pressure.used_slots += 1;
            pressure.unpublished_slots += u8::from(!published);
            pressure.glink_wait_slots += u8::from(published && !tx.glink_done);
            pressure.qrtr_wait_slots += u8::from(tx.glink_done && !tx.qrtr_done);
            pressure.kind_mask |= 1 << kind;
            if tx.kind.qrtr_addresses().is_some_and(|(_, peer)| peer == route.destination) {
                pressure.peer_kind = kind;
                #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
                if kind == 4 {
                    pressure.peer_publication = Some(tx.registry_publication);
                    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
                    if !published {
                        pressure.pending_ack_age = self.intent_ack_age();
                    }
                }
                #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
                if let TxKind::Reply { receive, .. } = tx.kind {
                    pressure.peer_request = Some(RequestHandle {
                        owner: self.owner,
                        generation: self.generation,
                        intent: self.receive[receive].id,
                    });
                }
                pressure.peer_flags = u8::from(tx.sent != 0) | (u8::from(published) << 1)
                    | (u8::from(tx.glink_done) << 2) | (u8::from(tx.qrtr_done) << 3);
            }
        }
        #[cfg(feature="sunfish-elf-tx-slots")]
        if reason==2 {self.observe_elf_tx_full(*handle);}
        self.registry_pressure = Some((*handle, pressure));
        #[cfg(feature="sunfish-elf-window-trace")]
        self.observe_elf_window_admission(*handle,route,reason);
    }

    /// Pure live ownership metadata, never a display history or timing guard.
    /// The older slot stays owned; normal queue_reply still checks credits.
    #[cfg(feature = "sunfish-registry-handoff-turn")]
    pub(crate) fn registry_handoff_pair(&self, older: &RequestHandle, waiting: &RequestHandle)
        -> Option<(u16, u16, Address, bool)>
    {
        self.check(self.generation).ok()?;
        if older.owner != self.owner || older.generation != self.generation { return None; }
        let pending = self.request_index(waiting).ok()?;
        let route = self.router.reply_route(self.receive[pending].ticket.as_ref()?).ok()?;
        if route.service != Service::ServiceRegistry { return None; }
        let receive = self.receive.iter().position(|rx| rx.id == older.intent && rx.state == RxState::ReplyQueued)?;
        let mut same_peer = self.transmit.iter().filter(|tx| tx.len != 0
            && tx.kind.qrtr_addresses().is_some_and(|(_,peer)| peer == route.destination));
        let tx = same_peer.next()?;
        if same_peer.next().is_some() || tx.glink_done || !tx.qrtr_done || tx.reference_flow.is_none() { return None; }
        let TxKind::Reply { receive: prior, route: prior_route, .. } = tx.kind else { return None; };
        if prior != receive || prior_route.service != Service::ServiceRegistry
            || prior_route.source != route.source { return None; }
        Some((prior_route.transaction, route.transaction, route.destination,
            tx.sent == tx.len && tx.remote_slot.is_some()))
    }

    #[cfg(feature = "sunfish-registry-window-trace")]
    pub fn registry_window_trace(&self) -> RegistryWindowTrace {
        #[cfg(not(feature = "sunfish-registry-predecessor-pressure"))]
        { self.registry_window }
        #[cfg(feature = "sunfish-registry-predecessor-pressure")]
        {
            let mut trace = self.registry_window;
            if let Some(prior) = trace.predecessor.as_mut()
                && !prior.publication_frozen
                && prior.request.owner == self.owner && prior.request.generation == self.generation
                && let Some(receive) = self.receive.iter().position(|rx| rx.id == prior.request.intent && rx.state == RxState::ReplyQueued)
                && let Some(tx) = self.transmit.iter().find(|tx| tx.len != 0
                    && matches!(tx.kind, TxKind::Reply { receive: index, .. } if index == receive))
            { prior.publication = Some(tx.registry_publication); }
            #[cfg(feature = "sunfish-registry-post-reply-trace")]
            if let Some(pair) = trace.target.as_mut()
                && !pair.publication_frozen
                && pair.newer.owner == self.owner && pair.newer.generation == self.generation
                && let Some(receive) = self.receive.iter().position(|rx| rx.id == pair.newer.intent && rx.state == RxState::ReplyQueued)
                && let Some(tx) = self.transmit.iter().find(|tx| tx.len != 0
                    && matches!(tx.kind, TxKind::Reply { receive: index, .. } if index == receive))
            { pair.publication = Some(tx.registry_publication); }
            trace
        }
    }

    #[cfg(feature = "sunfish-registry-post-reply-trace")]
    pub(crate) fn registry_predecessor_completed_by(&self, event: &Event) -> bool {
        self.registry_window.predecessor.is_some_and(|p| p.publication_frozen
            && p.request.owner == self.owner && p.request.generation == self.generation
            && *event == Event::ReplyRxDone(p.request))
    }

    #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
    fn observe_conversion_entry(&mut self, ordinary: bool) -> bool {
        let Some(pair) = self.registry_window.target else { return false; };
        let Some(first) = pair.conversion.first.filter(|t| t.active && t.entry.is_none()) else { return false; };
        let token = self.registry_converted_grant;
        let receive = self.receive.iter().position(|rx| rx.id == pair.newer.intent && rx.state == RxState::ReplyQueued);
        let tx = receive.and_then(|receive| self.transmit.iter().find(|tx| tx.len != 0
            && matches!(tx.kind, TxKind::Reply { receive: index, .. } if index == receive)));
        let flags = u32::from(ordinary)
            | (u32::from(self.intent_request_ack.is_some()) << 1)
            | (u32::from(self.intent_request_ack == Some(true)) << 2)
            | (u32::from(self.intent_ack_backing == Some(first.intent)) << 3)
            | (u32::from(token == Some((first.intent, pair.newer))) << 4)
            | (u32::from(self.receive[..RX_SLOTS].iter().any(|rx| rx.id == first.intent && rx.state == RxState::Advertising)) << 5)
            | (u32::from(receive.is_some_and(|i| self.receive[i].done_pending)) << 6)
            | (u32::from(self.registry_turn_used) << 7)
            | (u32::from(tx.is_some_and(|t| t.registry_grant_turn_used)) << 8)
            | (u32::from(tx.is_some()) << 9)
            | (u32::from(tx.is_some_and(|t| t.sent == t.len)) << 10)
            | (u32::from(self.intent_ack_yield) << 16)
            | (u32::from(tx.is_some_and(|t| self.remote_for_transmit(t).is_some())) << 17)
            | (u32::from(tx.is_some_and(|t| {
                #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")] { self.unsent_flow_predecessor(t) }
                #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))] { let _ = t; false }
            })) << 18);
        let entry = RegistryConversionEntry { at: self.intent_ack_clock.map(|now| now()), flags,
            backing: self.intent_ack_backing, token_request: token.map(|(_, r)| r.intent), gate: None };
        self.registry_window.target.as_mut().unwrap().conversion.first.as_mut().unwrap().entry = Some(entry);
        true
    }

    #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
    fn observe_conversion_choice(&mut self, record: &[u8], transaction: Option<u16>) -> bool {
        let Some(first) = self.registry_window.target.as_mut().and_then(|p| p.conversion.first.as_mut())
            .filter(|t| t.active && t.entry.is_some() && t.choice.is_none()) else { return false; };
        first.choice = Some(RegistryConversionChoice { at: self.intent_ack_clock.map(|now| now()),
            command: u16::from_le_bytes([record[0], record[1]]), parameter: word(record, 4), transaction, result: 0 });
        true
    }

    #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
    fn observe_conversion_callback(&mut self, captured: bool) {
        if captured && let Some(choice) = self.registry_window.target.as_mut()
            .and_then(|p| p.conversion.first.as_mut()).and_then(|t| t.choice.as_mut())
        { choice.result = 4; }
    }

    #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
    fn observe_conversion_exit(&mut self, cause: u8) {
        if let Some(first) = self.registry_window.target.as_mut().and_then(|p| p.conversion.first.as_mut())
            .filter(|t| t.active && t.exit.is_none())
        { first.exit = Some((cause, self.intent_ack_clock.map(|now| now()))); first.active = false; }
    }

    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    pub(crate) fn registry_prearm_queue_matches(&self,request:&RequestHandle)->bool {
        self.registry_window.target.is_some_and(|p| p.newer == *request && !p.publication_frozen)
    }

    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    fn registry_prearm_flags(&self, request: RequestHandle) -> u32 {
        let receive = self.receive.iter().position(|rx| rx.id == request.intent && rx.state == RxState::ReplyQueued);
        let tx = receive.and_then(|receive| self.transmit.iter().find(|tx| tx.len != 0
            && matches!(tx.kind, TxKind::Reply { receive: index, .. } if index == receive)));
        u32::from(receive.is_some_and(|i| self.receive[i].done_pending))
            | (u32::from(tx.is_some_and(|t| self.remote_for_transmit(t).is_some())) << 1)
            | (u32::from(self.intent_request_ack.is_some()) << 2)
            | (u32::from(self.intent_request_ack == Some(true)) << 3)
            | (u32::from(self.intent_ack_backing.is_some()) << 4)
            | (u32::from(self.next_transmit_in_order([1]).is_some()) << 5)
            | (u32::from(self.registry_turn_used) << 6)
            | (u32::from(tx.is_some_and(|t| t.registry_grant_turn_used)) << 7)
            | (u32::from(tx.is_some_and(|t| t.registry_credit_turn_used)) << 8)
            | (u32::from(tx.is_some()) << 9)
            | (u32::from({ #[cfg(feature="sunfish-glink-intent-request-experiment")] { matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. }) } #[cfg(not(feature="sunfish-glink-intent-request-experiment"))] { false } }) << 10)
            | (u32::from(tx.is_some_and(|t| { #[cfg(feature="sunfish-qrtr-rmtfs-flow-experiment")] { self.unsent_flow_predecessor(t) } #[cfg(not(feature="sunfish-qrtr-rmtfs-flow-experiment"))] { let _ = t; false } })) << 11)
            | (u32::from(receive.is_some() && request.owner == self.owner && request.generation == self.generation) << 12)
    }

    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    fn observe_prearm_entry(&mut self) -> bool {
        let Some(pair) = self.registry_window.target.filter(|p| !p.publication_frozen && p.conversion.arms == 0) else { return false; };
        let first = pair.prearm.passes == 0;
        let flags = first.then(|| self.registry_prearm_flags(pair.newer));
        let trace = &mut self.registry_window.target.as_mut().unwrap().prearm;
        trace.passes = trace.passes.saturating_add(1);
        if first { trace.first_pass_at = self.intent_ack_clock.map(|now| now()); trace.first_flags = flags; }
        first
    }

    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    fn observe_prearm_choice(&mut self, record: &[u8], transaction: Option<u16>) -> bool {
        let Some(pair) = self.registry_window.target.as_mut().filter(|p| !p.publication_frozen && p.conversion.arms == 0
            && p.prearm.passes != 0 && p.prearm.first_choice.is_none()) else { return false; };
        pair.prearm.first_choice = Some(RegistryConversionChoice { at: self.intent_ack_clock.map(|now| now()),
            command: u16::from_le_bytes([record[0], record[1]]), parameter: word(record,4), transaction, result: 0 });
        true
    }

    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    fn observe_prearm_result(&mut self, captured: bool, result: u8) {
        if captured && let Some(choice) = self.registry_window.target.as_mut().and_then(|p| p.prearm.first_choice.as_mut())
        { choice.result = result; }
    }

    fn queue_reply_with_ipa_token(
        &mut self,
        handle: &RequestHandle,
        reply: Reply<'_>,
        ipa_token: Option<u64>,
        #[cfg(feature="sunfish-elf-third-reply")] elf_third:bool,
    ) -> Result<(), Error> {
        #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
        { self.registry_pressure = None; }
        let receive = self.request_index(handle)?;
        let route = self.router.reply_route(
            self.receive[receive]
                .ticket
                .as_ref()
                .ok_or(Error::InvalidHandle)?,
        )?;
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if self.linux_ipa.is_some() && route.service == Service::IpaServer {
            return Err(Error::InvalidReply);
        }
        if ipa_token.is_some()
            && (route.service != Service::IpaServer
                || self.completed_ipa_service_response.is_some())
        {
            return Err(Error::InvalidReply);
        }
        #[cfg(feature = "sunfish-registry-window-trace")]
        let mut window_prior = None;
        let mut same_peer = self.transmit.iter().enumerate().filter(|(_, tx)| {
            tx.len != 0
                && tx.kind.qrtr_addresses()
                    .is_some_and(|(_, destination)| destination == route.destination)
        });
        if let Some((_previous_index, _previous)) = same_peer.next() {
            #[cfg(all(feature = "sunfish-wlan-registry-second-reply",not(feature="sunfish-elf-third-reply")))]
            let allow_second = route.service == Service::ServiceRegistry
                && matches!(_previous.kind, TxKind::Reply { route: older, .. }
                    if older.service == Service::ServiceRegistry && older.source == route.source)
                && _previous.sent == _previous.len
                && _previous.remote_slot.is_some()
                && !_previous.glink_done
                && _previous.qrtr_done
                && _previous.reference_flow.is_some()
                && same_peer.next().is_none();
            #[cfg(feature="sunfish-elf-third-reply")]
            let allow_second={
                let eligible=|tx:&Transmit| route.service==Service::ServiceRegistry
                    && matches!(tx.kind,TxKind::Reply{route:old,..}if old.service==Service::ServiceRegistry && old.source==route.source)
                    && tx.sent==tx.len && tx.remote_slot.is_some() && !tx.glink_done
                    && tx.qrtr_done && tx.reference_flow.is_some();
                eligible(_previous) && match same_peer.next(){
                    None=>true,
                    Some((_,second))=>elf_third && eligible(second)
                        && _previous.remote_slot!=second.remote_slot
                        && _previous.reference_flow!=second.reference_flow && same_peer.next().is_none(),
                }
            };
            #[cfg(not(feature = "sunfish-wlan-registry-second-reply"))]
            let allow_second = false;
            if !allow_second {
                #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
                self.record_registry_pressure(handle, route, 1);
                return Err(Error::FlowBusy);
            }
            #[cfg(feature = "sunfish-registry-window-trace")]
            { window_prior = Some(_previous_index); }
        }
        let Some(index) = self.free_tx_slot() else {
            #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
            self.record_registry_pressure(handle, route, 2);
            return Err(Error::NoCapacity);
        };
        let optional = match reply {
            Reply::Success { optional_tlvs } => optional_tlvs,
            Reply::Failure { error: 0 } => return Err(Error::InvalidReply),
            Reply::Failure { .. } => &[],
        };
        // QRTR(32), QMI(7), standard result TLV(7), optional service fields.
        if optional.len() > MAX_DATAGRAM_BYTES - 46 {
            return Err(qmi::Error::TooLarge.into());
        }
        let len = 46 + optional.len();
        let (result, error) = match reply {
            Reply::Success { .. } => (0u16, 0u16),
            Reply::Failure { error } => (1u16, error),
        };
        {
            let tx = &mut self.transmit[index];
            header(
                &mut tx.bytes,
                1,
                route.source,
                route.destination,
                len - 32,
                true,
            );
            tx.bytes[32] = 2;
            tx.bytes[33..35].copy_from_slice(&route.transaction.to_le_bytes());
            tx.bytes[35..37].copy_from_slice(&route.message_id.to_le_bytes());
            tx.bytes[37..39].copy_from_slice(&((len - 39) as u16).to_le_bytes());
            tx.bytes[39..42].copy_from_slice(&[2, 4, 0]);
            tx.bytes[42..44].copy_from_slice(&result.to_le_bytes());
            tx.bytes[44..46].copy_from_slice(&error.to_le_bytes());
            tx.bytes[46..len].copy_from_slice(optional);
            // Reuse the core's length/count/duplicate checks. Service output
            // fields must be optional TLVs; callers cannot replace the result.
            Message::decode(&tx.bytes[32..len])?;
            let mut at = 0;
            while at < optional.len() {
                if optional[at] < 0x10 {
                    return Err(Error::InvalidReply);
                }
                at += 3 + u16::from_le_bytes([optional[at + 1], optional[at + 2]]) as usize;
            }
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let is_rmtfs = route.service == Service::RemoteStorage;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_reply = is_rmtfs || (cfg!(feature = "sunfish-qrtr-servreg-flow-experiment")
            && route.service == Service::ServiceRegistry);
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let rmtfs_slot = if is_rmtfs && !self.rmtfs_flow_destinations.contains(&Some(route.destination)) {
            Some(self.rmtfs_flow_destinations.iter().position(Option::is_none).ok_or(Error::NoCapacity)?)
        } else { None };
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        if !reference_reply {
            if let Err(error) = self.remember_qrtr_flow(route.source, route.destination) {
                #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
                self.record_registry_pressure(handle, route, 3);
                return Err(error);
            }
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let reference_flow = match self.reserve_reference_flow(route.destination, !reference_reply) {
            Ok(token) => token,
            Err(error) => {
                if !reference_reply { self.forget_qrtr_flow_if_idle(route.source, route.destination); }
                #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
                match error {
                    Error::FlowBusy => self.record_registry_pressure(handle, route, 4),
                    Error::NoCapacity => self.record_registry_pressure(handle, route, 5),
                    _ => {}
                }
                return Err(error);
            }
        };
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        if let Some(slot) = rmtfs_slot { self.rmtfs_flow_destinations[slot] = Some(route.destination); }
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        if let Err(error) = self.remember_qrtr_flow(route.source, route.destination) {
            #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
            self.record_registry_pressure(handle, route, 3);
            return Err(error);
        }
        let tx = &mut self.transmit[index];
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        put(&mut tx.bytes, 16, u32::from(reference_flow.confirm_rx()));
        tx.bytes[len..align(len, 4)].fill(0);
        tx.len = align(len, 4);
        tx.kind = TxKind::Reply {
            receive,
            route,
            ipa_token,
        };
        tx.qrtr_done = false;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        {
            // QRTR credit is flow-based; it never releases the GLINK buffer.
            if reference_reply { tx.qrtr_done = true; }
            tx.reference_flow = Some(reference_flow);
        }
        #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
        if route.service == Service::ServiceRegistry {
            tx.registry_publication = RegistryPublication {
                transaction: route.transaction,
                bytes: tx.len as u16,
                ..RegistryPublication::default()
            };
        }
        self.receive[receive].state = RxState::ReplyQueued;
        #[cfg(feature = "sunfish-registry-handoff-turn")]
        if self.registry_window.predecessor.is_none() && route.service == Service::ServiceRegistry
            && route.transaction == 3 && route.destination == (Address { node: 0, port: 9 })
        {
            self.registry_window.predecessor = Some(RegistryPredecessorTrace {
                #[cfg(feature = "sunfish-registry-post-reply-trace")]
                grant: None,
                request: *handle, queued_at: self.intent_ack_clock.map(|clock| clock()),
                published_at: None, done_at: None,
                #[cfg(feature = "sunfish-registry-predecessor-pressure")]
                publication: None,
                #[cfg(feature = "sunfish-registry-predecessor-pressure")]
                publication_frozen: false,
            });
        }
        #[cfg(feature = "sunfish-registry-window-trace")]
        if let Some(prior) = window_prior {
            self.registry_window.admissions = self.registry_window.admissions.saturating_add(1);
            if self.registry_window.target.is_none()
                && route.transaction == 4 && route.destination == (Address { node: 0, port: 9 })
                && let TxKind::Reply { receive: older_receive, route: older_route, .. } = self.transmit[prior].kind
                && older_route.transaction == 3
            {
                #[cfg(feature = "sunfish-registry-prearm-io-trace")]
                let prearm_flags = self.registry_prearm_flags(*handle);
                self.registry_window.target = Some(RegistryWindowPair {
                    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
                    prearm: RegistryPrearmTrace { queued_flags: prearm_flags, ..RegistryPrearmTrace::default() },
                    #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                    conversion: RegistryConversionLifetime::default(),
                    #[cfg(feature = "sunfish-registry-post-reply-trace")]
                    publication: None,
                    #[cfg(feature = "sunfish-registry-post-reply-trace")]
                    publication_frozen: false,
                    older: RequestHandle { owner: self.owner, generation: self.generation, intent: self.receive[older_receive].id },
                    newer: *handle, older_transaction: 3, newer_transaction: 4,
                    peer: route.destination, queued_at: self.intent_ack_clock.map(|clock| clock()),
                    newer_published_at: None, older_done_at: None,
                    published_before_done: None, older_completed: false,
                });
            }
        }
        #[cfg(feature="sunfish-elf-window-trace")]
        self.observe_elf_window_admission(*handle,route,0);
        #[cfg(feature="sunfish-elf-tx-slots")]
        self.observe_elf_tx_admitted(*handle,index);
        Ok(())
    }

    #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
    fn observe_registry_publication(&mut self, choice: PublishChoice, blocked: bool) {
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        let waiting = matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. });
        #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
        let waiting = false;
        for (index, tx) in self.transmit.iter_mut().enumerate() {
            if tx.len == 0 || tx.sent == tx.len
                || !matches!(tx.kind, TxKind::Reply { route, .. } if route.service == Service::ServiceRegistry)
            {
                continue;
            }
            let suitable = tx.remote_slot.is_some() || self.remote.iter().flatten()
                .any(|intent| !intent.busy && intent.capacity as usize >= tx.len);
            let outcome = match choice {
                PublishChoice::Idle => 0,
                PublishChoice::IntentRequest => 1 << 6,
                PublishChoice::Control => 1 << 2,
                PublishChoice::Data(slot) if slot != index => 1 << 3,
                PublishChoice::Data(_) if blocked => 1 << 4,
                PublishChoice::Data(_) => 1 << 5,
            };
            let trace = &mut tx.registry_publication;
            trace.calls = trace.calls.saturating_add(1);
            trace.mask |= outcome | u32::from(!suitable) | (u32::from(waiting) << 1);
        }
        #[cfg(feature="sunfish-elf-writer-trace")]
        self.end_elf_writer_pass(choice,blocked);
    }

    #[cfg(feature = "sunfish-wlan-registry-turn-gates")]
    fn observe_registry_turn_gates(
        &mut self, tftp_ready: bool, candidate: Option<usize>, priority: Option<usize>,
    ) {
        for (index, tx) in self.transmit.iter_mut().enumerate() {
            let TxKind::Reply { receive, route, .. } = tx.kind else { continue };
            if route.service != Service::ServiceRegistry || tx.sent == tx.len {
                continue;
            }
            let gate = if self.registry_turn_used {
                0
            } else if tftp_ready {
                1
            } else if self.intent_request_ack.is_some() {
                2
            } else if self.receive[receive].done_pending {
                3
            } else {
                match candidate {
                    None => 4,
                    Some(slot) if slot != index => 5,
                    Some(_) if priority.is_none() => 6,
                    Some(_) => 7,
                }
            };
            let count = &mut tx.registry_publication.turn_gates[gate];
            *count = count.saturating_add(1);
        }
    }

    #[cfg(feature = "sunfish-registry-grant-eligibility")]
    fn observe_registry_grant_eligibility(
        &mut self, tftp_ready: bool, wrq_return: bool,
        candidate: Option<usize>, priority: Option<usize>,
        #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")] first_entry: bool,
        #[cfg(feature = "sunfish-registry-prearm-io-trace")] prearm_entry: bool,
    ) {
        for (index, tx) in self.transmit.iter_mut().enumerate() {
            let TxKind::Reply { receive, route, .. } = tx.kind else { continue };
            if route.service != Service::ServiceRegistry || tx.sent == tx.len { continue; }
            // Capture the actual conjunction used by the borrowed turn. Later
            // conditions are deliberately not inferred when an earlier one
            // fails. Use the already-computed selectors; no extra selection,
            // clock, hardware access or scheduling mutation.
            let gate = if self.intent_request_ack.is_none() { 0 }
                else if self.intent_request_ack == Some(false) { 1 }
                else if self.intent_ack_backing.is_none() { 2 }
                else if !self.intent_ack_yield && {
                    #[cfg(feature = "sunfish-registry-converted-grant-turn")]
                    { self.registry_converted_grant.is_none_or(|(backing, target)|
                        self.intent_ack_backing != Some(backing)
                            || target != (RequestHandle { owner: self.owner, generation: self.generation,
                                intent: self.receive[receive].id })) }
                    #[cfg(not(feature = "sunfish-registry-converted-grant-turn"))]
                    { true }
                }
/* READY_HANDOFF_BEGIN */
                && { #[cfg(feature="sunfish-registry-ready-handoffs")] {candidate!=Some(index)}
                     #[cfg(not(feature="sunfish-registry-ready-handoffs"))] {true} }
/* READY_HANDOFF_END */
                { 3 }
                else if self.registry_turn_used { 4 }
                else if tftp_ready { 5 }
                else if wrq_return { 6 }
                else if self.receive[receive].done_pending { 7 }
                else if tx.registry_grant_turn_used { 8 }
                else if candidate != Some(index) { 9 }
                else if priority != Some(index) { 10 }
                else { 11 };
            let count = &mut tx.registry_publication.grant_gates[gate];
            *count = count.saturating_add(1);
            #[cfg(feature = "sunfish-registry-prearm-io-trace")]
            if prearm_entry && let Some(pair) = self.registry_window.target.as_mut()
                && pair.newer == (RequestHandle { owner: self.owner, generation: self.generation, intent: self.receive[receive].id })
            { pair.prearm.first_gate = Some(gate as u8); }

            #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
            if first_entry && let Some(pair) = self.registry_window.target.as_mut()
                && pair.newer == (RequestHandle { owner: self.owner, generation: self.generation, intent: self.receive[receive].id })
                && let Some(entry) = pair.conversion.first.as_mut().and_then(|t| t.entry.as_mut())
            { entry.gate = Some(gate as u8); }

        }
    }

    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    pub fn set_registry_ack_clock(&mut self, clock: fn() -> u64) {
        self.intent_ack_clock = Some(clock);
    }

    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    pub fn intent_ack_summary(&self) -> (u32, Option<u64>) {
        self.intent_ack_summary
    }

    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    fn intent_ack_age(&self) -> Option<u64> {
        self.intent_ack_started.zip(self.intent_ack_clock)
            .map(|(started, clock)| clock().wrapping_sub(started))
    }

    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    fn observe_pending_ack(&mut self, event: AckObservation) {
        for tx in &mut self.transmit {
            if tx.sent == tx.len
                || !matches!(tx.kind, TxKind::Reply { route, .. } if route.service == Service::ServiceRegistry)
            {
                continue;
            }
            let trace = &mut tx.registry_publication.ack;
            match event {
                AckObservation::Sent(age) => {
                    trace.sent = trace.sent.saturating_add(1);
                    trace.max_age = trace.max_age.max(age);
                }
                AckObservation::Blocked => trace.blocked = trace.blocked.saturating_add(1),
                AckObservation::DoneAhead => trace.done_ahead = trace.done_ahead.saturating_add(1),
                AckObservation::AdvertisementAhead => trace.ads_ahead = trace.ads_ahead.saturating_add(1),
            }
        }
    }

    #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
    fn elf_oack_slot(&self) -> Option<usize> {
        self.elf_exchange.as_ref()?.server_check.as_ref()
            .filter(|write| write.oack.phase == 2)?
            .oack.slot
    }

    #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
    fn first_oack_queue_slot(&self) -> Option<usize> {
        #[cfg(feature = "sunfish-ipa-registration-rx-first")]
        return self.first_server_check.as_ref()
            .filter(|write| write.before_elf && write.exact_wrq && !write.coalesced
                && write.oack.phase == 2 && write.timing.closes == 0)?
            .oack.slot;
        #[cfg(not(feature = "sunfish-ipa-registration-rx-first"))]
        None
    }

    #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
    fn oack_publish_clock(&self) -> Option<u64> {
        let slot = self.elf_oack_slot().or_else(|| self.first_oack_queue_slot());
        #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
        let slot = slot.or_else(|| self.retry_oack_slot());
        #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
        let slot = slot.or_else(|| self.reserved_writer_pending().then_some(TX_SLOTS));
        slot?;
        self.intent_ack_clock.map(|clock| clock())
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
    fn retry_oack_slot(&self) -> Option<usize> {
        let retry = self.first_server_check.as_ref()?.repeat.as_ref()?;
        if retry.oack.phase == 2 { retry.oack.slot } else { None }
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
    fn observe_retry_publish(
        &mut self,
        started_at: Option<u64>,
        choice: &PublishSelection,
        result: Result<(), WriteError>,
    ) {
        let Some(slot) = self.retry_oack_slot() else { return; };
        let selection = match *choice {
            PublishSelection::Idle => 1,
            PublishSelection::IntentRequest => 2,
            PublishSelection::Advertisement => 3,
            PublishSelection::IntentAck => 4,
            PublishSelection::ReceiveDone => 5,
            PublishSelection::Data(selected) if selected == slot => 6,
            PublishSelection::Data(selected) if matches!(self.transmit[selected].kind, TxKind::TftpReply { .. }) => 7,
            PublishSelection::Data(_) => 8,
        };
        let target = &self.transmit[slot];
        let conditions = u32::from(self.remote_for_transmit(target).is_none()) << 8
            | u32::from(self.unsent_flow_predecessor(target)) << 10
            | u32::from(result == Err(WriteError::WouldBlock)) << 11
            | u32::from(result == Err(WriteError::Uncertain)) << 12;
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        let conditions = conditions
            | (u32::from(matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. })) << 9);
        let trace = &mut self.first_server_check.as_mut().unwrap().repeat.as_mut().unwrap().tx;
        let sample = selection | conditions;
        if trace.calls == 0 {
            trace.first_call = started_at;
            trace.first = Some(sample);
        }
        trace.last = Some(sample);
        trace.seen |= (1 << (selection - 1)) | conditions;
        trace.calls = trace.calls.saturating_add(1);
    }

    #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
    fn reserved_writer_pending(&self) -> bool {
        self.reserved_oack.is_some_and(|oack| oack.peer.port == 0x49 && oack.published_at.is_none())
            && self.transmit[TX_SLOTS].len != 0
            && self.transmit[TX_SLOTS].sent < self.transmit[TX_SLOTS].len
    }

    #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
    fn observe_reserved_writer(&mut self, started: Option<u64>, writer: Option<u64>,
        choice: PublishSelection, result: Option<Result<(), WriteError>>) {
        if !self.reserved_writer_pending() { return; }
        let end = self.intent_ack_clock.map(|clock| clock());
        let tx = &self.transmit[TX_SLOTS];
        let no_fit = self.remote_for_transmit(tx).is_none();
        let ready = self.ready_remote_for_transmit(tx).is_some();
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        let intent_wait = matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. });
        #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
        let intent_wait = false;
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        let older_flow = self.unsent_flow_predecessor(tx);
        #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
        let older_flow = false;
        let other_tftp = match choice { PublishSelection::Data(slot) if slot != TX_SLOTS =>
            matches!(self.transmit[slot].kind, TxKind::TftpReply { .. }), _ => false };
        let oack = self.reserved_oack.as_mut().unwrap();
        let trace = &mut oack.writer_trace;
        let delta = |a: Option<u64>, b: Option<u64>| a.zip(b).and_then(|(a,b)| b.checked_sub(a)).unwrap_or(0);
        if trace.calls == 0 { trace.first_at = started; trace.previous_end = oack.queued_at; }
        trace.max_gap_ticks = trace.max_gap_ticks.max(delta(trace.previous_end, started));
        trace.previous_end = end;
        trace.active_ticks = trace.active_ticks.saturating_add(delta(started, end));
        trace.writer_ticks = trace.writer_ticks.saturating_add(delta(writer, end));
        trace.calls = trace.calls.saturating_add(1);
        trace.no_fit = trace.no_fit.saturating_add(u32::from(no_fit));
        trace.ready = trace.ready.saturating_add(u32::from(ready));
        trace.intent_wait = trace.intent_wait.saturating_add(u32::from(intent_wait));
        trace.older_flow = trace.older_flow.saturating_add(u32::from(older_flow));
        let counter = match (result, choice) {
            (None, _) => &mut trace.followup_yield,
            (Some(Err(WriteError::WouldBlock)), PublishSelection::Data(slot)) if slot == TX_SLOTS => &mut trace.oack_blocked,
            (Some(Err(WriteError::WouldBlock)), _) => &mut trace.other_blocked,
            (Some(Ok(())), PublishSelection::Idle) => &mut trace.idle,
            (Some(Ok(())), PublishSelection::IntentRequest) => &mut trace.intent_requests,
            (Some(Ok(())), PublishSelection::Advertisement) => &mut trace.advertisements,
            (Some(Ok(())), PublishSelection::IntentAck) => &mut trace.intent_acks,
            (Some(Ok(())), PublishSelection::ReceiveDone) => &mut trace.receive_dones,
            (Some(Ok(())), PublishSelection::Data(slot)) if slot == TX_SLOTS => &mut trace.oack_selected,
            (Some(Ok(())), PublishSelection::Data(_)) if other_tftp => &mut trace.other_tftp,
            (Some(Ok(())), PublishSelection::Data(_)) => &mut trace.other_data,
            _ => return,
        };
        *counter = counter.saturating_add(1);
    }

    #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
    fn observe_elf_oack_publish(
        &mut self,
        started_at: Option<u64>,
        writer_at: Option<u64>,
        choice: PublishSelection,
        result: Result<(), WriteError>,
    ) {
        #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
        self.observe_retry_publish(started_at, &choice, result);
        #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
        self.observe_reserved_writer(started_at, writer_at, choice, Some(result));
        for (_first, slot) in [(true, self.first_oack_queue_slot()), (false, self.elf_oack_slot())] {
            let Some(slot) = slot else { continue };
            let ended_at = self.intent_ack_clock.map(|clock| clock());
            let target = &self.transmit[slot];
            let no_fit = self.remote_for_transmit(target).is_none();
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            let intent_wait = matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. });
            #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
            let intent_wait = false;
            #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
            let older_flow = self.unsent_flow_predecessor(target);
            #[cfg(not(feature = "sunfish-qrtr-rmtfs-flow-experiment"))]
            let older_flow = false;
            let write = self.elf_exchange.as_mut().and_then(|exchange| exchange.server_check.as_mut());
            #[cfg(feature = "sunfish-ipa-registration-rx-first")]
            let write = if _first { self.first_server_check.as_mut() } else { write };
            let Some(write) = write else { continue };
            let trace = &mut write.timing.queue;
            if trace.calls == 0 {
                trace.first_at = started_at;
            }
            let elapsed = |start: Option<u64>, end: Option<u64>| {
                start.zip(end).and_then(|(start, end)| end.checked_sub(start)).unwrap_or(0)
            };
            trace.max_gap_ticks = trace.max_gap_ticks.max(elapsed(trace.previous_end, started_at));
            trace.previous_end = ended_at;
            trace.active_ticks = trace.active_ticks.saturating_add(elapsed(started_at, ended_at));
            let writer_ticks = elapsed(writer_at, ended_at);
            trace.writer_ticks = trace.writer_ticks.saturating_add(writer_ticks);
            if matches!(choice, PublishSelection::Data(selected) if selected == slot) {
                trace.oack_writer_ticks = trace.oack_writer_ticks.saturating_add(writer_ticks);
            }
            trace.calls = trace.calls.saturating_add(1);
            trace.no_fit = trace.no_fit.saturating_add(u32::from(no_fit));
            trace.intent_wait = trace.intent_wait.saturating_add(u32::from(intent_wait));
            trace.older_flow = trace.older_flow.saturating_add(u32::from(older_flow));

            let count = match (result, choice) {
                (Err(WriteError::WouldBlock), PublishSelection::Data(selected)) if selected == slot => &mut trace.oack_blocked,
                (Err(WriteError::WouldBlock), _) => &mut trace.other_blocked,
                (Ok(()), PublishSelection::Idle) => &mut trace.idle,
                (Ok(()), PublishSelection::IntentRequest) => &mut trace.intent_requests,
                (Ok(()), PublishSelection::Advertisement) => &mut trace.advertisements,
                (Ok(()), PublishSelection::IntentAck) => &mut trace.intent_acks,
                (Ok(()), PublishSelection::ReceiveDone) => &mut trace.receive_dones,
                (Ok(()), PublishSelection::Data(selected)) if selected != slot => {
                    if matches!(self.transmit[selected].kind, TxKind::TftpReply { .. }) {
                        &mut trace.other_tftp
                    } else {
                        &mut trace.other_data
                    }
                }
                _ => continue,
            };
            *count = count.saturating_add(1);
        }
    }

    /// Emit at most one bounded record. No callback is invoked for stale state,
    /// exhausted peer intents, or an idle queue. A native implementation must
    /// preserve this writer's all-or-nothing publication contract.
    pub fn publish_next<F>(&mut self, generation: u64, writer: F) -> Result<Option<Published>, Error>
    where
        F: FnOnce(&[u8]) -> Result<(), WriteError>,
    {
        self.publish_inner(generation, writer, #[cfg(feature = "sunfish-tftp-oack-followup")] None)
    }

    #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
    pub(crate) fn early_reserved_oack_pending(&self) -> bool {
        self.reserved_oack.is_some_and(|oack| oack.peer.port == 0x49
            && oack.published_at.is_none() && oack.early_status == 0)
            && self.server_check_49().is_some_and(|trace| trace.first_close_at.is_none()
                && trace.oack_at.is_none()
                && self.transmit[TX_SLOTS].server_check_wrq == Some(trace.intent))
    }

    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub(crate) fn take_oack_credit_retry(&mut self) -> bool {
        #[cfg(feature = "sunfish-tftp-oack-credit-early")]
        if self.intent_request_ack.is_some() {
            return false;
        }
        let eligible = self.server_check_49().is_some_and(|trace|
            trace.first_close_at.is_none() && trace.oack_at.is_none());
        if let Some(oack) = self.reserved_oack.as_mut() {
            if oack.retry_state == 3 {
                if eligible && oack.published_at.is_none() {
                    oack.retry_state = 4;
                    oack.retry_attempt_at = self.intent_ack_clock.map(|clock| clock());
                    return true;
                }
                oack.retry_state = 5;
                return false;
            }
        }
        false
    }

    #[cfg(all(test, feature = "sunfish-tftp-oack-after-rxdone"))]
    pub(crate) fn replay_allow_receive_done(&mut self) {
        self.receive_done_since_data = false;
    }

    #[cfg(all(test, feature = "sunfish-tftp-oack-after-rxdone"))]
    pub(crate) fn replay_wait_for_oack_done(&mut self, id: u32) {
        let oack = self.reserved_oack.as_mut().expect("reserved OACK");
        oack.retry_choice = 3;
        oack.retry_choice_id = id;
        oack.retry_gates = 0x100;
        oack.retry_state = 4;
        oack.retry_result = 3;
        oack.after_done_state = 1;
    }

    #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
    pub(crate) fn take_oack_after_done(&mut self) -> bool {
        #[cfg(feature = "sunfish-tftp-after-done-trace")]
        if let Some(oack) = self.reserved_oack.as_mut() && oack.after_done_state == 2 {
            if oack.after_done_first_check.is_none() {
                oack.after_done_first_check = self.intent_ack_clock.map(|clock| clock());
            }
            if self.intent_request_ack.is_some() {
                oack.after_done_pending_checks = oack.after_done_pending_checks.saturating_add(1);
            }
        }
        #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
        let grant_blocks = self.intent_request_ack.is_some()
            && !(self.intent_request_ack == Some(true) && self.intent_ack_backing.is_some());
        #[cfg(not(feature = "sunfish-tftp-after-done-grant-turn"))]
        let grant_blocks = self.intent_request_ack.is_some();
        if grant_blocks {
            return false;
        }
        let eligible = self.server_check_49().is_some_and(|trace|
            trace.first_close_at.is_none() && trace.oack_at.is_none());
        if let Some(oack) = self.reserved_oack.as_mut() && oack.after_done_state == 2 {
            if eligible && oack.published_at.is_none() {
                oack.after_done_state = 3;
                oack.after_done_retry_at = self.intent_ack_clock.map(|clock| clock());
                return true;
            }
            oack.after_done_state = 4;
        }
        false
    }

    #[cfg(feature = "sunfish-tftp-oack-credit-early")]
    pub(crate) fn take_oack_credit_turn(&mut self) -> bool {
        let eligible = self.server_check_49().is_some_and(|trace|
            trace.first_close_at.is_none() && trace.oack_at.is_none());
        if let Some(oack) = self.reserved_oack.as_mut()
            && oack.retry_state == 2 && oack.credit_turn == 0 && eligible
        {
            oack.credit_turn = 1;
            return true;
        }
        false
    }

    #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
    pub(crate) fn replay_take_oack_after_grant(&mut self) -> bool {
        if self.intent_request_ack.is_some() {
            return false;
        }
        self.take_oack_credit_retry()
    }

    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
    pub(crate) fn oack_followup_grant_pending(&self) -> bool {
        self.intent_request_ack.is_some()
    }

    #[cfg(feature = "sunfish-tftp-oack-followup")]
    pub(crate) fn publish_oack_followup<F>(&mut self, generation: u64, stage: OackFollowup, writer: F)
        -> Result<Option<Published>, Error>
    where
        F: FnOnce(&[u8]) -> Result<(), WriteError>,
    {
        #[cfg(feature = "sunfish-first-oack-grant-window")]
        if matches!(stage, OackFollowup::FirstGrantAd | OackFollowup::FirstGrantInit | OackFollowup::FirstGrantAck | OackFollowup::FirstGrantReply) {
            return self.publish_inner(generation, writer, Some(stage));
        }
        let result = self.publish_inner(generation, writer, Some(stage));
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        if matches!(stage, OackFollowup::EarlyReserve) {
            if let Some(oack) = self.reserved_oack.as_mut() {
                oack.early_status = match result {
                    Ok(Some(_)) => 1,
                    Err(Error::WouldBlock) => 2,
                    Ok(None) | Err(Error::NoRemoteIntent) => 3,
                    Err(_) => 4,
                };
            }
            #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
            if let Some(oack) = self.reserved_oack.as_mut() {
                if oack.early_status == 3 && oack.early_command == 8
                    && oack.early_guards & 0x12 == 0x12 {
                    oack.retry_state = 1;
                }
            }
            return result;
        }
        #[cfg(feature = "sunfish-tftp-oack-credit-early")]
        if matches!(stage, OackFollowup::CreditReplay)
            && let Some(oack) = self.reserved_oack.as_mut() && oack.credit_turn == 1
        {
            oack.credit_turn = match result {
                Ok(Some(_)) => 2,
                Err(Error::WouldBlock) => 3,
                Ok(None) | Err(Error::NoRemoteIntent) => 4,
                Err(_) => 5,
            };
        }
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        if matches!(stage, OackFollowup::Reply)
            && let Some(oack) = self.reserved_oack.as_mut()
            && oack.retry_state == 4 && oack.retry_result == 0
        {
            oack.retry_result = match result {
                Ok(Some(_)) => 1,
                Err(Error::WouldBlock) => 2,
                Ok(None) => 3,
                Err(Error::NoRemoteIntent) => 4,
                Err(_) => 5,
            };
        }
        #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
        if let Some(oack) = self.reserved_oack.as_mut() && oack.after_done_state == 3 {
            let status = match result {
                Ok(Some(_)) => 1,
                Err(Error::WouldBlock) => 2,
                Ok(None) | Err(Error::NoRemoteIntent) => 3,
                Err(_) => 4,
            };
            match stage {
                OackFollowup::Advertise if oack.after_done_ad == 0 => oack.after_done_ad = status,
                OackFollowup::Grant if oack.after_done_ack == 0 => oack.after_done_ack = status,
                _ => {}
            }
        }
        #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
        if matches!(stage, OackFollowup::Reply)
            && let Some(oack) = self.reserved_oack.as_mut()
        {
            if oack.after_done_state == 3 && oack.after_done_result == 0 {
                oack.after_done_result = match result {
                    Ok(Some(_)) => 1,
                    Err(Error::WouldBlock) => 2,
                    Ok(None) => 3,
                    Err(Error::NoRemoteIntent) => 4,
                    Err(_) => 5,
                };
            } else if oack.after_done_state == 0 && oack.retry_state == 4
                && oack.retry_result == 3 && oack.retry_choice == 3
                && oack.retry_gates & 0x300 != 0
            {
                oack.after_done_state = 1;
            }
        }
        if let Some(trace) = self.oack_slot_follow.as_mut() {
            let status = match result {
                Ok(Some(_)) => 1,
                Err(Error::WouldBlock) => 2,
                Ok(None) | Err(Error::NoRemoteIntent) => 3,
                Err(_) => 4,
            };
            match stage {
                #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
                OackFollowup::CreditReplay => {},
                #[cfg(feature = "sunfish-first-oack-grant-window")]
                OackFollowup::FirstGrantAd | OackFollowup::FirstGrantInit | OackFollowup::FirstGrantAck | OackFollowup::FirstGrantReply => unreachable!(),
                OackFollowup::Advertise => trace.followup_ad = status,
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                OackFollowup::EarlyReserve => unreachable!(),
                #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
                OackFollowup::Grant => trace.followup_grant = status,
                OackFollowup::Reply => trace.followup_reply = status,
            }
        }
        result
    }

    fn publish_inner<F>(
        &mut self,
        generation: u64,
        writer: F,
        #[cfg(feature = "sunfish-tftp-oack-followup")] followup: Option<OackFollowup>,
    ) -> Result<Option<Published>, Error>
    where
        F: FnOnce(&[u8]) -> Result<(), WriteError>,
    {
        self.check(generation)?;
        #[cfg(feature="sunfish-elf-writer-trace")]
        self.begin_elf_writer_pass();
        #[cfg(feature = "sunfish-tftp-oack-followup")]
        let ordinary = followup.is_none();
        #[cfg(not(feature = "sunfish-tftp-oack-followup"))]
        let ordinary = true;
        #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
        let conversion_entry = self.observe_conversion_entry(ordinary);
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        let prearm_entry = self.observe_prearm_entry();
        #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
        let registration_started = self.registration_tx_slot().and(self.intent_ack_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
        let elf_oack_started = self.oack_publish_clock();
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        if let OutboundIntentRequest::Waiting { size, granted: true } = self.outbound_intent_request {
            if self.remote.iter().flatten().any(|intent| !intent.busy && intent.capacity >= size) {
                self.outbound_intent_request = OutboundIntentRequest::Done { size, granted: true };
            }
        }
        if self.control.started() {
            self.maintain_receives(generation)?;
            self.stage_control();
        }
        self.stage_wlfw_registration()?;
        self.stage_wlfw_dynamic_feature_mask()?;
        #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
        let first_oack_ready = ordinary && self.observe_first_oack_pass();
        #[cfg(feature = "sunfish-post-reg-oack-turn")]
        let post_reg_turn = self.start_post_reg_oack_turn(followup);
        #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
        if !self.transmit.iter().any(|tx| matches!(tx.kind, TxKind::TftpReply { .. }) && tx.len != 0 && tx.sent < tx.len) {
            self.tftp_control_turn_due = false;
        }
        #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
        let prefer_ack = self.intent_request_ack.is_some() && !self.intent_ack_yield;
        #[cfg(not(feature = "sunfish-wlan-intent-ack-turn"))]
        let prefer_ack = false;
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        let (conversion_wrq_return, conversion_tftp) = self.registry_conversion_protected_work();
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        let converted_registry = if ordinary && !conversion_wrq_return && !conversion_tftp {
            self.converted_registry_candidate()
        } else { None };
        // A released first WRQ can wait behind traffic that continually
        // refills lower receive slots. Give its exact RX_DONE one turn after
        // any preferred peer intent ACK and its backing advertisement.
        #[cfg(feature = "sunfish-wlan-first-wrq-rxdone-turn")]
        let priority_wrq_done = self.first_server_check.as_ref().filter(|write|
            !prefer_ack && write.before_elf && write.exact_wrq && !write.coalesced
                && write.wrq_result == Some(0) && write.oack.phase >= 1 && write.timing.closes == 0)
            .and_then(|write| self.receive.iter().position(|rx|
                rx.tftp_datagram_id() == write.request.intent
                    && rx.state == RxState::Control && rx.done_pending));
        #[cfg(not(feature = "sunfish-wlan-first-wrq-rxdone-turn"))]
        let priority_wrq_done = None;
        #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
        let priority_tftp = if self.tftp_control_turn_due && !prefer_ack && priority_wrq_done.is_none() {
            self.next_ready_tftp()
        } else {
            None
        };
        #[cfg(not(feature = "sunfish-glink-tftp-control-fairness-experiment"))]
        let priority_tftp: Option<(usize, usize)> = None;
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        let priority_tftp = if self.reserved_oack.is_some_and(|oack| oack.retry_state == 2)
            && self.next_transmit_in_order([1]).is_some() {
            None
        } else {
            priority_tftp
        };
        // Yield to the existing TFTP turn, intent ACKs and ready QRTR credits.
        // A successful registry turn must be followed by a normal publication
        // before another priority record can be sent.
        // After a committed ACK, lend one attempt before a new positive
        // grant's backing advertisement. Once advertised, its ACK keeps its
        // turn. The per-transmit latch survives blocked writes; diagnostics
        // and clocks never decide eligibility.
        #[cfg(feature = "sunfish-registry-between-grants")]
        let registry_before_grant = self.intent_ack_yield
            && self.intent_request_ack == Some(true) && self.intent_ack_backing.is_some();
        #[cfg(not(feature = "sunfish-registry-between-grants"))]
        let registry_before_grant = false;
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        let registry_before_grant = registry_before_grant || converted_registry.is_some();
/* READY_HANDOFF_BEGIN */
        #[cfg(feature="sunfish-registry-ready-handoffs")]
        let ready_registry = if ordinary && !registry_before_grant && !self.registry_turn_used
            && !conversion_wrq_return && !conversion_tftp
            && self.intent_request_ack==Some(true)
            && self.intent_ack_backing.is_some_and(|id|self.receive[..RX_SLOTS].iter()
                .any(|r|r.id==id && r.state==RxState::Advertising))
        {self.next_transmit_in_order([3]).filter(|(i,_)|self.transmit[*i].registry_return_turn_due
                && !self.transmit[*i].registry_grant_turn_used && !self.transmit[*i].registry_credit_turn_used)}else{None};
        #[cfg(feature="sunfish-registry-ready-handoffs")]
        let registry_before_grant=registry_before_grant || ready_registry.is_some();
/* READY_HANDOFF_END */
        #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
        let registry_candidate = if !self.registry_turn_used && priority_tftp.is_none()
            && (self.intent_request_ack.is_none() || registry_before_grant)
            && priority_wrq_done.is_none()
        {
            self.next_transmit_in_order([3]).filter(|(slot, _)| {
                #[cfg(feature = "sunfish-registry-between-grants")]
                if registry_before_grant && self.transmit[*slot].registry_grant_turn_used { return false; }
                let _ = slot;
                true
            })
        } else {
            None
        };
        #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
        let priority_registry = registry_candidate.filter(|_| self.next_transmit_in_order([1]).is_none());
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        let prefer_ack = prefer_ack && !(converted_registry.is_some() && priority_registry == converted_registry);
/* READY_HANDOFF_BEGIN */
        #[cfg(feature="sunfish-registry-ready-handoffs")]
        let prefer_ack=prefer_ack && !(ready_registry.is_some() && priority_registry==ready_registry);
/* READY_HANDOFF_END */
        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
        let converted_credit = self.converted_credit_candidate(registry_candidate, converted_registry);
        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
        let prefer_ack = prefer_ack && converted_credit.is_none();
        #[cfg(feature = "sunfish-wlan-registry-turn-gates")]
        self.observe_registry_turn_gates(
            priority_tftp.is_some(),
            registry_candidate.map(|(slot, _)| slot),
            priority_registry.map(|(slot, _)| slot),
        );
        #[cfg(feature = "sunfish-registry-grant-eligibility")]
        self.observe_registry_grant_eligibility(
            priority_tftp.is_some() || {
                #[cfg(feature = "sunfish-registry-converted-grant-turn")] { conversion_tftp }
                #[cfg(not(feature = "sunfish-registry-converted-grant-turn"))] { false }
            }, priority_wrq_done.is_some() || {
                #[cfg(feature = "sunfish-registry-converted-grant-turn")] { conversion_wrq_return }
                #[cfg(not(feature = "sunfish-registry-converted-grant-turn"))] { false }
            },
            registry_candidate.map(|(slot, _)| slot),
            priority_registry.map(|(slot, _)| slot),
            #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")] conversion_entry,
            #[cfg(feature = "sunfish-registry-prearm-io-trace")] prearm_entry,
        );
        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
        let entry_length_consistent = self.registry_window.target.and_then(|p| p.conversion.first)
            .is_some_and(|t| t.length_consistent);
        #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
        if conversion_entry && let Some(pair) = self.registry_window.target.as_mut()
            && let Some(entry) = pair.conversion.first.as_mut().and_then(|t| t.entry.as_mut())
        {
            entry.flags |= (u32::from(converted_registry.is_some()) << 11)
                | (u32::from(priority_registry.is_some()) << 12)
                | (u32::from(conversion_tftp) << 13)
                | (u32::from(conversion_wrq_return) << 14)
                | (u32::from(registry_candidate.is_some() && priority_registry.is_none()) << 15);
            #[cfg(feature = "sunfish-registry-converted-credit-turn")]
            { entry.flags |= (u32::from(converted_credit.is_some()) << 19)
                | (u32::from(entry_length_consistent) << 20); }

        }
        #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
        let priority_data = priority_tftp.or(priority_registry);
        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
        let priority_data = priority_data.or(converted_credit.map(|(_, credit, remote)| (credit, remote)));
        #[cfg(not(feature = "sunfish-wlan-registry-tx-turn"))]
        let priority_data = priority_tftp;
        // Borrow one attempt only before the backing advertisement. Once
        // advertised, the grant keeps its existing ACK turn.
        #[cfg(feature = "sunfish-ipa-reply-before-grant")]
        let registration_before_grant = self.intent_request_ack == Some(true)
            && self.intent_ack_backing.is_some()
            && self.first_server_check.as_ref().is_some_and(|write|
                write.before_elf && write.exact_wrq && !write.coalesced
                    && write.oack.phase == 2 && write.oack.slot.is_some() && write.timing.closes == 0);
        // Preserve protected TFTP/registry turns and ready QRTR credits.
        // A completed grant may lend one turn before the next advertisement.
        #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
        let grant_allows_registration_reply = self.intent_request_ack.is_none();
        #[cfg(feature = "sunfish-ipa-registration-reply-between-grants")]
        let grant_allows_registration_reply = grant_allows_registration_reply || self.intent_ack_yield;
        #[cfg(feature = "sunfish-ipa-reply-before-grant")]
        let grant_allows_registration_reply = grant_allows_registration_reply || registration_before_grant;
        #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
        let priority_registration = if let Some(slot) = self.registration_tx_slot() {
            let candidate = self.next_ipa_registration_transmit();
            let reason = if priority_data.is_some() { Some(0) }
                else if priority_wrq_done.is_some() { Some(1) }
                else if !grant_allows_registration_reply { Some(2) }
                else if self.next_transmit_in_order([1]).is_some() { Some(3) }
                else if candidate.is_none() {
                    let pending = self.ipa_registration_trace().is_some_and(|registration|
                        self.receive.iter().any(|rx| rx.id == registration.request.intent && rx.done_pending));
                    Some(if pending { 4 } else { 5 })
                } else { None };
            debug_assert!(candidate.is_none_or(|(target, _)| target == slot));
            self.observe_registration_priority(reason);
            candidate.filter(|_| reason.is_none())
        } else {
            None
        };
        #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
        let priority_data = priority_data.or(priority_registration);
        #[cfg(feature = "sunfish-ipa-reply-before-grant")]
        let prefer_ack = prefer_ack && !(registration_before_grant && priority_registration.is_some());
        // A successful ACK can lend the registration completion one attempt
        // before the next grant. The next ACK still requires its advertisement.
        #[cfg(all(feature = "sunfish-ipa-registration-rxdone-turn", not(feature = "sunfish-ipa-registration-rxdone-before-grant")))]
        let grant_allows_registration_done = self.intent_request_ack.is_none();
        #[cfg(all(feature = "sunfish-ipa-registration-between-grants", not(feature = "sunfish-ipa-registration-rxdone-before-grant")))]
        let grant_allows_registration_done = grant_allows_registration_done || self.intent_ack_yield;
        // Borrow one attempt ahead of the grant pair. The existing writer
        // still requires the backing advertisement before a positive ACK.
        #[cfg(feature = "sunfish-ipa-registration-rxdone-before-grant")]
        let grant_allows_registration_done = true;
        #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
        let priority_registration_done = if priority_data.is_none() && priority_wrq_done.is_none()
            && grant_allows_registration_done && self.next_transmit_in_order([1]).is_none()
        {
            self.next_ipa_registration_done()
        } else {
            None
        };
        #[cfg(not(feature = "sunfish-ipa-registration-rxdone-turn"))]
        let priority_registration_done: Option<usize> = None;
        #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
        self.observe_registration_done_pass([
            priority_data.is_some(), priority_wrq_done.is_some(),
            !grant_allows_registration_done, self.next_transmit_in_order([1]).is_some(),
        ], priority_registration_done.is_some());
        // Give the exact init datagram one writer attempt after existing
        // priority work. A blocked attempt returns to normal arbitration.
        #[cfg(feature = "sunfish-ipa-init-tx-turn")]
        let init_grant_allows_turn = self.intent_request_ack.is_none();
        // Borrow only the first ordinary pass ahead of the same unadvertised
        // positive grant seen at queueing. Once advertised, its ACK keeps its turn.
        #[cfg(feature = "sunfish-ipa-init-before-ad")]
        let init_before_ad = followup.is_none() && prefer_ack && self.intent_request_ack == Some(true)
            && self.intent_ack_backing.is_some() && self.original_1f_close_count() == Some(0)
            && self.ipa_registration_trace().is_some_and(|r| r.published_at.is_some() && r.completed_at.is_none())
            && self.ipa_init_transport_trace().is_some_and(|t| t.tx.passes == 0
                && t.tx.queue_state == 0xc7 && t.tx.queued_grant == self.intent_ack_backing)
            && !matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. })
            && self.next_transmit_in_order([3]).is_none()
            && self.queued_ipa_init_slot().is_some_and(|s| !self.unsent_flow_predecessor(&self.transmit[s]));
        #[cfg(feature = "sunfish-ipa-init-before-ad")]
        let init_grant_allows_turn = init_grant_allows_turn || init_before_ad;
        #[cfg(feature = "sunfish-ipa-init-tx-turn")]
        let priority_init = if priority_data.is_none() && priority_wrq_done.is_none()
            && priority_registration_done.is_none()
            && init_grant_allows_turn && self.next_transmit_in_order([1]).is_none()
        {
            self.next_ipa_init_transmit()
        } else {
            None
        };
        #[cfg(feature = "sunfish-ipa-init-before-ad")]
        let prefer_ack = prefer_ack && !(init_before_ad && priority_init.is_some());
        #[cfg(feature = "sunfish-ipa-init-writer-counts")]
        self.observe_ipa_schedule_gates([
            self.intent_request_ack.is_some(), priority_wrq_done.is_some(),
            priority_tftp.is_some(), priority_data.is_some() && priority_tftp.is_none(),
            self.next_transmit_in_order([1]).is_some(),
            self.queued_ipa_init_slot().is_some_and(|slot| {
                let tx = &self.transmit[slot];
                tx.sent != 0 || tx.len > TX_CHUNK_BYTES || self.ready_remote_for_transmit(tx).is_none()
            }),
            self.ipa_init_transport_trace().is_some_and(|trace| trace.tx.priority_turn != 0),
            priority_init.is_some(),
        ]);
        #[cfg(feature = "sunfish-ipa-init-tx-turn")]
        let priority_data = priority_data.or(priority_init);
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        let early_reserve = if matches!(followup, Some(OackFollowup::EarlyReserve)) {
            let pending = self.early_reserved_oack_pending();
            let grant = self.intent_request_ack.is_some();
            let credit = self.next_transmit_in_order([1]).is_some();
            let tx = &self.transmit[TX_SLOTS];
            let ready = self.ready_remote_for_transmit(tx).is_some();
            let mut guards = u16::from(!pending)
                | (u16::from(grant) << 1)
                | (u16::from(priority_wrq_done.is_some()) << 2)
                | (u16::from(priority_registration_done.is_some()) << 3)
                | (u16::from(credit) << 4)
                | (u16::from(priority_data.is_some()) << 5)
                | (u16::from(!ready) << 6)
                | (u16::from(self.remote_for_transmit(tx).is_none()) << 7);
            #[cfg(feature = "sunfish-ipa-before-first-oack")]
            { guards |= u16::from(self.holds_first_oack(tx)) << 8; }
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            { guards |= u16::from(matches!(
                self.outbound_intent_request, OutboundIntentRequest::Waiting { .. }
            )) << 9; }
            #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
            { guards |= u16::from(self.unsent_flow_predecessor(tx)) << 10; }
            if let Some(oack) = self.reserved_oack.as_mut() {
                oack.early_guards = guards;
            }
            pending && !grant && priority_wrq_done.is_none()
                && priority_registration_done.is_none() && !credit
        } else {
            false
        };
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        let priority_data = priority_data.or_else(|| {
            early_reserve.then(|| self.ready_remote_for_transmit(&self.transmit[TX_SLOTS]))
                .flatten().map(|remote| (TX_SLOTS, remote))
        });
        #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
        if let Some(index) = self.first_tftp_slot {
            let tx = &self.transmit[index];
            if tx.len != 0 && tx.sent < tx.len {
                let suitable = tx.remote_slot.is_some() || self.remote.iter().flatten()
                    .any(|intent| !intent.busy && intent.capacity as usize >= tx.len);
                let selected = priority_data.or_else(|| self.next_transmit()).map(|(slot, _)| slot);
                #[cfg(feature = "sunfish-glink-intent-request-experiment")]
                let waiting = matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. });
                #[cfg(not(feature = "sunfish-glink-intent-request-experiment"))]
                let waiting = false;
                let mask = u32::from(suitable)
                    | (u32::from(selected == Some(index)) << 1)
                    | (u32::from(selected.is_some_and(|slot| slot != index)) << 2)
                    | (u32::from(waiting) << 3)
                    | (u32::from(self.receive.iter().any(|rx| rx.state == RxState::Advertising)) << 4)
                    | (u32::from(self.intent_request_ack.is_some()) << 5)
                    | (u32::from(self.receive.iter().any(|rx| rx.done_pending)) << 6);
                self.first_tftp_schedule.calls = self.first_tftp_schedule.calls.saturating_add(1);
                self.first_tftp_schedule.mask |= mask;
            }
        }
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        {
            self.intent_gate_calls = self.intent_gate_calls.saturating_add(1);
            if self.transmit.iter().any(|tx| tx.len != 0 && tx.sent < tx.len) {
                self.intent_gate_pending_calls = self.intent_gate_pending_calls.saturating_add(1);
                let gates = u32::from(self.next_transmit().is_some())
                    | (u32::from(self.intent_request_ack.is_some()) << 1)
                    | (u32::from(self.receive.iter().any(|rx| rx.state == RxState::Advertising)) << 2)
                    | (u32::from(self.outbound_intent_request != OutboundIntentRequest::Idle) << 3);
                self.intent_gate_mask |= gates;
                self.intent_gate_combinations |= 1u32 << gates;
                self.intent_gate_last = gates;
            }
        }
        let mut record = [0u8; MAX_TX_RECORD_BYTES];
        #[cfg(feature = "sunfish-ipa-init-tx-trace")]
        let init_target = self.observe_ipa_init_publish_pass();
        #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
        let registration_target = self.observe_registration_pass(registration_started);
        // A single bounded request must not wait behind an unending stream
        // of receive-control records. If any queued data can use an existing
        // intent, preserve the regular completion/control priorities instead.
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        if ordinary && priority_wrq_done.is_none() && priority_registration_done.is_none()
            && self.outbound_intent_request == OutboundIntentRequest::Idle && self.next_transmit().is_none()
            && self.intent_request_ack.is_none()
            && !self.receive.iter().any(|rx| rx.state == RxState::Advertising) {
            if let Some(tx) = self.transmit.iter().find(|tx| {
                #[cfg(feature = "sunfish-ipa-before-first-oack")]
                if self.holds_first_oack(tx) { return false; }
                tx.len != 0 && tx.sent < tx.len
            }) {
                let size = tx.len as u32;
                glink_header(&mut record, 7, self.config.local_channel, size);
                self.outbound_intent_attempts = self.outbound_intent_attempts.saturating_add(1);
                self.quarantined = true;
                #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
                let elf_oack_writer = self.oack_publish_clock();
                #[cfg(feature = "sunfish-ipa-init-tx-trace")]
                let init_writer = init_target.and(self.intent_ack_clock).map(|clock| clock());
                #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
                let registration_writer = registration_target.and(self.intent_ack_clock).map(|clock| clock());
                #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                let conversion_choice = self.observe_conversion_choice(&record[..8], None);
                #[cfg(feature = "sunfish-registry-prearm-io-trace")]
                let prearm_choice = self.observe_prearm_choice(&record[..8], None);
                #[cfg(feature = "sunfish-registry-prearm-io-trace")]
                self.observe_prearm_result(prearm_choice, 4);
                #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                self.observe_conversion_callback(conversion_choice);
                let result = writer(&record[..8]);
                #[cfg(feature="sunfish-elf-writer-trace")]
                self.elf_writer_header(&record[..8],result);
                #[cfg(feature = "sunfish-registry-prearm-io-trace")]
                self.observe_prearm_result(prearm_choice, match result { Ok(()) => 1, Err(WriteError::WouldBlock) => 2, Err(WriteError::Uncertain) => 3 });
                #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                if conversion_choice && let Some(choice) = self.registry_window.target.as_mut()
                    .and_then(|p| p.conversion.first.as_mut()).and_then(|t| t.choice.as_mut())
                { choice.result = match result { Ok(()) => 1, Err(WriteError::WouldBlock) => 2, Err(WriteError::Uncertain) => 3 }; }
                #[cfg(feature = "sunfish-post-reg-oack-turn")]
                if let Some(i) = post_reg_turn {
                    self.post_reg_turns.turns[i].written(7, result);
                }
                #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
                self.observe_registration_done_turn(6, result);
                #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
                self.observe_registration_write(registration_target, registration_started, registration_writer,
                    PublishSelection::IntentRequest, &record[..8], result);
                #[cfg(feature = "sunfish-ipa-init-writer-counts")]
                self.observe_ipa_schedule_command(&record[..8], result);
                #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
                self.observe_elf_oack_publish(elf_oack_started, elf_oack_writer, PublishSelection::IntentRequest, result);
                #[cfg(feature = "sunfish-ipa-init-tx-trace")]
                if let Some(target) = init_target {
                    self.observe_ipa_init_writer(target, &PublishSelection::IntentRequest, init_writer, result);
                }
                #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
                self.observe_registry_publication(PublishChoice::IntentRequest, false);
                match result {
                    Err(WriteError::WouldBlock) => {
                        #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
                        self.note_tftp_writer_blocked();
                        self.outbound_intent_blocked = self.outbound_intent_blocked.saturating_add(1);
                        self.quarantined = false;
                        return Err(Error::WouldBlock);
                    }
                    Err(WriteError::Uncertain) => return Err(Error::PublicationUncertain),
                    Ok(()) => self.quarantined = false,
                }
                self.outbound_intent_request = OutboundIntentRequest::Waiting { size, granted: false };
                return Ok(Some(Published::RemoteIntentRequest { size }));
            }
        }
        enum Commit {
            Advertise(usize),
            IntentAck,
            Done(usize),
            Data(usize, usize, usize),
        }
        #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
        let replay_credit = matches!(followup, Some(OackFollowup::CreditReplay))
            && self.reserved_oack.is_some_and(|oack| oack.retry_state == 2)
            && self.server_check_49().is_some_and(|trace| trace.first_close_at.is_none()
                && trace.oack_at.is_none())
            && !(self.intent_request_ack == Some(true) && self.intent_ack_backing.is_none());
        #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
        let prefer_ack = prefer_ack && !replay_credit;
        #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
        let priority_wrq_done = if replay_credit { None } else { priority_wrq_done };
        #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
        let priority_registration_done = if replay_credit { None } else { priority_registration_done };
        #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
        let priority_data = if replay_credit { self.next_transmit_in_order([1]) } else { priority_data };
        // Give a pending grant its own advertisement followed by the ACK.
        // After a successful ACK, ordinary traffic gets the next available turn.
        #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
        let backing = self.intent_ack_backing.map(|id| {
            self.receive.iter().position(|rx| rx.id == id && rx.state == RxState::Advertising)
                .ok_or(Error::InvalidIntent)
        }).transpose()?;
        #[cfg(not(feature = "sunfish-wlan-intent-ack-turn"))]
        let backing = None;
        #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
        let yield_to_data = self.intent_ack_yield && self.intent_request_ack.is_some()
            && self.next_transmit().is_some();
        #[cfg(not(feature = "sunfish-wlan-intent-ack-turn"))]
        let yield_to_data = false;
        let advertisement = if prefer_ack {
            backing
        } else {
            self.receive.iter().position(|rx| rx.state == RxState::Advertising)
                .filter(|_| priority_data.is_none())
        };
        let ack = self.intent_request_ack.filter(|_| backing.is_none() && (
            prefer_ack || (priority_data.is_none() && !yield_to_data)
        ));
        let done_before_ack = self.intent_request_ack.is_some()
            .then(|| self.receive.iter().position(|rx| rx.done_pending)).flatten()
            .filter(|_| !prefer_ack && priority_data.is_none());
        // Intervene only when the ordinary grant arbitration would let
        // this OACK through. A still-pending backing advertisement can
        // supply the buffer, and its positive ACK remains ordered after it.
        #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
        let prime_target = if !prefer_ack && ack.is_none() && done_before_ack.is_none()
            && priority_wrq_done.is_none() && priority_registration_done.is_none()
        {
            self.server_check_prime_target(priority_data.or_else(|| self.next_transmit()),
                #[cfg(feature = "sunfish-first-oack-spare-buffer")] ordinary)
        } else {
            None
        };
        #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
        let (prime, prime_release) = if let Some(transfer) = prime_target {
            let existing = backing.or_else(|| self.receive[..RX_SLOTS].iter()
                .position(|rx| rx.state == RxState::Advertising));
            let offered = existing.or_else(|| {
                let id = self.offer_receive(generation).ok()?;
                self.receive[..RX_SLOTS].iter().position(|rx|
                    rx.state == RxState::Advertising && rx.id == id)
            });
            let release = if offered.is_none()
                && self.server_check_resources[transfer].prime_released.is_none()
                && self.next_local_intent.checked_add(1).is_some()
            {
                self.receive[..RX_SLOTS].iter().position(|rx|
                    rx.state == RxState::Control && rx.done_pending)
            } else {
                None
            };
            (offered.map(|rx| (transfer, rx)), release.map(|rx| (transfer, rx)))
        } else {
            (None, None)
        };
        // A repeat that took the replenished slot can return that same
        // physical slot once. Only preempt the exact original OACK.
        #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
        let repeat_release = if prime_target.is_some() && prime.is_none() && prime_release.is_none()
            && self.next_local_intent.checked_add(1).is_some()
            && self.oack_slot_follow.is_some_and(|trace| trace.exact_repeat_done_at.is_none())
        {
            self.exact_followed_wrq().map(|(slot, _)| slot)
                .filter(|&slot| self.receive[slot].state == RxState::Control
                    && self.receive[slot].done_pending)
        } else {
            None
        };
        #[cfg(not(feature = "sunfish-tftp-oack-repeat-turn"))]
        let repeat_release = None;
        #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
        let advertisement = prime.map(|(_, rx)| rx).or(advertisement);
        #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
        let oack_release = prime_release.map(|(_, rx)| rx);
        #[cfg(not(feature = "sunfish-tftp-oack-prime-rx"))]
        let oack_release = None;
        // A full-pool denial may borrow one writer attempt for a completion
        // that releases a wire slot without waiting for a service reply.
        #[cfg(feature = "sunfish-glink-denial-rxdone-turn")]
        let denial_done = if prefer_ack && self.retry_full_intent
            && priority_data.is_none() && self.next_local_intent.checked_add(1).is_some()
        {
            self.receive[..RX_SLOTS].iter().position(|rx|
                rx.state == RxState::Control && rx.done_pending)
        } else {
            None
        };
        #[cfg(not(feature = "sunfish-glink-denial-rxdone-turn"))]
        let denial_done = None;
        #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
        if followup.is_some() {
            let candidate = priority_data.or_else(|| self.next_transmit());
            let port = |index: usize| self.transmit[index].kind.qrtr_addresses().map(|(_, peer)| peer.port);
            let peer = self.server_check_49();
            let target = peer.and_then(|target| self.transmit.iter().position(|tx|
                tx.len != 0 && tx.server_check_wrq == Some(target.intent)));
            let slots = self.receive_snapshot().states;
            let gates = u32::from(prefer_ack)
                | (u32::from(self.intent_request_ack == Some(false)) << 1)
                | (u32::from(self.intent_request_ack == Some(true)) << 2)
                | (u32::from(backing.is_some()) << 3)
                | (u32::from(yield_to_data) << 4)
                | (u32::from(done_before_ack.is_some()) << 5)
                | (u32::from(priority_wrq_done.is_some()) << 6)
                | (u32::from(priority_registration_done.is_some()) << 7)
                | (u32::from(priority_data.is_some()) << 8)
                | (u32::from(prime_target.is_some()) << 9)
                | (u32::from(prime.is_some()) << 10)
                | (u32::from(target.is_some()) << 11)
                | (u32::from(target.is_some_and(|index|
                    self.ready_remote_for_transmit(&self.transmit[index]).is_some())) << 12)
                | (u32::from(peer.is_some_and(|trace| trace.first_close_at.is_some())) << 13)
                | (u32::from(candidate.is_some_and(|(index, _)| target == Some(index))) << 14)
                | (u32::from(ack.is_some()) << 15);
            let candidate = candidate.and_then(|(index, _)| port(index));
            if let Some(trace) = self.oack_slot_follow.as_mut() {
                trace.followup_choice = 0; // No record selected yet.
                trace.followup_candidate = candidate;
                trace.followup_selected = None;
                trace.followup_gates = gates;
                trace.followup_slots = slots;
            }
        }
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        if ordinary && let Some(oack) = self.reserved_oack.as_mut()
            && oack.retry_state == 2
        {
            oack.credit_pass_at.get_or_insert_with(|| self.intent_ack_clock.map_or(0, |clock| clock()));
        }
        let (commit, published, size) = if let Some(index) = priority_wrq_done
            .or(priority_registration_done).or(denial_done).or(done_before_ack).or(oack_release).or(repeat_release)
        {
            let id = self.receive[index].id;
            glink_header(&mut record, 6, self.config.local_channel, id);
            (Commit::Done(index), Published::ReceiveDone(id), 8)
        } else if let Some(index) = advertisement {
            let id = self.receive[index].id;
            glink_header(&mut record, 5, self.config.local_channel, 1);
            put(&mut record, 8, MAX_DATAGRAM_BYTES as u32);
            put(&mut record, 12, id);
            (Commit::Advertise(index), Published::ReceiveIntent(id), 16)
        } else if let Some(granted) = ack {
            glink_header(
                &mut record,
                8,
                self.config.local_channel,
                u32::from(granted),
            );
            (
                Commit::IntentAck,
                Published::IntentRequestAck { granted },
                8,
            )
        } else if let Some(index) = self.receive.iter().position(|rx| rx.done_pending).filter(|_| {
            if priority_data.is_some() { return false; }
            #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
            { !self.receive_done_since_data || self.next_transmit().is_none() }
            #[cfg(not(feature = "sunfish-glink-tx-fairness-experiment"))]
            { true }
        }) {
            let id = self.receive[index].id;
            glink_header(&mut record, 6, self.config.local_channel, id);
            (Commit::Done(index), Published::ReceiveDone(id), 8)
        } else if let Some((index, remote)) = priority_data.or_else(|| self.next_transmit()) {
            let tx = &self.transmit[index];
            let id = self.remote[remote].ok_or(Error::InvalidIntent)?.id;
            let chunk = (tx.len - tx.sent).min(TX_CHUNK_BYTES);
            let remaining = tx.len - tx.sent - chunk;
            glink_header(
                &mut record,
                if tx.sent == 0 { 9 } else { 12 },
                self.config.local_channel,
                id,
            );
            put(&mut record, 8, chunk as u32);
            put(&mut record, 12, remaining as u32);
            record[16..16 + chunk].copy_from_slice(&tx.bytes[tx.sent..tx.sent + chunk]);
            (
                Commit::Data(index, remote, chunk),
                Published::Data {
                    remote_intent: id,
                    last_fragment: remaining == 0,
                },
                align(16 + chunk, 8),
            )
        } else {
            #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
            if ordinary && let Some(oack) = self.reserved_oack.as_mut() && oack.retry_state == 2 {
                oack.credit_counts[5] = oack.credit_counts[5].saturating_add(1);
            }
            #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
            self.observe_elf_oack_publish(elf_oack_started, None, PublishSelection::Idle, Ok(()));
            #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
            self.observe_registration_write(registration_target, registration_started, None, PublishSelection::Idle, &[], Ok(()));
            #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
            self.observe_registration_done_turn(0, Ok(()));
            #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
            self.observe_registry_publication(PublishChoice::Idle, false);
            return if self
                .transmit
                .iter()
                .any(|tx| tx.len != 0 && tx.sent < tx.len)
            {
                Err(Error::NoRemoteIntent)
            } else {
                Ok(None)
            };
        };
        #[cfg(feature = "sunfish-post-reg-oack-turn")]
        if let Some(i) = post_reg_turn {
            let tag = match &commit {
                Commit::Data(slot, ..) if self.is_unpublished_first_oack(&self.transmit[*slot]) => 0x100,
                Commit::Data(slot, ..) if matches!(self.transmit[*slot].kind, TxKind::Resume) => 0x200,
                _ => 0,
            };
            let turn = &mut self.post_reg_turns.turns[i];
            turn.command = tag | u16::from_le_bytes([record[0], record[1]]);
            turn.result = 1;
        }
        // These extra attempts may only publish the exact pair selected
        // by the ordinary arbiter. Pending grants and other work keep
        // their normal priority on the next receive/write iteration.
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        let prearm_choice = self.observe_prearm_choice(&record[..size], match &commit {
            Commit::Data(slot, ..) => match self.transmit[*slot].kind { TxKind::Reply { route, .. } => Some(route.transaction), _ => None }, _ => None,
        });
        #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
        let conversion_choice = self.observe_conversion_choice(&record[..size], match &commit {
            Commit::Data(slot, ..) => match self.transmit[*slot].kind {
                TxKind::Reply { route, .. } => Some(route.transaction), _ => None,
            }, _ => None,
        });
        #[cfg(feature = "sunfish-tftp-oack-followup")]
        if let Some(stage) = followup {
            #[cfg(feature = "sunfish-tftp-oack-followup-reason")]
            {
                let (choice, selected) = match &commit {
                    Commit::Advertise(index) => (1, Some(self.receive[*index].id)),
                    Commit::IntentAck => (2, self.intent_request_ack.map(u32::from)),
                    Commit::Done(index) => (3, Some(self.receive[*index].id)),
                    Commit::Data(index, ..) => (4, self.transmit[*index].kind.qrtr_addresses()
                        .map(|(_, peer)| peer.port)),
                };
                #[cfg(feature = "sunfish-first-oack-grant-window")]
                let original = matches!(stage, OackFollowup::FirstGrantAd | OackFollowup::FirstGrantInit | OackFollowup::FirstGrantAck | OackFollowup::FirstGrantReply);
                #[cfg(not(feature = "sunfish-first-oack-grant-window"))]
                let original = false;
                if !original && let Some(trace) = self.oack_slot_follow.as_mut() {
                    trace.followup_choice = choice;
                    trace.followup_selected = selected;
                }
            }
            #[cfg(feature = "sunfish-tftp-oack-credit-early")]
            if matches!(stage, OackFollowup::Reply)
                && self.reserved_oack.is_some_and(|oack| oack.retry_state == 4 && oack.retry_result == 0)
            {
                let (choice, id) = match &commit {
                    Commit::Advertise(index) => (1, self.receive[*index].id),
                    Commit::IntentAck => (2, self.intent_request_ack.map_or(u32::MAX, u32::from)),
                    Commit::Done(index) => (3, self.receive[*index].id),
                    Commit::Data(index, remote, _) => (4,
                        self.transmit[*index].kind.qrtr_addresses().map_or(
                            self.remote[*remote].map_or(u32::MAX, |intent| intent.id),
                            |(_, peer)| peer.port)),
                };
                let tx = &self.transmit[TX_SLOTS];
                let gates = u32::from(self.intent_request_ack.is_some())
                    | (u32::from(priority_wrq_done.is_some()) << 1)
                    | (u32::from(priority_registration_done.is_some()) << 2)
                    | (u32::from(self.next_transmit_in_order([1]).is_some()) << 3)
                    | (u32::from(self.ready_remote_for_transmit(tx).is_some()) << 4)
                    | (u32::from(tx.len != 0 && tx.sent == 0) << 5)
                    | (u32::from(priority_data.is_some()) << 6)
                    | (u32::from(backing.is_some()) << 7)
                    | (u32::from(oack_release.is_some()) << 8)
                    | (u32::from(repeat_release.is_some()) << 9);
                if let Some(oack) = self.reserved_oack.as_mut() {
                    oack.retry_choice = choice;
                    oack.retry_choice_id = id;
                    oack.retry_gates = gates;
                }
            }
            #[cfg(feature = "sunfish-tftp-after-done-trace")]
            if matches!(stage, OackFollowup::Reply)
                && self.reserved_oack.is_some_and(|oack| oack.after_done_state == 3
                    && oack.after_done_result == 0)
            {
                let (choice, id) = match &commit {
                    Commit::Advertise(index) => (1, self.receive[*index].id),
                    Commit::IntentAck => (2, self.intent_request_ack.map_or(u32::MAX, u32::from)),
                    Commit::Done(index) => (3, self.receive[*index].id),
                    Commit::Data(index, remote, _) => (4,
                        self.transmit[*index].kind.qrtr_addresses().map_or(
                            self.remote[*remote].map_or(u32::MAX, |intent| intent.id),
                            |(_, peer)| peer.port)),
                };
                let tx = &self.transmit[TX_SLOTS];
                let gates = u32::from(self.intent_request_ack.is_some())
                    | (u32::from(priority_wrq_done.is_some()) << 1)
                    | (u32::from(priority_registration_done.is_some()) << 2)
                    | (u32::from(self.next_transmit_in_order([1]).is_some()) << 3)
                    | (u32::from(self.ready_remote_for_transmit(tx).is_some()) << 4)
                    | (u32::from(tx.len != 0 && tx.sent == 0) << 5)
                    | (u32::from(priority_data.is_some()) << 6)
                    | (u32::from(backing.is_some()) << 7)
                    | (u32::from(oack_release.is_some()) << 8)
                    | (u32::from(repeat_release.is_some()) << 9)
                    | (u32::from(prime.is_some()) << 10);
                if let Some(oack) = self.reserved_oack.as_mut() {
                    oack.after_done_choice = choice;
                    oack.after_done_choice_id = id;
                    oack.after_done_gates = gates;
                }
            }
            let allowed = match (stage, &commit) {
                #[cfg(feature = "sunfish-paired-oack-init")]
                (OackFollowup::FirstGrantAd, Commit::Advertise(index)) =>
                    self.first_oack_grant_trigger() == Some(Published::ReceiveIntent(self.receive[*index].id)),
                #[cfg(feature = "sunfish-paired-oack-init")]
                (OackFollowup::FirstGrantInit, Commit::Data(slot, _, chunk)) => {
                    let tx = &self.transmit[*slot];
                    self.queued_ipa_init_slot() == Some(*slot)
                        && self.original_1f_close_count() == Some(0)
                        && self.registration_reply_in_flight()
                        && self.intent_request_ack.is_none()
                        && self.next_transmit_in_order([1]).is_none()
                        && tx.sent == 0 && *chunk == tx.len
                        && !self.unsent_flow_predecessor(tx)
                        && self.ready_remote_for_transmit(tx).is_some()
                }
                #[cfg(feature = "sunfish-first-oack-grant-window")]
                (OackFollowup::FirstGrantAck, Commit::IntentAck) =>
                    self.first_oack_grant_trigger() == Some(Published::IntentRequestAck { granted: true }),
                #[cfg(feature = "sunfish-first-oack-grant-window")]
                (OackFollowup::FirstGrantReply, Commit::Data(slot, _, chunk)) => {
                    let tx = &self.transmit[*slot];
                    self.holds_first_oack(tx) && self.can_attempt_first_oack(tx)
                        && *chunk == tx.len && self.ready_remote_for_transmit(tx).is_some()
                }
                #[cfg(any(feature = "sunfish-tftp-oack-credit-early", all(test, feature = "sunfish-tftp-oack-credit-retry")))]
                (OackFollowup::CreditReplay, Commit::Data(index, _, chunk)) =>
                    replay_credit && matches!(self.transmit[*index].kind, TxKind::Resume)
                        && self.transmit[*index].sent == 0 && *chunk == self.transmit[*index].len,
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                (OackFollowup::EarlyReserve, Commit::Data(index, _, chunk)) =>
                    early_reserve && *index == TX_SLOTS && self.transmit[*index].sent == 0
                        && *chunk == self.transmit[*index].len,
                (OackFollowup::Advertise, Commit::Advertise(index)) => {
                    let targeted = prime.is_some_and(|(_, slot)| slot == *index);
                    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
                    let ordinary_ad_allowed = backing.is_none_or(|slot| slot == *index);
                    #[cfg(all(feature = "sunfish-tftp-oack-ordinary-ad", not(feature = "sunfish-tftp-oack-grant-followup")))]
                    let ordinary_ad_allowed = self.intent_request_ack.is_none();
                    #[cfg(feature = "sunfish-tftp-oack-ordinary-ad")]
                    let targeted = targeted || (ordinary_ad_allowed
                        && self.server_check_49().is_some_and(|trace|
                            trace.first_close_at.is_none() && trace.oack_at.is_none()
                                && self.transmit.iter().any(|tx|
                                    tx.len != 0 && tx.sent == 0 && tx.server_check_wrq == Some(trace.intent)
                                        && self.ready_remote_for_transmit(tx).is_some())));
                    targeted
                }
                #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
                (OackFollowup::Grant, Commit::IntentAck) => backing.is_none(),
                (OackFollowup::Reply, Commit::Data(index, _, chunk)) => {
                    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
                    if self.intent_request_ack.is_some() { return Ok(None); }
                    let tx = &self.transmit[*index];
                    self.server_check_49().is_some_and(|trace|
                        trace.first_close_at.is_none() && trace.oack_at.is_none()
                            && tx.server_check_wrq == Some(trace.intent)
                            && tx.sent == 0 && *chunk == tx.len)
                }
                _ => false,
            };
            #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
            if matches!(stage, OackFollowup::EarlyReserve)
                && let Some(oack) = self.reserved_oack.as_mut()
            {
                oack.early_command = record[0];
                oack.early_guards |= u16::from(matches!(commit, Commit::Data(TX_SLOTS, _, _))) << 11;
                oack.early_guards |= u16::from(allowed) << 12;
            }
            #[cfg(feature = "sunfish-tftp-oack-guard")]
            if matches!(stage, OackFollowup::Advertise) {
                let peer = self.server_check_49();
                let mut snapshot = OackAdvertisementGuard {
                    oack_at: peer.and_then(|trace| trace.oack_at),
                    first_record: None, matching: 0, unsent: 0, ready_unsent: 0,
                    grant_pending: self.intent_request_ack.is_some(),
                    peer_closed: peer.is_some_and(|trace| trace.first_close_at.is_some()),
                    allowed,
                };
                if let Some(peer) = peer {
                    for (slot, tx) in self.transmit.iter().enumerate()
                        .filter(|(_, tx)| tx.len != 0 && tx.server_check_wrq == Some(peer.intent))
                    {
                        snapshot.first_record.get_or_insert(OackGuardRecord {
                            slot: slot as u8, kind: tx.kind.diagnostic_kind(),
                            len: tx.len as u32, sent: tx.sent as u32,
                        });
                        snapshot.matching += 1;
                        if tx.sent == 0 {
                            snapshot.unsent += 1;
                            if self.ready_remote_for_transmit(tx).is_some() {
                                snapshot.ready_unsent += 1;
                            }
                        }
                    }
                }
                if let Some(trace) = self.oack_slot_follow.as_mut() {
                    trace.advertisement_guard = Some(snapshot);
                }
            }
            if !allowed {
                #[cfg(feature = "sunfish-tftp-reserved-writer-trace")]
                {
                    let choice = match &commit {
                        Commit::Advertise(_) => PublishSelection::Advertisement,
                        Commit::IntentAck => PublishSelection::IntentAck,
                        Commit::Done(_) => PublishSelection::ReceiveDone,
                        Commit::Data(index, ..) => PublishSelection::Data(*index),
                    };
                    self.observe_reserved_writer(elf_oack_started, None, choice, None);
                }
                #[cfg(feature="sunfish-elf-writer-trace")]
                self.end_elf_writer_yield();
                return Ok(None);
            }
        }
        // Keep the wire intent only for the existing exact WRQ exception or
        // a fully accepted NEW_SERVER. Retained service requests never qualify.
        #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
        let reuse = if let Commit::Done(index) = &commit {
            #[cfg(feature = "sunfish-glink-announcement-reuse")]
            let announcement = self.receive[*index].state == RxState::Control
                && self.receive[*index].announcement_reuse
                && self.receive[*index].context.is_none() && self.receive[*index].ticket.is_none();
            #[cfg(not(feature = "sunfish-glink-announcement-reuse"))]
            let announcement = false;
            if priority_wrq_done == Some(*index) || announcement {
                let id = self.next_local_intent;
                let next = id.checked_add(1).ok_or(Error::IdExhausted)?;
                glink_header(&mut record, crate::wlfw_protocol::GLINK_CMD_RX_DONE_W_REUSE,
                    self.config.local_channel, self.receive[*index].id);
                Some((id, next))
            } else { None }
        } else { None };
        // An unwinding host writer is also an uncertain publication. Leave the
        // guard set unless it returns one of the explicit known outcomes.
        self.quarantined = true;
        #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
        let elf_oack_writer = self.oack_publish_clock();
        #[cfg(feature = "sunfish-ipa-init-tx-trace")]
        let init_writer = init_target.and(self.intent_ack_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
        let registration_writer = registration_target.and(self.intent_ack_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
        if matches!(&commit, Commit::Data(slot, ..)
            if priority_registration.is_some_and(|(target, _)| target == *slot))
        {
            self.mark_registration_priority_attempt();
        }
        #[cfg(feature = "sunfish-ipa-registration-rxdone-turn")]
        if matches!(&commit, Commit::Done(index) if priority_registration_done == Some(*index)) {
            self.mark_registration_done_attempt();
        }
        #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
        if let Commit::Data(slot, ..) = &commit {
            let tx = &self.transmit[*slot];
            if !self.in_wrq_return_writer() && self.holds_first_oack(tx) && self.can_attempt_first_oack(tx)
                && let Some(intent) = tx.server_check_wrq
                && let Some(first) = self.server_check_trace_for_intent(intent)
            {
                // Consume on the writer call, including WouldBlock. A later
                // ordinary retry still requires the matching IPA completion.
                first.early_oack_attempted = true;
            }
        }
        #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
        if first_oack_ready {
            self.first_oack_eligibility.ready_command = Some(u16::from_le_bytes([record[0], record[1]]));
        }
        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
        let borrowed_credit = converted_credit.filter(|(_, credit, _)|
            matches!(&commit, Commit::Data(actual, ..) if actual == credit));
        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
        if let Some((registry, credit, _)) = borrowed_credit {
            // One attempt per actual registry transmit, including blocked or
            // uncertain outcomes. Never lend the same conversion twice.
            self.transmit[registry].registry_credit_turn_used = true;
            self.transmit[registry].registry_publication.mask |= 1 << 17;
            if let Some(first) = self.registry_window.target.as_mut().and_then(|p| p.conversion.first.as_mut())
                .filter(|t| t.active)
            {
                let bytes = &self.transmit[credit].bytes;
                let pack = |a: u32, b: u32| (a <= 65535 && b <= 65535).then_some((a << 16) | b);
                first.credit_ports = pack(word(bytes, 12), word(bytes, 28));
                first.credit_nodes = pack(word(bytes, 8), word(bytes, 24));
            }
            self.observe_conversion_exit(5);
            self.registry_converted_grant = None;
        }
        #[cfg(feature = "sunfish-registry-between-grants")]
        let borrowed_registry_slot = if registry_before_grant {
            priority_registry.filter(|(slot, _)| matches!(&commit, Commit::Data(actual, ..) if slot == actual))
                .map(|(slot, _)| slot)
        } else { None };
        #[cfg(feature = "sunfish-registry-between-grants")]
        if let Some(slot) = borrowed_registry_slot {
            self.transmit[slot].registry_grant_turn_used = true;
            self.transmit[slot].registry_publication.mask |= 1 << 8;
        }
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        let converted_slot = borrowed_registry_slot.filter(|slot|
            converted_registry.is_some_and(|(target, _)| target == *slot));
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        if borrowed_registry_slot.is_some() {
            // Consume before the checked writer, including WouldBlock or an
            // unwind. A different reply cannot inherit this grant opportunity.
            #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
            self.observe_conversion_exit(1);
            self.registry_converted_grant = None;
            if let Some(slot) = converted_slot { self.transmit[slot].registry_publication.mask |= 1 << 11; }
        }
        #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
        self.observe_conversion_callback(conversion_choice);
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        self.observe_prearm_result(prearm_choice, 4);
/* READY_HANDOFF_BEGIN */
        #[cfg(feature="sunfish-registry-ready-handoffs")]
        if let Some((i,_))=ready_registry.filter(|(i,_)|borrowed_registry_slot==Some(*i)){
            self.transmit[i].registry_publication.mask|=1<<19;
        }
/* READY_HANDOFF_END */
        let result = writer(&record[..size]);
/* READY_HANDOFF_BEGIN */
        #[cfg(feature="sunfish-registry-ready-handoffs")]
        if result.is_ok() && let Some((i,_))=ready_registry.filter(|(i,_)|borrowed_registry_slot==Some(*i)){
            self.transmit[i].registry_publication.mask|=1<<20;
        }
/* READY_HANDOFF_END */
        #[cfg(feature="sunfish-elf-writer-trace")]
        self.elf_writer_header(&record[..size],result);
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        self.observe_prearm_result(prearm_choice, match result { Ok(()) => 1, Err(WriteError::WouldBlock) => 2, Err(WriteError::Uncertain) => 3 });
        #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
        if conversion_choice && let Some(choice) = self.registry_window.target.as_mut()
            .and_then(|p| p.conversion.first.as_mut()).and_then(|t| t.choice.as_mut())
        { choice.result = match result { Ok(()) => 1, Err(WriteError::WouldBlock) => 2, Err(WriteError::Uncertain) => 3 }; }
        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
        if result.is_ok() && let Some((registry, _, _)) = borrowed_credit {
            self.transmit[registry].registry_publication.mask |= 1 << 18;
        }
        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
        if result.is_ok() && let Some(slot) = converted_slot {
            self.transmit[slot].registry_publication.mask |= 1 << 12;
        }
        #[cfg(feature = "sunfish-registry-between-grants")]
        if result.is_ok() && let Some(slot) = borrowed_registry_slot {
            self.transmit[slot].registry_publication.mask |= 1 << 9;
        }
        #[cfg(feature = "sunfish-post-reg-oack-turn")]
        if let Some(i) = post_reg_turn {
            self.post_reg_turns.turns[i].written(u16::from_le_bytes([record[0], record[1]]), result);
        }
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        if ordinary && let Some(oack) = self.reserved_oack.as_mut() && oack.retry_state == 2 {
            let category = match &commit {
                Commit::Advertise(_) => 0,
                Commit::IntentAck => 1,
                Commit::Done(_) => 2,
                Commit::Data(index, _, _) if matches!(self.transmit[*index].kind, TxKind::Resume) => 3,
                Commit::Data(..) => 4,
            };
            oack.credit_counts[category] = oack.credit_counts[category].saturating_add(1);
            if result == Err(WriteError::WouldBlock) {
                oack.credit_counts[6] = oack.credit_counts[6].saturating_add(1);
            }
        }
        #[cfg(feature = "sunfish-glink-denial-rxdone-turn")]
        if matches!((&commit, result), (Commit::Done(index), Err(WriteError::WouldBlock))
            if denial_done == Some(*index))
        {
            // Preserve the pending negative ACK and give it the next normal turn.
            self.retry_full_intent = false;
        }
        #[cfg(feature = "sunfish-ipa-registration-rxdone-trace")]
        {
            let turn = match commit {
                Commit::Advertise(_) => 1,
                Commit::IntentAck => 2,
                Commit::Done(index) => {
                    if self.ipa_registration_trace().is_some_and(|trace|
                        trace.request.intent == self.receive[index].id) { 7 } else { 3 }
                }
                Commit::Data(slot, ..) if matches!(self.transmit[slot].kind, TxKind::Resume) => 4,
                Commit::Data(..) => 5,
            };
            self.observe_registration_done_turn(turn, result);
        }
        #[cfg(feature = "sunfish-ipa-init-writer-counts")]
        self.observe_ipa_schedule_command(&record[..size], result);
        #[cfg(feature = "sunfish-ipa-init-tx-turn")]
        if let Commit::Data(slot, ..) = &commit
            && priority_init.is_some_and(|(target, _)| target == *slot)
        {
            self.observe_ipa_init_priority(result);
        }
        #[cfg(any(feature = "sunfish-wlan-elf-oack-queue-trace", feature = "sunfish-ipa-init-tx-trace"))]
        let choice = match &commit {
            Commit::Advertise(_) => PublishSelection::Advertisement,
            Commit::IntentAck => PublishSelection::IntentAck,
            Commit::Done(_) => PublishSelection::ReceiveDone,
            Commit::Data(index, ..) => PublishSelection::Data(*index),
        };
        #[cfg(feature = "sunfish-ipa-registration-tx-trace")]
        self.observe_registration_write(registration_target, registration_started, registration_writer,
            choice, &record[..size], result);
        #[cfg(feature = "sunfish-wlan-elf-oack-queue-trace")]
        self.observe_elf_oack_publish(elf_oack_started, elf_oack_writer, choice, result);
        #[cfg(feature = "sunfish-ipa-init-tx-trace")]
        if let Some(target) = init_target {
            self.observe_ipa_init_writer(target, &choice, init_writer, result);
        }
        #[cfg(feature = "sunfish-wlan-elf-publish-trace")]
        if !matches!(result, Err(WriteError::Uncertain)) {
            let choice = match &commit {
                Commit::Data(slot, ..) => PublishChoice::Data(*slot),
                _ => PublishChoice::Control,
            };
            self.observe_registry_publication(choice, matches!(result, Err(WriteError::WouldBlock)));
        }
        #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
        if self.intent_request_ack.is_some() {
            let event = match (&commit, &result) {
                (Commit::IntentAck, Ok(())) => Some(AckObservation::Sent(self.intent_ack_age())),
                (Commit::IntentAck, Err(WriteError::WouldBlock)) => Some(AckObservation::Blocked),
                (Commit::Done(_), Ok(())) => Some(AckObservation::DoneAhead),
                (Commit::Advertise(_), Ok(())) => Some(AckObservation::AdvertisementAhead),
                _ => None,
            };
            if let Some(event) = event {
                self.observe_pending_ack(event);
            }
        }
        match result {
            Err(WriteError::WouldBlock) => {
                #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
                self.note_tftp_writer_blocked();
                self.quarantined = false;
                return Err(Error::WouldBlock);
            }
            Err(WriteError::Uncertain) => {
                return Err(Error::PublicationUncertain);
            }
            Ok(()) => self.quarantined = false,
        }
        #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
        if let Some(oack) = self.reserved_oack.as_mut() {
            match &commit {
                Commit::IntentAck if oack.retry_state == 1 => {
                    oack.retry_state = 2;
                    oack.retry_ack_at = self.intent_ack_clock.map(|clock| clock());
                }
                Commit::Data(index, _, _) if oack.retry_state == 2
                    && matches!(self.transmit[*index].kind, TxKind::Resume) => {
                    oack.retry_state = 3;
                    oack.retry_credit_at = self.intent_ack_clock.map(|clock| clock());
                }
                _ => {}
            }
        }
        #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
        if let Commit::Done(index) = &commit
            && let Some(oack) = self.reserved_oack.as_mut()
            && oack.after_done_state == 1
            && self.receive[*index].id == oack.retry_choice_id
        {
            oack.after_done_state = 2;
            oack.after_done_at = self.intent_ack_clock.map(|clock| clock());
            #[cfg(feature = "sunfish-tftp-after-done-trace")]
            { oack.after_done_origin = if ordinary { 1 } else { 2 }; }
        }
        #[cfg(feature = "sunfish-tftp-after-done-trace")]
        if matches!(&commit, Commit::IntentAck)
            && let Some(oack) = self.reserved_oack.as_mut()
            && oack.after_done_state == 2 && oack.after_done_grant_at.is_none()
        {
            oack.after_done_grant_at = self.intent_ack_clock.map(|clock| clock());
        }
        #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
        if matches!(&commit, Commit::Advertise(_) | Commit::IntentAck | Commit::Done(_))
            && self.transmit.iter().any(|tx| matches!(tx.kind, TxKind::TftpReply { .. }) && tx.len != 0 && tx.sent < tx.len) {
            self.tftp_control_turn_due = true;
        }
        #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
        {
            self.registry_turn_used = priority_registry.is_some();
            if let Some((slot, _)) = priority_registry {
                self.transmit[slot].registry_publication.mask |= 1 << 7;
            }
        }
        #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
        { self.intent_ack_yield = matches!(&commit, Commit::IntentAck); }
        match commit {
            Commit::Advertise(index) => {
                #[cfg(feature = "sunfish-registry-converted-grant-turn")]
                if self.registry_converted_grant.is_some_and(|(backing, _)| backing == self.receive[index].id) {
                    #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                    self.observe_conversion_exit(2);
                    self.registry_converted_grant = None;
                }
                self.receive[index].state = RxState::Available;
                #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
                if let Some((transfer, rx)) = prime && index == rx {
                    self.server_check_resources[transfer].primed_intent = Some(self.receive[index].id);
                }
                #[cfg(feature = "sunfish-tftp-oack-resources")]
                self.observe_oack_intent_available(self.receive[index].id);
                #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
                if self.intent_ack_backing == Some(self.receive[index].id) {
                    #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                    if self.grant_backing.intent == self.intent_ack_backing {
                        self.grant_backing.writer = Some(match followup {
                            None => GrantAdWriter::Ordinary,
                            Some(OackFollowup::FirstGrantAd) => GrantAdWriter::Registration,
                            Some(_) => GrantAdWriter::OtherFollowup,
                        });
                        self.grant_backing.advertised_at = self.intent_ack_clock.map(|now| now());
                        #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                        if let Some(first) = self.registry_window.target.as_mut().and_then(|p| p.conversion.first.as_mut())
                            && first.intent == self.receive[index].id && !first.advertised
                        { first.advertised = true; first.advertised_at = self.grant_backing.advertised_at; }
                        #[cfg(feature = "sunfish-registry-post-reply-trace")]
                        if let Some(grant) = self.registry_window.predecessor.as_mut().and_then(|p| p.grant.as_mut())
                            && grant.intent == self.receive[index].id && !grant.advertised
                        { grant.advertised = true; grant.advertised_at = self.grant_backing.advertised_at; }
                    }
                    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
                    self.follow_oack_slot_available(index, 2);
                    self.intent_ack_backing = None;
                }
                #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
                self.follow_oack_slot_available(index, 1);
            },
            Commit::IntentAck => {
                #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                self.observe_conversion_exit(3);
                #[cfg(feature = "sunfish-registry-converted-grant-turn")]
                { self.registry_converted_grant = None; }
                #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
                if let Some((requested, request)) = self.pending_denial_slots.take()
                    && self.intent_request_ack == Some(false)
                    && self.ipa_registration_trace().is_some_and(|trace| trace.completed_at.is_none())
                {
                    self.last_denial_window = Some(IntentDenialWindow {
                        requested, request, ack: self.intent_denial_slots(),
                    });
                }
                #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
                {
                    let age = self.intent_ack_age();
                    let (count, maximum) = &mut self.intent_ack_summary;
                    *count = count.saturating_add(1);
                    *maximum = (*maximum).max(age);
                }
                #[cfg(feature = "sunfish-registry-post-reply-trace")]
                if self.intent_request_ack == Some(true)
                    && let Some(grant) = self.registry_window.predecessor.as_mut().and_then(|p| p.grant.as_mut())
                    && self.grant_backing.intent == Some(grant.intent) && grant.advertised && !grant.acked
                { grant.acked = true; grant.ack_at = self.intent_ack_clock.map(|now| now()); }
                #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                if self.intent_request_ack == Some(true)
                    && let Some(first) = self.registry_window.target.as_mut().and_then(|p| p.conversion.first.as_mut())
                    && self.grant_backing.intent == Some(first.intent) && first.advertised && !first.acked
                { first.acked = true; first.ack_at = self.intent_ack_clock.map(|now| now()); }
                self.intent_request_ack = None;
                #[cfg(feature = "sunfish-glink-announcement-reuse")]
                { self.intent_ack_reused = None; }
                #[cfg(feature = "sunfish-glink-retry-full-intent")]
                { self.retry_full_intent = false; }
                #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
                { self.intent_ack_started = None; }
            },
            Commit::Done(index) => {
/* READY_HANDOFF_BEGIN */
                #[cfg(feature="sunfish-registry-ready-handoffs")]
                for tx in &mut self.transmit {
                    if tx.len!=0 && tx.sent==0 && matches!(tx.kind,TxKind::Reply{receive,route,..}
                        if receive==index && route.service==Service::ServiceRegistry && route.transaction==3
                            && route.source.port==crate::qcom_servreg::LOCAL_PORT
                            && route.destination==(Address{node:0,port:9})) {tx.registry_return_turn_due=true;}
                }
/* READY_HANDOFF_END */
                #[cfg(feature = "sunfish-registry-prearm-io-trace")]
                if let Some(pair) = self.registry_window.target.as_mut()
                    && !pair.prearm.own_done_seen
                    && pair.newer == (RequestHandle { owner: self.owner, generation: self.generation, intent: self.receive[index].id })
                { pair.prearm.own_done_seen = true; pair.prearm.own_done_at = self.intent_ack_clock.map(|now| now()); }
                #[cfg(feature = "sunfish-ack-elf-phase-trace")]
                {
                    let request = TftpHandle { owner: self.owner, generation: self.generation,
                        intent: self.receive[index].tftp_datagram_id() };
                    let clock = self.intent_ack_clock;
                    for write in self.server_check_traces_mut() {
                        if let Some(data) = write.data.as_mut().filter(|d| d.request == request)
                            && data.stages.returned_at.is_none()
                        { data.stages.returned_at = clock.map(|clock| clock()); }
                    }
                }
                #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
                if repeat_release == Some(index) && let Some(trace) = self.oack_slot_follow.as_mut() {
                    trace.exact_repeat_done_at = self.intent_ack_clock.map(|clock| clock());
                }
                #[cfg(feature = "sunfish-tftp-oack-prime-rx")]
                if let Some((transfer, rx)) = prime_release && index == rx {
                    self.server_check_resources[transfer].prime_released = Some(self.receive[index].id);
                    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
                    if self.oack_slot_follow.is_none() {
                        self.oack_slot_follow = Some(OackSlotFollow {
                            slot: index, released: self.receive[index].id,
                            released_at: self.intent_ack_clock.map(|clock| clock()),
                            ..OackSlotFollow::default()
                        });
                    }
                }
                #[cfg(feature = "sunfish-ipa-registration-tx-turn")]
                self.note_registration_request_done(self.receive[index].id);
                #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
                if let Some(wrq) = self.receive[index].server_check_wrq {
                    let clock = self.intent_ack_clock;
                    if let Some(trace) = self.server_check_trace_for_intent(wrq.intent) {
                        trace.wrq_done_at = clock.map(|clock| clock());
                        #[cfg(feature = "sunfish-wlan-first-wrq-rxdone-turn")]
                        { trace.wrq_priority_published = priority_wrq_done == Some(index); }
                        #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
                        { trace.wrq_reused = reuse.is_some(); }
                    }
                }
                #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
                { self.receive_done_since_data = true; }
                self.receive[index].done_pending = false;
                #[cfg(feature = "sunfish-glink-registry-early-release")]
                if matches!(self.receive[index].state, RxState::Request | RxState::ReplyQueued)
                    && self.receive[index].context.is_some_and(|context| context.service == Service::ServiceRegistry)
                    && let Some(spare) = (RX_SLOTS..REQUEST_SLOTS).find(|&slot| self.receive[slot].state == RxState::Free)
                {
                    // The spare entry owns the request and ticket but has no
                    // wire intent. Keep a queued reply bound to that entry.
                    self.receive.swap(index, spare);
                    for tx in &mut self.transmit {
                        if let TxKind::Reply { receive, .. } = &mut tx.kind
                            && *receive == index
                        {
                            *receive = spare;
                        }
                    }
                    self.receive[index].state = RxState::Control;
                }
                if self.receive[index].state == RxState::Control {
                    let receive = Receive::empty();
                    #[cfg(feature = "sunfish-wlan-first-wrq-reuse")]
                    let receive = if let Some((datagram_id, next)) = reuse {
                        self.next_local_intent = next;
                        Receive {
                            state: RxState::Available,
                            id: self.receive[index].id,
                            reused_datagram_id: Some(datagram_id),
                            #[cfg(feature = "sunfish-glink-announcement-reuse")]
                            announcement_reuse: self.receive[index].announcement_reuse,
                            ..receive
                        }
                    } else {
                        receive
                    };
                    self.receive[index] = receive;
                    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
                    if self.receive[index].state == RxState::Available {
                        self.follow_oack_slot_available(index, 3);
                    }
                    #[cfg(feature = "sunfish-tftp-oack-resources")]
                    if self.receive[index].state == RxState::Available {
                        self.observe_oack_intent_available(self.receive[index].id);
                    }
                    // A pending full-pool denial may use this exact return only
                    // after RX_DONE_W_REUSE committed. Its original advertisement
                    // and this return already precede the eventual positive ACK.
                    #[cfg(all(feature = "sunfish-glink-announcement-reuse", feature = "sunfish-glink-retry-full-intent"))]
                    if self.retry_full_intent && self.intent_request_ack == Some(false)
                        && self.receive[index].state == RxState::Available
                        && self.receive[index].announcement_reuse
                    {
                        self.intent_request_ack = Some(true);
                        self.intent_ack_reused = Some(self.receive[index].id);
                        self.retry_full_intent = false;
                    }
                    // Reserve the slot after RX_DONE commits, before another
                    // publisher pass can use it for a routine advertisement.
                    #[cfg(feature = "sunfish-glink-retry-full-intent")]
                    if self.retry_full_intent && self.intent_request_ack == Some(false)
                        && self.receive[index].state == RxState::Free
                        && self.next_local_intent.checked_add(1).is_some()
                    {
                        let id = self.offer_receive(generation)?;
                        self.intent_ack_backing = Some(id);
                        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                        {
                            self.grant_backing.intent = Some(id);
                            self.grant_backing.reserved_at = self.intent_ack_clock.map(|now| now());
                        }
                        self.intent_request_ack = Some(true);
                        self.retry_full_intent = false;
                        #[cfg(feature = "sunfish-registry-converted-grant-turn")]
                        {
                            let candidate = self.next_transmit_in_order([3]);
                            for (slot, tx) in self.transmit.iter_mut().enumerate() {
                                let TxKind::Reply { receive, route, .. } = tx.kind else { continue; };
                                if route.service != Service::ServiceRegistry || tx.sent == tx.len { continue; }
                                tx.registry_publication.mask |= (1 << 13)
                                    | (u32::from(self.receive[receive].done_pending) << 14)
                                    | (u32::from(candidate.is_none_or(|(target, _)| target != slot)) << 15)
                                    | (u32::from(tx.registry_grant_turn_used) << 16);
                            }
                            #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                            self.observe_conversion_exit(4);
                            self.registry_converted_grant = candidate
                                .filter(|(slot, _)| !self.transmit[*slot].registry_grant_turn_used)
                                .map(|(slot, _)| {
                                    let TxKind::Reply { receive, .. } = self.transmit[slot].kind else { unreachable!() };
                                    self.transmit[slot].registry_publication.mask |= 1 << 10;
                                    let request = RequestHandle { owner: self.owner, generation: self.generation,
                                        intent: self.receive[receive].id };
                                    #[cfg(feature = "sunfish-registry-post-reply-trace")]
                                    if let Some(prior) = self.registry_window.predecessor.as_mut()
                                        && prior.request == request && prior.grant.is_none()
                                    {
                                        prior.grant = Some(RegistryConvertedGrantTrace {
                                            intent: id, converted_at: self.grant_backing.reserved_at,
                                            advertised_at: None, ack_at: None, advertised: false, acked: false,
                                        });
                                    }
                                    (id, request)
                                });
                            #[cfg(feature = "sunfish-registry-conversion-lifetime-trace")]
                            if let Some((intent, request)) = self.registry_converted_grant
                                && let Some(pair) = self.registry_window.target.as_mut()
                                && pair.newer == request
                            {
                                pair.conversion.arms = pair.conversion.arms.saturating_add(1);
                                if pair.conversion.first.is_none() && let Some((slot, _)) = candidate {
                                    let tx = &self.transmit[slot];
                                    pair.conversion.first = Some(RegistryConversionTurn {
                                        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
                                        packed_len_sent: (u32::from(tx.registry_publication.bytes) << 16) | tx.sent as u32,
                                        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
                                        length_consistent: tx.len != 0 && tx.len == usize::from(tx.registry_publication.bytes),
                                        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
                                        credit_ports: None,
                                        #[cfg(feature = "sunfish-registry-converted-credit-turn")]
                                        credit_nodes: None,
                                        intent, request_intent: request.intent, bytes: tx.len as u16, sent: tx.sent as u16,
                                        armed_at: self.grant_backing.reserved_at, entry: None, choice: None,
                                        exit: None, advertised_at: None, ack_at: None,
                                        active: true, advertised: false, acked: false,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Commit::Data(index, remote, chunk) => {
                #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
                if self.transmit[index].missing_reply_observed {
                    if let Some(trace) = self.missing_tftp_reply.as_mut() {
                        if self.transmit[index].sent == 0 {
                            trace.flags |= 4;
                            trace.remote_intent = self.remote[remote].as_ref().map_or(u32::MAX, |v| v.id);
                        }
                        if self.transmit[index].sent + chunk == self.transmit[index].len { trace.flags |= 8; }
                    }
                }
                #[cfg(feature = "sunfish-tftp-final-ack-observer")]
                if self.transmit[index].final_ack_observed {
                    if let Some(trace) = self.final_tftp_ack.as_mut() {
                        if self.transmit[index].sent == 0 {
                            trace.flags |= 4;
                            trace.remote_intent = self.remote[remote].as_ref().map_or(u32::MAX, |v| v.id);
                        }
                        if self.transmit[index].sent + chunk == self.transmit[index].len {
                            trace.flags |= 8;
                            #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
                            { trace.published_at = self.intent_ack_clock.map(|clock| clock()); }
                        }
                    }
                }
                #[cfg(feature = "sunfish-tftp-mcfg-trace")]
                if self.mcfg_oack_slot == Some(index) {
                    if let Some(trace) = self.mcfg_oack.as_mut() {
                        if self.transmit[index].sent == 0 {
                            trace.flags |= 4;
                            trace.remote_intent = self.remote[remote].as_ref().map_or(u32::MAX, |v| v.id);
                        }
                        if self.transmit[index].sent + chunk == self.transmit[index].len { trace.flags |= 8; }
                    }
                }
                #[cfg(feature = "sunfish-tftp-endpoint-observer")]
                if self.first_tftp_slot == Some(index) {
                    if let Some(trace) = self.first_tftp_endpoint.as_mut() {
                        if self.transmit[index].sent == 0 {
                            trace.flags |= 2;
                            trace.remote_intent = self.remote[remote].as_ref().map_or(u32::MAX, |v| v.id);
                        }
                        if self.transmit[index].sent + chunk == self.transmit[index].len { trace.flags |= 4; }
                    }
                }
                #[cfg(feature = "sunfish-native-tftp-server-check")]
                if matches!(self.transmit[index].kind, TxKind::TftpReply { .. }) {
                    if self.transmit[index].sent == 0 { self.tftp_tx_progress[0] = self.tftp_tx_progress[0].saturating_add(1); }
                    if self.transmit[index].sent + chunk == self.transmit[index].len { self.tftp_tx_progress[1] = self.tftp_tx_progress[1].saturating_add(1); }
                }
                #[cfg(feature = "sunfish-glink-credit-fairness-experiment")]
                { self.credit_sent_since_data = matches!(self.transmit[index].kind, TxKind::Resume); }
                #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
                {
                    match self.transmit[index].kind {
                        TxKind::TftpReply { .. } => self.other_sent_since_tftp = false,
                        TxKind::Resume => {},
                        _ => self.other_sent_since_tftp = true,
                    }
                }
                #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
                if matches!(self.transmit[index].kind, TxKind::TftpReply { .. }) { self.tftp_control_turn_due = false; }
                #[cfg(feature = "sunfish-glink-tx-fairness-experiment")]
                { self.receive_done_since_data = false; }
                self.remote[remote].as_mut().unwrap().busy = true;
                self.transmit[index].remote_slot = Some(remote);
                self.transmit[index].sent += chunk;
                if self.transmit[index].sent == self.transmit[index].len {
                    #[cfg(feature="sunfish-elf-writer-trace")]
                    self.freeze_elf_writer(index);
                    #[cfg(feature="sunfish-post-elf-mcfg-trace")]
                    self.observe_post_mcfg_publication(index,remote);
/* STARTUP_PREREQ_BEGIN */
#[cfg(feature="sunfish-startup-prerequisites")]
                    self.observe_startup_publication(index,remote);
/* STARTUP_PREREQ_END */
                    #[cfg(feature = "sunfish-registry-handoff-turn")]
                    if let Some(prior) = self.registry_window.predecessor.as_mut()
                        && let TxKind::Reply { receive, .. } = self.transmit[index].kind
                        && prior.request == (RequestHandle { owner: self.owner, generation: self.generation, intent: self.receive[receive].id })
                    {
                        prior.published_at = self.intent_ack_clock.map(|clock| clock());
                        #[cfg(feature = "sunfish-registry-predecessor-pressure")]
                        {
                            prior.publication = Some(self.transmit[index].registry_publication);
                            prior.publication_frozen = true;
                        }
                    }
                    #[cfg(feature = "sunfish-registry-window-trace")]
                    if let Some(pair) = self.registry_window.target.as_mut()
                        && pair.published_before_done.is_none()
                        && let TxKind::Reply { receive, .. } = self.transmit[index].kind
                        && pair.newer == (RequestHandle { owner: self.owner, generation: self.generation, intent: self.receive[receive].id })
                    {
                        #[cfg(feature = "sunfish-registry-post-reply-trace")]
                        {
                            pair.publication = Some(self.transmit[index].registry_publication);
                            pair.publication_frozen = true;
                        }
                        pair.newer_published_at = self.intent_ack_clock.map(|clock| clock());
                        pair.published_before_done = Some(!pair.older_completed);
                    }
                    #[cfg(feature = "sunfish-tftp-oack-reserve")]
                    if index == TX_SLOTS && let Some(oack) = self.reserved_oack.as_mut() {
                        oack.published_at = self.intent_ack_clock.map(|clock| clock());
                        #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
                        {
                            let tx = &self.transmit[index];
                            oack.wire_len = tx.len as u32;
                            oack.wire_intent = self.remote[remote].map_or(u32::MAX, |intent| intent.id);
                            let len = tx.len.min(oack.wire_bytes.len());
                            oack.wire_bytes[..len].copy_from_slice(&tx.bytes[..len]);
                        }
                    }
                    #[cfg(feature = "sunfish-ipa-registration-trace")]
                    self.note_linux_ipa_published(index, remote);
                    #[cfg(feature = "sunfish-wlan-first-wrq-window")]
                    if let Some(after) = self.wrq_window_phase() {
                        let tx = &self.transmit[index];
                        let intent = self.remote[remote].unwrap().id;
                        if let Some(neighbor) = FirstWrqWindow::neighbor(
                            &tx.bytes[..tx.len], 48, self.config.local_node, self.config.peer_node,
                            self.intent_ack_clock, intent, Some(index),
                        ) {
                            self.first_wrq_window.publish(after, neighbor);
                        }
                    }
                    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
                    self.observe_server_check_publish(index);
                    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
                    let clock = self.intent_ack_clock;
                    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
                    for write in self.server_check_traces_mut() {
                        for reply in [&mut write.oack, &mut write.ack] {
                            if reply.slot == Some(index) {
                                reply.phase = 3;
                                reply.published_at = clock.map(|clock| clock());
                            }
                        }
                    }
                    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
                    if let Some(repeat) = self.first_server_check.as_mut().and_then(|write| write.repeat.as_mut())
                        && repeat.oack.slot == Some(index) && repeat.oack.phase == 2
                    {
                        repeat.oack.phase = 3;
                        repeat.oack.published_at = clock.map(|clock| clock());
                    }
                    #[cfg(feature = "sunfish-wlan-elf-host-credit-trace")]
                    if let Some(credit) = self.elf_host_credit.as_mut() {
                        if credit.resume_slot == Some(index) {
                            credit.remote_intent = Some(self.remote[remote].unwrap().id);
                        }
                        if let TxKind::Reply { receive, .. } = self.transmit[index].kind
                            && self.receive[receive].id == credit.request.intent
                        {
                            credit.before_reply = Some([credit.remote_intent.is_some(), credit.completed]);
                        }
                    }
                    #[cfg(feature = "sunfish-wlan-elf-credit-trace")]
                    if let TxKind::Reply { receive, .. } = self.transmit[index].kind
                        && let Some(trace) = self.elf_exchange.as_mut()
                        && self.receive[receive].id == trace.request.intent
                    {
                        trace.credit.published = true;
                    }
                    #[cfg(feature = "sunfish-ipa-ignore-init-response-reference")]
                    self.note_linux_ipa_indication_published(index);
                    if let TxKind::Control(token) = self.transmit[index].kind {
                        self.control.published(token);
                        #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                        self.observe_ipa_endpoint_ready();
                    }
                }
            }
        }
        Ok(Some(published))
    }

    #[cfg(all(test, feature = "sunfish-tftp-oack-ordinary-ad"))]
    pub(crate) fn replay_clear_tftp_writer_turns(&mut self) {
        self.tftp_control_turn_due = false;
        self.credit_sent_since_data = false;
        self.other_sent_since_tftp = false;
    }

    /// Credits have priority, and an unavailable large intent never blocks a
    /// smaller ready exchange. All scans have fixed slot bounds.
    fn next_transmit(&self) -> Option<(usize, usize)> {
        // Class 2 is an optional bounded turn for TFTP only; all other
        // non-credit messages retain their original ordering behind credits.
        #[cfg(all(feature = "sunfish-glink-credit-fairness-experiment", feature = "sunfish-glink-tftp-fairness-experiment"))]
        let order = if self.credit_sent_since_data || self.other_sent_since_tftp { [2, 1, 0] } else { [1, 0, 2] };
        #[cfg(all(feature = "sunfish-glink-credit-fairness-experiment", not(feature = "sunfish-glink-tftp-fairness-experiment")))]
        let order = if self.credit_sent_since_data { [2, 1, 0] } else { [1, 0, 2] };
        #[cfg(not(feature = "sunfish-glink-credit-fairness-experiment"))]
        let order = [1, 0, 2];
        self.next_transmit_in_order(order)
    }

    #[cfg(feature = "sunfish-registry-converted-grant-turn")]
    fn registry_conversion_protected_work(&self) -> (bool, bool) {
        // Check protected work independently of prefer_ack: a conversion
        // normally prefers its backing advertisement, whereas this exception
        // may lend only a registry DATA turn, never another control turn.
        let wrq_return = self.first_server_check.as_ref().is_some_and(|write|
            write.before_elf && write.exact_wrq && !write.coalesced
                && write.wrq_result == Some(0) && write.oack.phase >= 1 && write.timing.closes == 0
                && self.receive.iter().any(|rx| rx.tftp_datagram_id() == write.request.intent
                    && rx.state == RxState::Control && rx.done_pending));
        let tftp = !wrq_return && self.tftp_control_turn_due && self.next_ready_tftp().is_some();
        (wrq_return, tftp)
    }

    #[cfg(feature = "sunfish-registry-converted-credit-turn")]
    fn converted_credit_candidate(&self, registry: Option<(usize, usize)>, converted: Option<(usize, usize)>)
        -> Option<(usize, usize, usize)>
    {
        // Use only the real converted-token selector and normal ready order;
        // no diagnostic counters, clocks or captured histories decide policy.
        let (target, _) = converted?;
        if registry != converted || self.transmit[target].registry_credit_turn_used { return None; }
        if !matches!(self.transmit[target].kind, TxKind::Reply { route, .. }
            if route.service == Service::ServiceRegistry && route.transaction == 4
                && route.destination == (Address { node: 0, port: 9 })) { return None; }
        let (credit, remote) = self.next_transmit_in_order([1])?;
        let tx = &self.transmit[credit];
        (matches!(tx.kind, TxKind::Resume) && tx.sent == 0 && tx.len == 52)
            .then_some((target, credit, remote))
    }

    #[cfg(feature = "sunfish-registry-converted-grant-turn")]
    fn converted_registry_candidate(&self) -> Option<(usize, usize)> {
        let (backing, request) = self.registry_converted_grant?;
        if request.owner != self.owner || request.generation != self.generation
            || self.intent_request_ack != Some(true) || self.intent_ack_backing != Some(backing)
            || !self.receive[..RX_SLOTS].iter().any(|rx| rx.id == backing && rx.state == RxState::Advertising)
        { return None; }
        let candidate = self.next_transmit_in_order([3])?;
        let tx = &self.transmit[candidate.0];
        let TxKind::Reply { receive, .. } = tx.kind else { return None; };
        (!tx.registry_grant_turn_used && self.receive[receive].id == request.intent
            && self.receive[receive].state == RxState::ReplyQueued).then_some(candidate)
    }

    #[cfg(feature = "sunfish-glink-tftp-control-fairness-experiment")]
    fn next_ready_tftp(&self) -> Option<(usize, usize)> {
        self.next_transmit_in_order([2, 2, 2])
    }

    #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
    fn unsent_flow_predecessor(&self, tx: &Transmit) -> bool {
        // Slot reuse must preserve send order within each destination flow.
        tx.reference_flow.as_ref().is_some_and(|current| {
            self.transmit.iter().any(|older| older.len != 0 && older.sent < older.len
                && older.reference_flow.as_ref().is_some_and(|token|
                    token.destination() == current.destination() && token.sequence() < current.sequence()))
        })
    }

    fn remote_for_transmit(&self, tx: &Transmit) -> Option<usize> {
        tx.remote_slot.or_else(|| {
            self.remote
                .iter()
                .enumerate()
                .filter_map(|(slot, intent)| {
                    intent
                        .filter(|intent| !intent.busy && intent.capacity as usize >= tx.len)
                        .map(|intent| (slot, intent.capacity))
                })
                .min_by_key(|(_, capacity)| *capacity)
                .map(|(slot, _)| slot)
        })
    }

    fn next_transmit_in_order<const N: usize>(&self, order: [u8; N]) -> Option<(usize, usize)> {
        for class in order {
            for (index, tx) in self.transmit.iter().enumerate() {
                let credit = matches!(tx.kind, TxKind::Resume);
                if tx.len == 0 || tx.sent == tx.len || match class {
                    1 => !credit,
                    0 => {
                        #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
                        { credit || matches!(tx.kind, TxKind::TftpReply { .. }) }
                        #[cfg(not(feature = "sunfish-glink-tftp-fairness-experiment"))]
                        { credit }
                    },
                    #[cfg(feature = "sunfish-wlan-registry-tx-turn")]
                    3 => !matches!(tx.kind, TxKind::Reply { receive, route, .. }
                        if route.service == Service::ServiceRegistry && !self.receive[receive].done_pending),
                    _ => !matches!(tx.kind, TxKind::TftpReply { .. }),
                } {
                    continue;
                }
                if let Some(remote) = self.ready_remote_for_transmit(tx) {
                    return Some((index, remote));
                }
            }
        }
        None
    }

    #[cfg(feature = "sunfish-ipa-before-first-oack")]
    pub fn enable_ipa_before_first_oack(&mut self) {
        self.ipa_before_first_oack = true;
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    pub fn enable_first_oack_after_ipa_publish(&mut self) {
        self.first_oack_after_ipa_publish = true;
    }

    #[cfg(feature = "sunfish-tftp-first-oack-unheld")]
    pub fn observe_first_tftp_without_oack_hold(&mut self) {
        self.first_tftp_unheld = true;
    }

    #[cfg(feature = "sunfish-ipa-before-first-oack")]
    fn is_unpublished_first_oack(&self, tx: &Transmit) -> bool {
        self.ipa_before_first_oack && tx.server_check_wrq.is_some()
            && tx.len != 0 && tx.sent == 0 && self.server_check_oacks()[0].is_some_and(|first|
            tx.server_check_wrq == Some(first.intent))
    }

    #[cfg(feature = "sunfish-ipa-before-first-oack")]
    fn holds_first_oack(&self, tx: &Transmit) -> bool {
        #[cfg(feature = "sunfish-tftp-first-oack-unheld")]
        if self.first_tftp_unheld {
            return false;
        }
        self.is_unpublished_first_oack(tx)
            && self.ipa_registration_trace().is_none_or(|trace| trace.completed_at.is_none())
    }

    #[cfg(feature = "sunfish-ipa-before-first-oack")]
    fn cancel_unpublished_first_oack(&mut self, port: u32) -> Result<(), Error> {
        let Some(index) = self.transmit.iter().position(|tx| self.is_unpublished_first_oack(tx)
            && matches!(tx.kind, TxKind::TftpReply { destination, .. } if destination.port == port))
        else { return Ok(()); };
        let tx = &self.transmit[index];
        let TxKind::TftpReply { source, destination } = tx.kind else { return Err(Error::InvalidReply); };
        let reservation = tx.reference_flow.as_ref().ok_or(Error::InvalidReply)?;
        self.rmtfs_reference_flows.fail_unpublished(reservation).map_err(|_| Error::InvalidReply)?;
        // The close snapshot keeps the queued phase, but this slot can
        // now belong to a different packet and must lose its trace binding.
        if let Some(write) = self.first_server_check.as_mut()
            && write.oack.slot == Some(index)
        {
            write.oack.slot = None;
        }
        self.transmit[index] = Transmit::empty();
        #[cfg(feature="sunfish-elf-tx-slots")]
        self.observe_elf_tx_freed(index,6);
        if self.first_tftp_slot == Some(index) {
            self.first_tftp_slot = None;
        }
        self.forget_qrtr_flow_if_idle(source, destination);
        Ok(())
    }

    #[cfg(feature = "sunfish-tftp-oack-reserve")]
    fn cancel_unpublished_reserved_oack(&mut self, port: u32) -> Result<(), Error> {
        let tx = &self.transmit[TX_SLOTS];
        let TxKind::TftpReply { source, destination } = tx.kind else { return Ok(()); };
        if tx.len == 0 || tx.sent != 0 || destination.port != port {
            return Ok(());
        }
        let reservation = tx.reference_flow.as_ref().ok_or(Error::InvalidReply)?;
        self.rmtfs_reference_flows.fail_unpublished(reservation).map_err(|_| Error::InvalidReply)?;
        self.transmit[TX_SLOTS] = Transmit::empty();
        for write in self.server_check_traces_mut() {
            if write.oack.slot == Some(TX_SLOTS) {
                write.oack.slot = None;
            }
        }
        self.forget_qrtr_flow_if_idle(source, destination);
        Ok(())
    }

    #[cfg(feature = "sunfish-post-reg-oack-turn")]
    pub(crate) fn post_reg_oack_turns(&self) -> &PostRegOackTurns { &self.post_reg_turns }

    #[cfg(feature = "sunfish-post-reg-oack-turn")]
    fn start_post_reg_oack_turn(&mut self, followup: Option<OackFollowup>) -> Option<usize> {
        self.first_oack_eligibility.queued_at?;
        self.ipa_registration_trace()?.published_at?;
        let first = self.first_oack_transfer()?;
        if first.peer != (Address { node: 0, port: 0x1f })
            || first.first_close_at.is_some() || first.oack_at.is_some() {
            return None;
        }
        let i = self.post_reg_turns.len;
        if i == self.post_reg_turns.turns.len() {
            self.post_reg_turns.omitted = self.post_reg_turns.omitted.saturating_add(1);
            return None;
        }
        let at = (self.intent_ack_clock?)();
        let mask = self.first_oack_blockers(first);
        let path = match followup {
            None => 0,
            Some(OackFollowup::FirstGrantAd) => 1,
            Some(OackFollowup::FirstGrantAck) => 2,
            Some(OackFollowup::FirstGrantReply) => 3,
            Some(OackFollowup::FirstGrantInit) => 4,
            Some(_) => 5,
        };
        self.post_reg_turns.turns[i] = PostRegOackTurn { at, mask, path, ..Default::default() };
        self.post_reg_turns.len += 1;
        Some(i)
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    pub(crate) fn first_oack_eligibility(&self) -> FirstOackEligibility {
        self.first_oack_eligibility
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    fn first_oack_transfer(&self) -> Option<&ServerCheckOackTrace> {
        self.server_check_transfers.iter().flatten().min_by_key(|t| t.wrq_at.unwrap_or(u64::MAX))
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    fn first_oack_blockers(&self, first: &ServerCheckOackTrace) -> u16 {
        let mut mask = 0;
        if !self.registration_reply_in_flight() { mask |= 0x02; }
        if self.intent_request_ack.is_some() { mask |= 0x04; }
        if self.transmit.iter().any(|tx| matches!(tx.kind, TxKind::Resume) && tx.sent < tx.len) {
            mask |= 0x08;
        }
        if matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. }) {
            mask |= 0x10;
        }
        if first.early_oack_attempted { mask |= 0x80; }
        if let Some(tx) = self.transmit.iter().find(|tx| tx.len != 0
            && tx.server_check_wrq == Some(first.intent)
            && matches!(tx.kind, TxKind::TftpReply { source, destination }
                if destination == first.peer && source.port == first.transfer_port))
        {
            if self.unsent_flow_predecessor(tx) { mask |= 0x20; }
            if self.remote_for_transmit(tx).is_none() { mask |= 0x40; }
            if tx.sent != 0 || tx.len > TX_CHUNK_BYTES { mask |= 0x100; }
        } else {
            mask |= 0x01;
        }
        mask
    }

    #[cfg(feature = "sunfish-first-wrq-spare-oack")]
    pub fn enable_first_wrq_spare_oack(&mut self) {
        self.first_wrq_spare_oack = true;
    }

    #[cfg(feature = "sunfish-first-wrq-spare-oack")]
    pub(crate) fn first_oack_ready_without_grant(&self) -> bool {
        self.first_wrq_spare_oack && self.first_oack_after_ipa_publish
            && self.original_1f_close_count() == Some(0)
            && self.first_oack_transfer().is_some_and(|first|
                first.oack_at.is_none() && self.first_oack_blockers(first) == 0)
    }

    /// Match the ordinary commit that may open a bounded follow-up. The
    /// currently pending positive grant must be the sole measured blocker.
    #[cfg(feature = "sunfish-first-oack-grant-window")]
    pub(crate) fn first_oack_grant_trigger(&self) -> Option<Published> {
        if !self.first_oack_after_ipa_publish || self.intent_request_ack != Some(true) {
            return None;
        }
        let first = self.first_oack_transfer()?;
        if first.first_close_at.is_some() || first.oack_at.is_some() || self.first_oack_blockers(first) != 4 {
            return None;
        }
        Some(match self.intent_ack_backing {
            Some(id) => Published::ReceiveIntent(id),
            None => Published::IntentRequestAck { granted: true },
        })
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    fn observe_first_oack_pass(&mut self) -> bool {
        if !self.first_oack_after_ipa_publish || self.first_oack_eligibility.close_mask.is_some() {
            return false;
        }
        let Some(first) = self.first_oack_transfer() else { return false };
        if first.early_oack_attempted || first.oack_at.is_some() { return false; }
        let mask = self.first_oack_blockers(first);
        let trace = &mut self.first_oack_eligibility;
        trace.passes = trace.passes.saturating_add(1);
        if mask & 0x02 != 0 { return false; }
        trace.common = if trace.ipa_passes == 0 { mask } else { trace.common & mask };
        trace.seen |= mask;
        trace.ipa_passes = trace.ipa_passes.saturating_add(1);
        if mask == 0 {
            trace.ready_passes = trace.ready_passes.saturating_add(1);
            trace.ready_command = None;
        }
        mask == 0
    }

    #[cfg(feature = "sunfish-first-wrq-return-oack")]
    pub(crate) fn wrq_return_oack_candidate(&self) -> Option<(u16, usize)> {
        let first = self.first_oack_transfer()?;
        if !self.first_oack_after_ipa_publish || first.peer != (Address { node: 0, port: 0x1f })
            || first.wrq_done_at.is_none() || first.oack_at.is_some()
            || first.first_close_at.is_some()
            || self.ipa_registration_trace().is_some_and(|reg| reg.queued_at.is_some())
        {
            return None;
        }
        // The server-check OACK occupies 72 bytes including its QRTR header.
        let fitting = self.remote.iter().flatten()
            .filter(|intent| !intent.busy && intent.capacity >= 72).count();
        Some((self.first_oack_blockers(first), fitting))
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    fn in_wrq_return_writer(&self) -> bool {
        #[cfg(feature = "sunfish-first-wrq-return-oack")]
        { self.wrq_return_writer }
        #[cfg(not(feature = "sunfish-first-wrq-return-oack"))]
        { false }
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    fn can_attempt_first_oack(&self, tx: &Transmit) -> bool {
        let TxKind::TftpReply { source, destination } = tx.kind else { return false };
        self.first_oack_after_ipa_publish && self.server_check_oacks()[0].is_some_and(|first|
            tx.server_check_wrq == Some(first.intent) && source.port == first.transfer_port
            && destination == first.peer && first.first_close_at.is_none() && !first.early_oack_attempted)
            && tx.sent == 0 && tx.len <= TX_CHUNK_BYTES
            && self.intent_request_ack.is_none()
            && !self.transmit.iter().any(|other| matches!(other.kind, TxKind::Resume) && other.sent < other.len)
            && (self.registration_reply_in_flight() || self.in_wrq_return_writer())
    }

    fn ready_remote_for_transmit(&self, tx: &Transmit) -> Option<usize> {
        #[cfg(feature = "sunfish-ipa-before-first-oack")]
        if self.holds_first_oack(tx) {
            #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
            if !self.can_attempt_first_oack(tx) { return None; }
            #[cfg(not(feature = "sunfish-first-oack-after-ipa-publish"))]
            return None;
        }
        #[cfg(feature = "sunfish-glink-intent-request-experiment")]
        if matches!(self.outbound_intent_request, OutboundIntentRequest::Waiting { .. })
            && !matches!(tx.kind, TxKind::Resume) {
            return None;
        }
        #[cfg(feature = "sunfish-qrtr-rmtfs-flow-experiment")]
        if self.unsent_flow_predecessor(tx) {
            return None;
        }
        self.remote_for_transmit(tx)
    }
}

#[cfg(all(test, feature = "sunfish-first-data-ack-elf-trace"))]
#[path = "qcom_tftp_completion_test.rs"]
mod completion_identity_tests;
