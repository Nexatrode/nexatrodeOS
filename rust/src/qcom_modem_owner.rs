//! Serialized modem transport owner: bounded work, retained fault state, IRQs.
//! Native attachment must consume the actual authenticated channel handoff.
#![forbid(unsafe_code)]
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "sunfish_ipa_linux_qmi_owner.rs"]
mod linux_ipa_owner;
#[cfg(feature = "sunfish-linux-rmtfs")]
#[path = "sunfish_linux_rmtfs_owner.rs"]
mod linux_rmtfs_owner;
use crate::qcom_glink_fifo::{self as fifo, Adapter, Io, Receive, Transmit};
use crate::qcom_glink_qmi::{Event, MAX_REQUESTS, Reply, RequestHandle, TftpHandle};
use crate::qcom_modem_handlers::{self as actions, Backend, Handlers};
use crate::qcom_modem_services::{Request, Service};
use crate::qcom_msa::Reservation as MsaReservation;
use crate::qcom_qmi_router::Context;
use crate::qcom_tftp::{self as tftp, RequestObservation};
#[cfg(feature = "sunfish-linux-ipa-qmi")]
pub use linux_ipa_owner::{LinuxIpaError, LinuxIpaProgress, LinuxIpaReport};
#[cfg(feature = "sunfish-ipa-registration-owner-trace")]
pub use linux_ipa_owner::IpaRegistrationOwnerTrace;
#[cfg(feature = "sunfish-ipa-init-continuation")]
pub use linux_ipa_owner::IpaInitOwnerTrace;

pub const GLINK_CHANNEL: u8 = 0;
pub const STATUS_CHANNEL: u8 = 1;
pub const CONTINUE_SEND: u8 = 2;
pub const CONTINUE_RECEIVE: u8 = 3;
pub const GLINK_IRQ: u32 = 481;
pub const STATUS_IRQ: u32 = 483;
pub const WORK_BUDGET: usize = 16;
const GLINK: u64 = 1 << GLINK_CHANNEL;
const STATUS: u64 = 1 << STATUS_CHANNEL;
const CONTINUE: u64 = 1 << CONTINUE_RECEIVE;

pub const SMEM_VIRTUAL: usize = 0x44000000;
pub const AP_OFFSET: usize = 0x11d000;
pub const AP_BYTES: usize = 0x5000;
pub const SMEM_BYTES: usize = 0x200000;

/// Retained negotiation records, including the two secondary OPEN requests
/// which this IPCRTR owner has NOT acknowledged. Validation is wire parsing,
/// not a boot authorization; native attachment also consumes a checkpoint.
pub struct Prelude {
    bytes: [u8; 88],
}
impl Prelude {
    pub fn validate(bytes: [u8; 88]) -> Result<Self, Error> {
        // The peer can advertise the two unacknowledged channels in either
        // order. Match the validated OPEN shapes used by pixel_glink_open:
        // IDs stay 1/2, exactly one name is IPCRTR, and the name lengths are
        // 7/10 bytes including NUL. The spelling of the other offer is not
        // authorization; only its bounded wire shape is retained here.
        let open = |at: usize, id: u16, allowed_len: &[u32]| -> Result<(usize, u32), Error> {
            if at + 8 > 40
                || bytes[at..at + 2] != 2u16.to_le_bytes()
                || bytes[at + 2..at + 4] != id.to_le_bytes()
            {
                return Err(Error::Prelude);
            }
            let name_bytes = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
            if !allowed_len.contains(&name_bytes) {
                return Err(Error::Prelude);
            }
            let name_end = at + 8 + name_bytes as usize;
            let record_end = (name_end + 7) & !7;
            if name_end > 40
                || record_end > 40
                || bytes[name_end - 1] != 0
                || bytes[at + 8..name_end - 1].contains(&0)
                || (name_bytes == 7 && &bytes[at + 8..name_end - 1] != b"IPCRTR")
            {
                return Err(Error::Prelude);
            }
            Ok((record_end, name_bytes))
        };
        let (first_end, first_len) = open(0, 1, &[7, 10])?;
        let (second_end, second_len) = open(first_end, 2, &[7, 10])?;
        if first_len == second_len || second_end != 40 {
            return Err(Error::Prelude);
        }
        // The next unacknowledged offer has a fixed ID and bounded wire
        // length. Preserve its spelling exactly as observed by the upstream
        // parser, which deliberately treats this non-IPCRTR name as opaque.
        let third_at = 40usize;
        let third_name_bytes = 19usize;
        let third_name_end = third_at + 8 + third_name_bytes;
        if bytes[third_at..third_at + 2] != 2u16.to_le_bytes()
            || bytes[third_at + 2..third_at + 4] != 3u16.to_le_bytes()
            || bytes[third_at + 4..third_at + 8] != (third_name_bytes as u32).to_le_bytes()
            || bytes[third_name_end - 1] != 0
            || bytes[third_at + 8..third_name_end - 1].contains(&0)
            || (third_name_end + 7) & !7 != 72
        {
            return Err(Error::Prelude);
        }
        if bytes[72..80] != [1, 0, 1, 0, 0, 0, 0, 0] || bytes[80..88] != [4, 0, 1, 0, 0, 0, 0, 0] {
            return Err(Error::Prelude);
        }
        Ok(Self { bytes })
    }
    pub fn retained_bytes(&self) -> &[u8; 88] {
        &self.bytes
    }
    pub fn validate_cursors(words: [u32; 8]) -> Result<(), Error> {
        if words[..3] != [40, 40, 8]
            || words[3] < 96
            || words[3] >= fifo::FIFO_BYTES as u32
            || words[3] % 8 != 0
            || words[4..] != [0; 4]
        {
            return Err(Error::Prelude);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    descriptor: usize,
    tx: usize,
    rx: usize,
}
impl Layout {
    /// Offsets come from the locked SMEM directory, never from a QMI address.
    /// Only the five pre-authorized AP pages may be writable. RX may share
    /// their last page, but its byte range must be disjoint from AP writers.
    pub fn validate(
        descriptor: (usize, usize),
        tx: (usize, usize),
        rx: (usize, usize),
    ) -> Result<Self, Error> {
        let fits = |at: usize, n: usize, start: usize, end: usize| {
            at >= start && at.checked_add(n).is_some_and(|v| v <= end)
        };
        if descriptor.1 != 32
            || tx.1 != fifo::FIFO_BYTES
            || rx.1 != fifo::FIFO_BYTES
            || descriptor.0 % 4 != 0
            || tx.0 % 8 != 0
            || rx.0 % 8 != 0
            || !fits(descriptor.0, descriptor.1, AP_OFFSET, AP_OFFSET + AP_BYTES)
            || !fits(tx.0, tx.1, AP_OFFSET, AP_OFFSET + AP_BYTES)
            || !fits(rx.0, rx.1, 0, SMEM_BYTES - 4096)
        {
            return Err(Error::Mapping);
        }
        let overlap = |a: (usize, usize), b: (usize, usize)| a.0 < b.0 + b.1 && b.0 < a.0 + a.1;
        if overlap(descriptor, tx) || overlap(rx, descriptor) || overlap(rx, tx) {
            return Err(Error::Mapping);
        }
        Ok(Self {
            descriptor: descriptor.0,
            tx: tx.0,
            rx: rx.0,
        })
    }
    pub fn offsets(self) -> (usize, usize, usize) {
        (self.descriptor, self.tx, self.rx)
    }
    pub fn validate_symbols(
        smem: usize,
        ap: usize,
        tail: usize,
        lock: usize,
        doorbell: usize,
    ) -> Result<(), Error> {
        if (smem, ap, tail, lock, doorbell)
            != (
                SMEM_VIRTUAL,
                SMEM_VIRTUAL + AP_OFFSET,
                SMEM_VIRTUAL + AP_OFFSET + 4096,
                0x40006000,
                0x40005000,
            )
        {
            return Err(Error::Mapping);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    #[cfg(feature = "sunfish-linux-rmtfs")]
    LinuxRmtfs(crate::sunfish_linux_rmtfs_server::Error),
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    LinuxIpa(LinuxIpaError),
    Service(actions::Error),
    Transport(fifo::Error),
    Mapping,
    Prelude,
    Stopped,
    UnexpectedNotification,
    Ack,
    Schedule,
    QueueFull,
    IpaReadinessRequired,
    MemoryServicesNotReady,
    TftpImageNotReady,
    GenerationMismatch,
}
impl From<fifo::Error> for Error {
    fn from(error: fifo::Error) -> Self {
        Self::Transport(error)
    }
}

pub trait Interrupts {
    /// Exactly one kernel IRQ ACK. Failure may have taken effect; never retry.
    fn acknowledge(&mut self, channel: u8) -> Result<(), Error>;
    /// Signal the dedicated local continuation channel; no recursive dispatch.
    fn schedule(&mut self) -> Result<(), Error>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Progress {
    #[cfg(feature = "sunfish-ipa-init-empty-rx")]
    pub(crate) init_at_registration: bool,
    pub received: u32,
    pub published: u32,
    pub requests: u32,
    pub controls: u32,
    pub deferred: bool,
    pub acknowledged: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceProgress {
    Idle,
    Progress,
    ReplyQueued,
    WaitingReply,
}

#[cfg(feature = "sunfish-wlan-read-sequence")]
#[derive(Clone, Copy, Default)]
pub struct ReadSequence {
    pub total: u32,
    pub kinds: u32,
    pub first: [Option<(u32, u32, Option<u64>)>; 5],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TftpServiceProgress {
    Idle,
    ResponseQueued,
    WaitingReply,
    Closed(crate::qcom_qrtr_tftp::CloseReason),
    Ignored(crate::qcom_qrtr_tftp::IgnoreReason),
}

#[derive(Clone, Copy)]
struct PendingTftpReply {
    route: crate::qcom_glink_qmi::TftpReplyRoute,
    bytes: usize,
}
#[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
#[derive(Clone, Copy)]
struct PeerTftpReply {
    meta: PendingTftpReply,
    bytes: [u8; crate::qcom_tftp::MAX_REQUEST_BYTES],
    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
    request: crate::qcom_glink_qmi::TftpHandle,
}

#[cfg(feature = "sunfish-wlan-elf-timing")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfLookupTrace {
    pub peer_port: u32,
    pub domain_offset: u32,
    pub total_domains: Option<u16>,
    pub returned_rows: Option<u8>,
    pub received: Option<u64>,
    pub queued: Option<u64>,
    pub published: Option<u64>,
    pub done: Option<u64>,
    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
    pub waits_by_reason: [u32; 6],
    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
    pub last_pressure: Option<crate::qcom_glink_qmi::RegistryQueuePressure>,
    #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
    pub dispatch: ElfDispatchTiming,
    #[cfg(feature = "sunfish-wlan-elf-phase-io")]
    pub io: ElfIoTrace,
    #[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
    pub blocker: Option<ElfReplyBlocker>,
}

/// The first unpublished registry reply observed holding the ELF peer gate.
/// The opaque request handle binds subsequent observations to the exact reply.
#[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElfReplyBlocker {
    handle: RequestHandle,
    pub transaction: u16,
    pub flags_at_wait: u8,
    pub wait: Option<u64>,
    pub first_turn: Option<u64>,
    pub published: Option<u64>,
    pub done: Option<u64>,
    #[cfg(feature = "sunfish-wlan-elf-completion-turn")]
    pub yielded: bool,
    #[cfg(feature = "sunfish-wlan-elf-completion-turn")]
    pub yielded_at: Option<u64>,
}

/// Counter snapshots at the existing first-ELF timing boundaries. Nested
/// detail/domain counters overlap the top-level I/O categories.
#[cfg(feature = "sunfish-wlan-elf-phase-io")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfIoTrace {
    pub received: Option<[u64; 12]>,
    pub receive_pass_end: Option<[u64; 12]>,
    pub queued: Option<[u64; 12]>,
    pub published: Option<[u64; 12]>,
}

#[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfDispatchTiming {
    pub receive_pass_end: Option<u64>,
    pub registry_entered: Option<u64>,
    pub selected: Option<u64>,
    #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
    pub turns: ElfRegistryTurns,
}

#[cfg(feature = "sunfish-wlan-elf-registry-turns")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElfRegistryTurns {
    previous_entry: Option<u64>,
    pub max_gap: u64,
    pub max_call: u64,
    pub calls: u32,
    /// Four consecutive runs, encoded as transaction, name/outcome, count.
    pub runs: [u32; 4],
    pub omitted: u32,
}

#[cfg(feature = "sunfish-wlan-elf-registry-turns")]
impl ElfRegistryTurns {
    fn enter(&mut self, now: u64) {
        if let Some(previous) = self.previous_entry.replace(now) {
            self.max_gap = self.max_gap.max(now.wrapping_sub(previous));
        }
    }

    fn record(&mut self, transaction: u16, name_code: u32, outcome: u8, ended: Option<u64>) {
        self.calls = self.calls.saturating_add(1);
        if let Some((start, end)) = self.previous_entry.zip(ended) {
            self.max_call = self.max_call.max(end.wrapping_sub(start));
        }
        let name = if name_code <= 6 { name_code } else { 0 };
        let key = u32::from(transaction) << 16 | name << 12 | u32::from(outcome) << 8;
        if self.omitted == 0 {
            if let Some(last) = self.runs.iter_mut().rev().find(|run| **run != 0)
                && *last & !0xff == key && *last & 0xff < 0xff
            {
                *last += 1;
                return;
            }
            if let Some(next) = self.runs.iter_mut().find(|run| **run == 0) {
                *next = key | 1;
                return;
            }
        }
        self.omitted = self.omitted.saturating_add(1);
    }
}

#[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryTxn4Timing {
    pub name_code: u32,
    pub received: Option<u64>,
    pub queued: Option<u64>,
    pub published: Option<u64>,
    pub done: Option<u64>,
    pub elf_waits: u32,
    #[cfg(feature = "sunfish-wlan-tx4-pressure")]
    pub waits_by_reason: [u32; 6],
    #[cfg(feature = "sunfish-wlan-tx4-pressure")]
    pub last_pressure: Option<crate::qcom_glink_qmi::RegistryQueuePressure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceRegistryTrace {
    /// GLINK QMI requests delivered to the manifest-backed LOC endpoint.
    pub requests: u32,
    /// Replies queued into the QRTR/GLINK transmit path.
    pub replies_queued: u32,
    /// Stable service-name code for the first successfully queued LOC reply.
    pub first_reply_name_code: u32,
    /// Whether the peer returned GLINK RX_DONE for that first queued reply.
    pub first_reply_glink_rx_done: bool,
    #[cfg(feature = "sunfish-wlan-startup-trace")]
    pub queued_names: u32,
    #[cfg(feature = "sunfish-wlan-startup-trace")]
    pub completed_names: u32,
    #[cfg(feature = "sunfish-wlan-elf-timing")]
    pub elf: Option<ElfLookupTrace>,
    #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
    pub elf_exchange: Option<crate::qcom_glink_qmi::ElfExchangeTrace>,
    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    pub first_server_check: Option<crate::qcom_glink_qmi::ElfServerCheckTrace>,
    #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
    pub first_server_check_tail: fifo::ServerCheckTail,
    #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
    pub txn4: Option<RegistryTxn4Timing>,
    /// Whether a matching QRTR RESUME_TX arrived for that first queued reply.
    pub first_reply_qrtr_resume: bool,
    /// Transient reply-queue backpressure observations.
    pub reply_waits: u32,
    /// Enqueue backpressure counts indexed by the requested service name.
    pub reply_wait_name_counts: [u32; crate::qcom_servreg::NAME_COUNTER_COUNT],
    /// Reply enqueue attempts rejected because this destination flow was busy.
    pub reply_flow_busy: u32,
    /// Reply enqueue attempts rejected because no transmit slot was available.
    pub reply_no_capacity: u32,
    /// Stable `qcom_servreg::service_name_code`, or NAME_UNKNOWN before a request.
    pub last_name_code: u32,
    /// Bounded, uppercase copy of the first accepted LOC service name.
    pub first_service_name: [u8; crate::qcom_servreg::MAX_NAME_BYTES],
    pub first_service_name_len: u8,
    /// Bounded, uppercase display copy of the last accepted LOC service name.
    pub last_service_name: [u8; crate::qcom_servreg::MAX_NAME_BYTES],
    pub last_service_name_len: u8,
    #[cfg(feature = "sunfish-wlan-locator-other-trace")]
    first_other_service_name: [u8; crate::qcom_servreg::MAX_NAME_BYTES],
    #[cfg(feature = "sunfish-wlan-locator-other-trace")]
    first_other_service_name_len: u8,
    #[cfg(feature = "sunfish-wlan-locator-other-trace")]
    pub avs_audio_queries: u32,
    /// Saturating query counts in the fixed `qcom_servreg` name-index order.
    pub name_counts: [u32; crate::qcom_servreg::NAME_COUNTER_COUNT],
    pub last_domain_offset: u32,
    pub last_total_domains: u32,
}

/// Fixed payload-free trace for the raw QRTR TFTP observer. It records only
/// parsed request metadata; it never retains a QRTR or TFTP DATA payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TftpObserverTrace {
    pub packets: u32,
    pub read_requests: u32,
    pub peer: Option<crate::qcom_qmi_router::Address>,
    pub last: Option<RequestObservation>,
    pub parse_error: Option<tftp::Error>,
}

impl TftpObserverTrace {
    const fn new() -> Self {
        Self {
            packets: 0,
            read_requests: 0,
            peer: None,
            last: None,
            parse_error: None,
        }
    }

    fn record(
        &mut self,
        peer: crate::qcom_qmi_router::Address,
        observation: Result<RequestObservation, tftp::Error>,
    ) {
        self.packets = self.packets.saturating_add(1);
        self.peer = Some(peer);
        match observation {
            Ok(observation) => {
                if observation.opcode == tftp::OP_RRQ {
                    self.read_requests = self.read_requests.saturating_add(1);
                }
                self.last = Some(observation);
                self.parse_error = None;
            }
            Err(error) => {
                self.last = None;
                self.parse_error = Some(error);
            }
        }
    }
}

impl ServiceRegistryTrace {
    const fn new() -> Self {
        Self {
            requests: 0,
            replies_queued: 0,
            first_reply_name_code: crate::qcom_servreg::NAME_UNKNOWN,
            first_reply_glink_rx_done: false,
            #[cfg(feature = "sunfish-wlan-startup-trace")]
            queued_names: 0,
            #[cfg(feature = "sunfish-wlan-startup-trace")]
            completed_names: 0,
            #[cfg(feature = "sunfish-wlan-elf-timing")]
            elf: None,
            #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
            elf_exchange: None,
            #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
            first_server_check: None,
            #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
            first_server_check_tail: fifo::ServerCheckTail::Untracked,
            #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
            txn4: None,
            first_reply_qrtr_resume: false,
            reply_waits: 0,
            reply_wait_name_counts: [0; crate::qcom_servreg::NAME_COUNTER_COUNT],
            reply_flow_busy: 0,
            reply_no_capacity: 0,
            last_name_code: crate::qcom_servreg::NAME_UNKNOWN,
            first_service_name: [0; crate::qcom_servreg::MAX_NAME_BYTES],
            first_service_name_len: 0,
            last_service_name: [0; crate::qcom_servreg::MAX_NAME_BYTES],
            last_service_name_len: 0,
            #[cfg(feature = "sunfish-wlan-locator-other-trace")]
            first_other_service_name: [0; crate::qcom_servreg::MAX_NAME_BYTES],
            #[cfg(feature = "sunfish-wlan-locator-other-trace")]
            first_other_service_name_len: 0,
            #[cfg(feature = "sunfish-wlan-locator-other-trace")]
            avs_audio_queries: 0,
            name_counts: [0; crate::qcom_servreg::NAME_COUNTER_COUNT],
            last_domain_offset: u32::MAX,
            last_total_domains: u32::MAX,
        }
    }

    fn record_query(&mut self, query: crate::qcom_servreg::Query<'_>) {
        let name_code = crate::qcom_servreg::service_name_code(query.service_name);
        if self.requests == 0 {
            self.first_service_name_len =
                copy_service_name(&mut self.first_service_name, query.service_name);
        }
        self.requests = self.requests.saturating_add(1);
        self.last_name_code = name_code;
        let count_index = crate::qcom_servreg::name_counter_index(self.last_name_code);
        #[cfg(feature = "sunfish-wlan-locator-other-trace")]
        if count_index == crate::qcom_servreg::NAME_COUNTER_OTHER {
            if self.first_other_service_name_len == 0 {
                self.first_other_service_name_len =
                    copy_service_name(&mut self.first_other_service_name, query.service_name);
            }
            if query.service_name == b"avs/audio" {
                self.avs_audio_queries = self.avs_audio_queries.saturating_add(1);
            }
        }
        self.name_counts[count_index] = self.name_counts[count_index].saturating_add(1);
        self.last_service_name_len =
            copy_service_name(&mut self.last_service_name, query.service_name);
        self.last_domain_offset = query.domain_offset;
        self.last_total_domains = u32::from(crate::qcom_servreg::respond(query).total_domains());
    }

    fn record_reply_queued(&mut self, name_code: u32) {
        if self.replies_queued == 0 {
            self.first_reply_name_code = name_code;
        }
        self.replies_queued = self.replies_queued.saturating_add(1);
    }

    fn record_reply_wait(&mut self, name_code: u32) {
        self.reply_waits = self.reply_waits.saturating_add(1);
        let index = crate::qcom_servreg::name_counter_index(name_code);
        self.reply_wait_name_counts[index] = self.reply_wait_name_counts[index].saturating_add(1);
    }

    pub fn first_service_name_parts(&self) -> (&str, &str) {
        service_name_parts(&self.first_service_name, self.first_service_name_len)
    }

    pub fn last_service_name_parts(&self) -> (&str, &str) {
        service_name_parts(&self.last_service_name, self.last_service_name_len)
    }

    #[cfg(feature = "sunfish-wlan-locator-other-trace")]
    pub fn first_other_service_name_parts(&self) -> (&str, &str) {
        service_name_parts(&self.first_other_service_name, self.first_other_service_name_len)
    }
}

fn copy_service_name(output: &mut [u8; crate::qcom_servreg::MAX_NAME_BYTES], input: &[u8]) -> u8 {
    output.fill(0);
    let name_len = input.len().min(output.len());
    for (output, input) in output[..name_len].iter_mut().zip(&input[..name_len]) {
        *output = match *input {
            b'a'..=b'z' => *input - (b'a' - b'A'),
            b'A'..=b'Z' | b'0'..=b'9' => *input,
            _ => b' ',
        };
    }
    name_len as u8
}

fn service_name_parts(
    service_name: &[u8; crate::qcom_servreg::MAX_NAME_BYTES],
    service_name_len: u8,
) -> (&str, &str) {
    let len = usize::from(service_name_len).min(service_name.len());
    let split = len.min(32);
    let name = core::str::from_utf8(&service_name[..len]).unwrap_or("");
    if name.is_empty() {
        return ("", "");
    }
    (&name[..split], &name[split..])
}

#[cfg(test)]
mod service_registry_trace_tests {
    #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
    #[test]
    fn elf_registry_runs_coalesce_retries_and_report_omissions() {
        let mut trace = super::ElfRegistryTurns::default();
        for (index, (txn, outcome)) in [
            (4, 2), (4, 2), (4, 1), (5, 3), (6, 1), (7, 1), (6, 1),
        ].into_iter().enumerate() {
            let start = index as u64 * 20;
            trace.enter(start);
            trace.record(txn, 1, outcome, Some(start + 3));
        }
        trace.enter(200);
        assert_eq!(trace.calls, 7);
        assert_eq!(trace.runs, [0x0004_1202, 0x0004_1101, 0x0005_1301, 0x0006_1101]);
        assert_eq!(trace.omitted, 2);
        assert_eq!((trace.max_gap, trace.max_call), (80, 3));
    }

    use super::ServiceRegistryTrace;

    #[test]
    fn trace_keeps_bounded_uppercase_service_name_in_two_display_parts() {
        let mut name = [b'a'; crate::qcom_servreg::MAX_NAME_BYTES];
        name[31] = b'/';
        name[32] = b'Z';
        let query = crate::qcom_servreg::Query {
            service_name: &name,
            domain_offset: 0,
        };
        let mut trace = ServiceRegistryTrace::new();
        trace.record_query(query);
        let (first, second) = trace.last_service_name_parts();

        assert_eq!(trace.last_name_code, crate::qcom_servreg::NAME_UNKNOWN);
        assert_eq!(
            trace.last_service_name_len,
            crate::qcom_servreg::MAX_NAME_BYTES as u8
        );
        assert_eq!(first.len(), 32);
        assert!(first[..31].bytes().all(|byte| byte == b'A'));
        assert_eq!(&first[31..], " ");
        assert_eq!(&second[..1], "Z");
        assert!(second[1..].bytes().all(|byte| byte == b'A'));
        assert_eq!(trace.first_service_name_parts(), (first, second));
    }

    #[test]
    fn trace_counts_known_names_and_groups_unknown_names() {
        let names: [&[u8]; 8] = [
            b"tms/servreg",
            b"gps/gps_service",
            b"kernel/elf_loader",
            b"wlan/fw",
            b"tms/pdr_enabled",
            b"tms/pddump_disabled",
            b"unmapped/first",
            b"unmapped/second",
        ];
        let mut trace = ServiceRegistryTrace::new();
        for service_name in names {
            trace.record_query(crate::qcom_servreg::Query {
                service_name,
                domain_offset: 0,
            });
        }

        assert_eq!(trace.requests, names.len() as u32);
        assert_eq!(trace.name_counts, [1, 1, 1, 1, 1, 1, 2]);
        assert_eq!(trace.first_service_name_parts(), ("TMS SERVREG", ""));
        assert_eq!(trace.last_service_name_parts(), ("UNMAPPED SECOND", ""));
        #[cfg(feature = "sunfish-wlan-locator-other-trace")]
        {
            for service_name in [b"AVS/AUDIO".as_slice(), b"avs/audio", b"avs/audio"] {
                trace.record_query(crate::qcom_servreg::Query {
                    service_name,
                    domain_offset: 0,
                });
            }
            assert_eq!(trace.first_other_service_name_parts(), ("UNMAPPED FIRST", ""));
            assert_eq!(trace.avs_audio_queries, 2);
            assert_eq!(
                trace.last_total_domains,
                u32::from(cfg!(feature = "sunfish-servreg-avs-audio"))
            );
        }
    }

    #[test]
    fn trace_latches_name_of_first_successfully_queued_reply() {
        let mut trace = ServiceRegistryTrace::new();
        assert_eq!(
            trace.first_reply_name_code,
            crate::qcom_servreg::NAME_UNKNOWN
        );

        trace.record_reply_queued(crate::qcom_servreg::NAME_TMS_PDR_ENABLED);
        trace.record_reply_queued(crate::qcom_servreg::NAME_TMS_PDDUMP_DISABLED);

        assert_eq!(trace.replies_queued, 2);
        assert_eq!(
            trace.first_reply_name_code,
            crate::qcom_servreg::NAME_TMS_PDR_ENABLED
        );
    }

    #[test]
    fn trace_attributes_enqueue_waits_to_the_requested_service_name() {
        let mut trace = ServiceRegistryTrace::new();
        trace.record_reply_wait(crate::qcom_servreg::NAME_TMS_PDR_ENABLED);
        trace.record_reply_wait(crate::qcom_servreg::NAME_TMS_PDDUMP_DISABLED);
        trace.record_reply_wait(crate::qcom_servreg::NAME_TMS_PDDUMP_DISABLED);
        trace.record_reply_wait(crate::qcom_servreg::NAME_UNKNOWN);

        assert_eq!(trace.reply_waits, 4);
        assert_eq!(trace.reply_wait_name_counts, [0, 0, 0, 0, 1, 2, 1]);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CombinedProgress {
    pub transport: Progress,
    pub service: ServiceProgress,
}

/// Times observed by the serialized host owner, not by the modem or on wire.
/// A reply to a different peer is deliberately not paired with the first WRQ.
#[cfg(feature = "sunfish-tftp-timing-observer")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TftpTiming {
    pub peer: Option<crate::qcom_qmi_router::Address>,
    pub request: Option<u64>,
    pub queued: Option<u64>,
    pub published: Option<u64>,
    pub done: Option<u64>,
    #[cfg(feature = "sunfish-tftp-peer-close-observer")]
    pub peer_close: Option<u64>,
    #[cfg(feature = "sunfish-tftp-peer-close-observer")]
    pub peer_close_phase: u32,
    #[cfg(feature = "sunfish-tftp-tail-observer")]
    pub peer_tail: Option<u64>,
    #[cfg(feature = "sunfish-tftp-host-gap-observer")]
    pub max_host_gap: u64,
    #[cfg(feature = "sunfish-tftp-host-gap-observer")]
    pub host_observations: u32,
    #[cfg(feature = "sunfish-tftp-host-gap-observer")]
    last_host_observation: Option<u64>,
    #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
    pub last_rx_empty: Option<u64>,
    #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
    pub max_rx_bytes: u32,
    pub different_peer: bool,
    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
    pub matching_data: u32,
    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
    pub first_matching_data: Option<u64>,
}
#[cfg(feature = "sunfish-tftp-timing-observer")]
impl TftpTiming {
    #[cfg(feature = "sunfish-tftp-peer-close-observer")]
    fn peer_closed(&mut self, port: u32, now: u64) {
        if self.request.is_none() || self.peer_close.is_some()
            || !self.peer.is_some_and(|peer| peer.port == port) { return; }
        self.peer_close = Some(now);
        self.peer_close_phase = u32::from(self.queued.is_some())
            | (u32::from(self.published.is_some()) << 1)
            | (u32::from(self.done.is_some()) << 2);
    }
    fn request(&mut self, peer: crate::qcom_qmi_router::Address, now: u64) {
        if self.request.is_none() { self.peer = Some(peer); self.request = Some(now); }
    }
    fn queued(&mut self, peer: crate::qcom_qmi_router::Address, now: u64) {
        if self.queued.is_some() || self.different_peer { return; }
        if self.request.is_none() { return; }
        if self.peer != Some(peer) { self.different_peer = true; return; }
        self.queued = Some(now);
    }
    fn observe(&mut self, flags: u32, now: u64) {
        if self.queued.is_none() { return; }
        if flags & 4 != 0 && self.published.is_none() { self.published = Some(now); }
        #[cfg(feature = "sunfish-tftp-host-gap-observer")]
        if self.published.is_some() && self.done.is_none() {
            if let Some(last) = self.last_host_observation {
                let gap = now.wrapping_sub(last);
                if gap < (1u64 << 63) { self.max_host_gap = self.max_host_gap.max(gap); }
            }
            self.last_host_observation = Some(now);
            self.host_observations = self.host_observations.saturating_add(1);
        }
        if flags & 8 != 0 && self.done.is_none() { self.done = Some(now); }
    }
    #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
    fn observe_rx(&mut self, bytes: u32, early: u32, now: u64) {
        if self.published.is_none() || self.done.is_some() { return; }
        self.max_rx_bytes = self.max_rx_bytes.max(bytes);
        if bytes == 0 && early == 0 { self.last_rx_empty = Some(now); }
    }
    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
    fn data(&mut self, endpoint: Option<crate::qcom_glink_qmi::TftpEndpointTrace>,
        route: crate::qcom_glink_qmi::TftpReplyRoute, now: u64) {
        if endpoint.is_some_and(|e| e.source == route.source && e.destination == route.destination)
            && !self.different_peer && self.queued.is_some() {
            self.matching_data = self.matching_data.saturating_add(1);
            if self.first_matching_data.is_none() { self.first_matching_data = Some(now); }
        }
    }
}

#[cfg(all(test, feature = "sunfish-tftp-timing-observer"))]
mod tftp_timing_tests {
    use super::TftpTiming;
    use crate::qcom_qmi_router::Address;

    #[cfg(feature = "sunfish-tftp-peer-close-observer")]
    #[test]
    fn first_peer_close_records_only_matching_port_and_latches_phase() {
        let mut t = TftpTiming::default();
        t.peer_closed(31, 50);
        t.request(Address { node: 0, port: 31 }, 100);
        t.peer_closed(32, 110);
        assert_eq!(t.peer_close, None);
        t.queued(Address { node: 0, port: 31 }, 120);
        t.observe(4, 130);
        t.peer_closed(31, 140);
        t.observe(12, 150);
        t.peer_closed(31, 160);
        assert_eq!((t.peer_close, t.peer_close_phase), (Some(140), 3));
    }

    #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
    #[test]
    fn rx_empty_observation_excludes_early_queue_and_stops_at_completion() {
        let peer = Address { node: 0, port: 31 };
        let mut t = TftpTiming::default();
        t.request(peer, 100);
        t.queued(peer, 110);
        t.observe_rx(0, 0, 120);
        assert_eq!(t.last_rx_empty, None);
        t.observe(5, 180);
        t.observe_rx(0, 1, 190);
        assert_eq!(t.last_rx_empty, None);
        t.observe_rx(64, 0, 200);
        t.observe_rx(0, 0, 210);
        t.observe_rx(32, 0, 220);
        assert_eq!((t.last_rx_empty, t.max_rx_bytes), (Some(210), 64));
        t.observe(13, 230);
        t.observe_rx(0, 0, 240);
        assert_eq!(t.last_rx_empty, Some(210));
    }

    #[cfg(feature = "sunfish-tftp-host-gap-observer")]
    #[test]
    fn host_gap_tracks_only_published_to_completion_observations() {
        let peer = Address { node: 0, port: 31 };
        let mut t = TftpTiming::default();
        t.request(peer, 100);
        t.queued(peer, 110);
        t.observe(1, 120);
        t.observe(5, 180);
        t.observe(5, 190);
        t.observe(13, 500);
        t.observe(13, 900);
        assert_eq!((t.published, t.done), (Some(180), Some(500)));
        assert_eq!((t.max_host_gap, t.host_observations), (310, 3));
    }

    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
    #[test]
    fn data_observation_requires_exact_first_oack_peer_and_transfer_port() {
        use crate::qcom_glink_qmi::{TftpEndpointTrace, TftpReplyRoute};
        let peer = Address { node: 0, port: 31 };
        let local = Address { node: 1, port: 0x5001 };
        let route = TftpReplyRoute { source: local, destination: peer };
        let endpoint = TftpEndpointTrace { source: local, destination: peer,
            payload_bytes: 39, flags: 1, remote_intent: u32::MAX };
        let mut t = TftpTiming::default();
        t.request(peer, 100);
        t.data(Some(endpoint), route, 105);
        assert_eq!(t.matching_data, 0, "an OACK must first be queued");
        t.queued(peer, 110);
        t.data(None, route, 115);
        t.data(Some(endpoint), TftpReplyRoute { source: Address { port: 0x5002, ..local }, ..route }, 120);
        t.data(Some(endpoint), TftpReplyRoute { destination: Address { port: 32, ..peer }, ..route }, 125);
        assert_eq!(t.matching_data, 0);
        t.data(Some(endpoint), route, 130);
        t.data(Some(endpoint), route, 140);
        assert_eq!((t.matching_data, t.first_matching_data), (2, Some(130)));
    }

    #[test]
    fn first_request_and_matching_reply_keep_first_observed_milestones() {
        let peer = Address { node: 0, port: 31 };
        let mut t = TftpTiming::default();
        t.request(peer, 100);
        t.request(peer, 120);
        t.queued(peer, 150);
        t.observe(1, 160);
        t.observe(5, 180);
        #[cfg(feature = "sunfish-tftp-host-gap-observer")]
        t.observe(5, 190);
        t.observe(13, 200);
        t.observe(13, 220);
        assert_eq!((t.request, t.queued, t.published, t.done), (Some(100), Some(150), Some(180), Some(200)));
        #[cfg(feature = "sunfish-tftp-host-gap-observer")]
        assert_eq!((t.max_host_gap, t.host_observations), (10, 3));
        assert!(!t.different_peer);
    }

    #[test]
    fn unrelated_or_missing_request_never_produces_paired_latencies() {
        let first = Address { node: 0, port: 31 };
        let other = Address { node: 0, port: 32 };
        for with_request in [false, true] {
            let mut t = TftpTiming::default();
            if with_request { t.request(first, 100); }
            t.queued(other, 200);
            t.observe(13, 300);
            assert_eq!(t.different_peer, with_request);
            assert_eq!((t.queued, t.published, t.done), (None, None, None));
        }
    }
}

pub struct Owner<I> {
    #[cfg(feature = "sunfish-linux-rmtfs")]
    linux_rmtfs_id: Option<u64>,
    #[cfg(feature = "sunfish-linux-rmtfs")]
    rmtfs_large_reply: Option<RequestHandle>,
    #[cfg(feature = "sunfish-linux-rmtfs")]
    rmtfs_large_reply_flags: u32,
    #[cfg(feature = "sunfish-linux-rmtfs")]
    rmtfs_event_clock: Option<fn() -> u64>,
    #[cfg(feature = "sunfish-linux-rmtfs")]
    rmtfs_event_times: [Option<u64>; 5],
    #[cfg(feature = "sunfish-tftp-timing-observer")]
    tftp_timing: TftpTiming,
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    linux_ipa: Option<linux_ipa_owner::LinuxRuntime>,
    adapter: Adapter<I>,
    generation: u64,
    stopped: bool,
    error: Option<Error>,
    pending_irqs: u64,
    scheduled: bool,
    requests: [Option<RequestHandle>; MAX_REQUESTS],
    tftp_requests: [Option<TftpHandle>; MAX_REQUESTS],
    #[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
    tftp_yield_debt: bool,
    #[cfg(feature = "sunfish-ipa-registration-rx-first")]
    first_oack_rx_passes: u32,
    #[cfg(feature = "sunfish-first-oack-owner-turn")]
    first_oack_initial_turn_used: bool,
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pending_tftp_close: Option<crate::qcom_qmi_router::Address>,
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    tftp_close_commits: u32,
    #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
    test_fail_close_owner_stage: u8,
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    tftp_owner_snapshot: [(Option<crate::qcom_qmi_router::Address>, u8); crate::qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS],
    #[cfg(feature = "sunfish-tftp-close-owner-fence")]
    firmware_peers: [Option<crate::qcom_qmi_router::Address>; crate::qcom_qrtr_tftp::MAX_ACTIVE_TRANSFERS],
    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
    pending_firmware_close: Option<crate::qcom_qmi_router::Address>,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    queue_full_kind: u32,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    tftp_wrq_coalesced: u32,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    tftp_wire_counts: [u32; 4],
    #[cfg(feature = "sunfish-wlan-read-sequence")]
    read_sequence: ReadSequence,
    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    first_wlan_rrq_at: Option<u64>,
    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    first_wlfw_server_at: Option<u64>,
    #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
    first_rrq: Option<RequestObservation>,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    tftp_wait_reason: u32,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    tftp_wait_tx: u32,
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    tftp_wait_flows: u32,
    tftp_output: [u8; crate::qcom_tftp::MAX_REQUEST_BYTES],
    pending_tftp_reply: Option<PendingTftpReply>,
    #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
    peer_tftp_replies: [Option<PeerTftpReply>; MAX_REQUESTS],
    #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
    peer_tftp_cursor: usize,
    handlers_id: Option<u64>,
    service_registry_trace: ServiceRegistryTrace,
    first_service_registry_reply_handle: Option<RequestHandle>,
    #[cfg(feature = "sunfish-wlan-startup-trace")]
    first_registry_replies_by_name: [Option<RequestHandle>; crate::qcom_servreg::NAME_COUNTER_COUNT],
    first_service_registry_reply_qrtr_resume_handle: Option<RequestHandle>,
    #[cfg(feature = "sunfish-wlan-elf-timing")]
    elf_lookup_handle: Option<RequestHandle>,
    #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
    registry_txn4_handle: Option<RequestHandle>,
    tftp_trace: TftpObserverTrace,
}
impl<I: Io> Owner<I> {
    /// Run a short native-I/O operation through the same adapter retained by
    /// this serialized owner. The callback cannot retain an adapter borrow.
    #[allow(dead_code)] // Used when the post-auth receive owner is composed.
    pub fn with_io<T>(&mut self, action: impl FnOnce(&mut I) -> T) -> T {
        self.adapter.with_io(action)
    }

    fn bind_handlers<B: Backend>(
        &mut self,
        handlers: &mut Handlers<B>,
        activate: bool,
    ) -> Result<(), Error> {
        #[cfg(feature = "sunfish-linux-rmtfs")]
        if self.linux_rmtfs_id.is_some() {
            handlers.fault();
            return Err(self.stop(Error::LinuxRmtfs(
                crate::sunfish_linux_rmtfs_server::Error::Binding,
            )));
        }
        let profile = handlers.configuration();
        let config = self.adapter.configuration();
        if profile.generation != self.generation
            || profile.local_node != config.local_node
            || profile.peer_node != config.peer_node
            || self
                .adapter
                .endpoint(Service::RemoteStorage)
                .is_none_or(|e| e.address.port != profile.storage_port)
            || self
                .adapter
                .endpoint(Service::HeapMemory)
                .is_none_or(|e| e.address.port != profile.memory_port)
            || (self.handlers_id != Some(handlers.id())
                && !(activate && self.handlers_id.is_none()))
        {
            handlers.fault();
            return Err(self.stop(Error::Service(actions::Error::Configuration)));
        }
        self.handlers_id = Some(handlers.id());
        Ok(())
    }
    /// Call only with real backing data/reservations and a confirmed backend.
    /// The native runtime deliberately has no such backend attached yet.
    pub fn activate_handlers<B: Backend>(
        &mut self,
        handlers: &mut Handlers<B>,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        if let Err(error) = self.check() {
            handlers.fault();
            return Err(error);
        }
        self.bind_handlers(handlers, true)?;
        self.stopped = true;
        if let Err(error) = handlers.validate() {
            return Err(self.stop(Error::Service(error)));
        }
        self.stopped = false;
        for service in [Service::RemoteStorage, Service::HeapMemory] {
            if let Err(error) = self.set_service_available(service, true, interrupts) {
                handlers.fault();
                return Err(self.stop(error));
            }
        }
        Ok(())
    }
    /// One action IO step, followed by at most one cached reply enqueue. A
    /// blocked reply is retained without repeating disk or ownership effects.
    pub fn dispatch_handlers_step<B: Backend>(
        &mut self,
        handlers: &mut Handlers<B>,
        interrupts: &mut impl Interrupts,
    ) -> Result<ServiceProgress, Error> {
        if let Err(error) = self.check() {
            handlers.fault();
            return Err(error);
        }
        self.bind_handlers(handlers, false)?;
        self.stopped = true;
        let result: Result<actions::Progress, Error> = (|| {
            self.adapter.revalidate(self.generation)?;
            if handlers.active_handle().is_none() {
                let mut selected = None;
                for handle in self.requests.iter().flatten().copied() {
                    let context = self
                        .adapter
                        .request_qmi(self.generation, &handle)
                        .map(|(context, _)| context)?;
                    if handlers.handles_service(context.service) {
                        selected = Some(handle);
                        break;
                    }
                }
                let Some(handle) = selected else {
                    handlers.validate().map_err(Error::Service)?;
                    return Ok(actions::Progress::Idle);
                };
                let (context, request) = self.adapter.request(self.generation, &handle)?;
                handlers
                    .begin(handle, context, request)
                    .map_err(Error::Service)?;
            }
            if !self
                .requests
                .iter()
                .any(|slot| *slot == handlers.active_handle())
            {
                return Err(Error::Service(actions::Error::InvalidRequest));
            }
            handlers.step().map_err(Error::Service)
        })();
        let progress = match result {
            Ok(progress) => {
                self.stopped = false;
                progress
            }
            Err(error) => {
                handlers.fault();
                return Err(self.stop(error));
            }
        };
        match progress {
            actions::Progress::Idle => Ok(ServiceProgress::Idle),
            actions::Progress::Progress => {
                if let Err(error) = self.schedule(interrupts) {
                    handlers.fault();
                    return Err(error);
                }
                Ok(ServiceProgress::Progress)
            }
            actions::Progress::ReplyReady => {
                let handle = handlers.active_handle().unwrap();
                let response = handlers.response().unwrap();
                match self.queue_reply_inner(&handle, response.reply(), interrupts) {
                    Ok(()) => {
                        if let Err(error) = handlers.reply_queued(&handle) {
                            handlers.fault();
                            return Err(self.stop(Error::Service(error)));
                        }
                        Ok(ServiceProgress::ReplyQueued)
                    }
                    Err(Error::Transport(fifo::Error::Session(
                        crate::qcom_glink_qmi::Error::FlowBusy
                        | crate::qcom_glink_qmi::Error::NoCapacity,
                    ))) => Ok(ServiceProgress::WaitingReply),
                    Err(error) => {
                        handlers.fault();
                        Err(error)
                    }
                }
            }
        }
    }
    pub fn notified_with_handlers<B: Backend>(
        &mut self,
        channels: u64,
        handlers: &mut Handlers<B>,
        interrupts: &mut impl Interrupts,
    ) -> Result<CombinedProgress, Error> {
        if let Err(error) = self.check() {
            handlers.fault();
            return Err(error);
        }
        self.bind_handlers(handlers, false)?;
        self.stopped = true;
        if let Err(error) = handlers.validate() {
            return Err(self.stop(Error::Service(error)));
        }
        self.stopped = false;
        let transport = match self.notified_inner(channels, interrupts) {
            Ok(p) => p,
            Err(e) => {
                handlers.fault();
                return Err(e);
            }
        };
        let service = self.dispatch_handlers_step(handlers, interrupts)?;
        Ok(CombinedProgress { transport, service })
    }
    /// Trusted owner boundary: adapter belongs to this boot's authenticated,
    /// exclusively owned negotiated channel. This does not authenticate a peer.
    pub fn attach(mut adapter: Adapter<I>, generation: u64) -> Result<Self, Error> {
        adapter.revalidate(generation)?;
        Ok(Self {
            adapter,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            linux_rmtfs_id: None,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_large_reply: None,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_large_reply_flags: 0,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_event_clock: None,
            #[cfg(feature = "sunfish-linux-rmtfs")]
            rmtfs_event_times: [None; 5],
            #[cfg(feature = "sunfish-tftp-timing-observer")]
            tftp_timing: TftpTiming::default(),
            #[cfg(feature = "sunfish-linux-ipa-qmi")]
            linux_ipa: None,
            generation,
            stopped: false,
            error: None,
            pending_irqs: 0,
            scheduled: false,
            requests: [None; MAX_REQUESTS],
            tftp_requests: [None; MAX_REQUESTS],
            #[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
            tftp_yield_debt: false,
            #[cfg(feature = "sunfish-ipa-registration-rx-first")]
            first_oack_rx_passes: 0,
            #[cfg(feature = "sunfish-first-oack-owner-turn")]
            first_oack_initial_turn_used: false,
            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
            pending_tftp_close: None,
            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
            tftp_close_commits: 0,
            #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
            test_fail_close_owner_stage: 0,
            #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
            tftp_owner_snapshot: [(None, 0); crate::qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS],
            #[cfg(feature = "sunfish-tftp-close-owner-fence")]
            firmware_peers: [None; crate::qcom_qrtr_tftp::MAX_ACTIVE_TRANSFERS],
            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
            pending_firmware_close: None,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            queue_full_kind: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wrq_coalesced: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wire_counts: [0; 4],
            #[cfg(feature = "sunfish-wlan-read-sequence")]
            read_sequence: Default::default(),
            #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
            first_wlan_rrq_at: None,
            #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
            first_wlfw_server_at: None,
            #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
            first_rrq: None,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wait_reason: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wait_tx: 0,
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            tftp_wait_flows: 0,
            tftp_output: [0; crate::qcom_tftp::MAX_REQUEST_BYTES],
            pending_tftp_reply: None,
            #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
            peer_tftp_replies: [None; MAX_REQUESTS],
            #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
            peer_tftp_cursor: 0,
            handlers_id: None,
            service_registry_trace: ServiceRegistryTrace::new(),
            first_service_registry_reply_handle: None,
            #[cfg(feature = "sunfish-wlan-startup-trace")]
            first_registry_replies_by_name: [None; crate::qcom_servreg::NAME_COUNTER_COUNT],
            first_service_registry_reply_qrtr_resume_handle: None,
            #[cfg(feature = "sunfish-wlan-elf-timing")]
            elf_lookup_handle: None,
            #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
            registry_txn4_handle: None,
            tftp_trace: TftpObserverTrace::new(),
        })
    }
    fn check(&self) -> Result<(), Error> {
        if self.stopped {
            Err(Error::Stopped)
        } else if cfg!(feature = "sunfish-tftp-wlan-close-handoff") && self.firmware_close_is_pending() {
            Err(Error::Stopped)
        } else {
            Ok(())
        }
    }
    fn firmware_close_is_pending(&self) -> bool {
        #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
        { self.pending_firmware_close.is_some() }
        #[cfg(not(feature = "sunfish-tftp-wlan-close-handoff"))]
        { false }
    }
    fn stop(&mut self, error: Error) -> Error {
        #[cfg(feature = "sunfish-linux-ipa-qmi")]
        if let Some(runtime) = self.linux_ipa.as_mut() {
            runtime.fault();
        }
        self.stopped = true;
        self.error.get_or_insert(error);
        self.adapter.suspend();
        error
    }
    pub fn fault(&mut self) {
        self.stop(Error::Stopped);
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    /// Native transport detail from the validated FIFO backend, if available.
    pub fn transport_diagnostic(&self) -> u32 {
        self.adapter.io_diagnostic()
    }
    pub fn capacity_diagnostic(&self) -> Option<crate::qcom_glink_qmi::CapacityDiagnostic> {
        self.adapter.capacity_diagnostic()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn linux_ipa_tx_diagnostic(&self) -> Option<crate::qcom_glink_qmi::LinuxIpaTxDiagnostic> {
        self.adapter.linux_ipa_tx_diagnostic()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn linux_ipa_receipt_pending(&self) -> Option<bool> {
        self.adapter.linux_ipa_receipt_pending()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn linux_ipa_receive_available(&self) -> u32 {
        self.adapter.linux_ipa_receive_available()
    }
    #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
    pub fn intent_denial_window(&self) -> Option<crate::qcom_glink_qmi::IntentDenialWindow> {
        self.adapter.intent_denial_window()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn intent_denial_diagnostic(&self) -> Option<crate::qcom_glink_qmi::IntentDenialDiagnostic> {
        self.adapter.intent_denial_diagnostic()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn receive_snapshot(&self) -> crate::qcom_glink_qmi::ReceiveSnapshot {
        self.adapter.receive_snapshot()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn routing_diagnostic(&self) -> Option<crate::qcom_glink_qmi::RoutingDiagnostic> {
        self.adapter.routing_diagnostic()
    }
    pub fn session_failure_diagnostic(&self) -> Option<fifo::SessionFailureDiagnostic> {
        self.adapter.session_failure_diagnostic()
    }
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    pub fn open_diagnostic(&self) -> Option<fifo::OpenDiagnostic> {
        self.adapter.open_diagnostic()
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn crash_evidence(&self) -> Option<crate::qcom_modem_crash::Evidence> {
        self.adapter.crash_evidence()
    }
    pub fn in_flight(&self) -> usize {
        self.adapter.in_flight()
    }
    /// Bits: queued, fully published, GLINK RX_DONE, QRTR RESUME_TX, success.
    /// These are observational milestones, not proof of EFS interpretation.
    #[cfg(feature = "sunfish-linux-rmtfs")]
    pub fn rmtfs_large_reply_flags(&self) -> u32 {
        self.rmtfs_large_reply_flags
    }
    /// Optional observational clock; event times use the caller's clock origin.
    #[cfg(feature = "sunfish-linux-rmtfs")]
    pub fn set_rmtfs_event_clock(&mut self, clock: fn() -> u64) {
        self.rmtfs_event_clock = Some(clock);
        #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
        self.adapter.set_registry_ack_clock(clock);
        #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
        if self.linux_ipa.is_some() { self.adapter.track_first_wrq(); }
    }
    #[cfg(feature = "sunfish-linux-rmtfs")]
    pub fn rmtfs_event_time(&self, index: usize) -> Option<u64> {
        self.rmtfs_event_times.get(index).copied().flatten()
    }
    #[cfg(feature = "sunfish-linux-rmtfs")]
    fn mark_rmtfs_event(&mut self, index: usize) {
        if self.rmtfs_event_times[index].is_none() {
            self.rmtfs_event_times[index] = self.rmtfs_event_clock.map(|clock| clock());
        }
    }
    /// Identity of the attached channel lifecycle, checked again before
    /// identity-bound MSA/QMI operations.
    pub fn identity(&self) -> u64 {
        self.adapter.identity()
    }
    /// Boot generation bound to this serialized native owner.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Revalidate the live FIFO lifecycle without polling or publishing data.
    /// A failed observation permanently suspends this owner.
    pub fn revalidate(&mut self) -> Result<(), Error> {
        self.check()?;
        match self.adapter.revalidate(self.generation) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    /// Startup-only configuration. The caller must kick initial bounded work
    /// before entering the IRQ loop. Never attach control to an active session.
    pub fn start_control(&mut self) -> Result<(), Error> {
        self.check()?;
        match self.adapter.start_control(self.generation) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    /// Register and stage the compiled, manifest-derived SERVREG endpoint
    /// before QRTR control starts. This only changes local bounded state; the
    /// endpoint is announced after HELLO and no transport work is scheduled.
    pub fn prepare_service_registry(&mut self) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.register(
            self.generation,
            Service::ServiceRegistry,
            crate::qcom_servreg::LOCAL_PORT,
        ) {
            return Err(self.stop(error.into()));
        }
        if let Err(error) = self
            .adapter
            .prepare_service_available(self.generation, Service::ServiceRegistry)
        {
            return Err(self.stop(error.into()));
        }
        Ok(())
    }
    /// Register the AP IPA QMI endpoint without announcing it. Call this
    /// during startup; publish availability only after a live IPA provider
    /// has completed and validated AP hardware initialization.
    pub fn prepare_ipa_server_endpoint(&mut self, local_port: u32) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self
            .adapter
            .register(self.generation, Service::IpaServer, local_port)
        {
            return Err(self.stop(error.into()));
        }
        Ok(())
    }
    /// Announce the AP IPA QMI service after its trusted runtime provider has
    /// established live hardware readiness. No IPA MMIO or initialization is
    /// performed here.
    pub fn enable_ipa_server(
        &mut self,
        readiness: &crate::qcom_ipa_service::ApReadyPermit,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        readiness
            .check(self.generation)
            .map_err(|_| Error::IpaReadinessRequired)?;
        self.set_service_available_inner(Service::IpaServer, true, interrupts)
    }
    /// Advertise the raw TFTP service for a metadata-only observation run.
    /// This observer sends no TFTP reply and never supplies image bytes.
    pub fn prepare_tftp_observer(&mut self, local_port: u32) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self
            .adapter
            .prepare_tftp_observer(self.generation, local_port)
        {
            return Err(self.stop(error.into()));
        }
        Ok(())
    }
    /// Prepare a QRTR TFTP endpoint whose inbound datagrams are held for the
    /// caller's bounded server dispatcher. The caller must provide an
    /// independently authorized immutable image and retain any response that
    /// encounters transport backpressure before finishing the request.
    pub fn prepare_tftp_server(&mut self, local_port: u32) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self
            .adapter
            .prepare_tftp_server(self.generation, local_port)
        {
            return Err(self.stop(error.into()));
        }
        Ok(())
    }
    /// Report whether the exact TFTP server endpoint is already staged in the
    /// attached, pristine QRTR session. This lets service adapters avoid a
    /// duplicate pre-attach registration without trusting an external flag.
    pub fn tftp_server_prepared(&self, local_port: u32) -> bool {
        self.adapter
            .tftp_server_prepared(self.generation, local_port)
    }
    /// Register the compiled, manifest-derived SERVREG endpoint after QRTR
    /// control starts. It is independent of storage/heap backend readiness.
    pub fn enable_service_registry(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.register(
            self.generation,
            Service::ServiceRegistry,
            crate::qcom_servreg::LOCAL_PORT,
        ) {
            return Err(self.stop(error.into()));
        }
        self.set_service_available(Service::ServiceRegistry, true, interrupts)
    }
    /// Reserve the native owner's local QRTR port for WLFW registration before
    /// direct-peer control starts. This is a QMI client endpoint, not an
    /// advertised service and does not cross the MSA ownership gate.
    pub fn enable_wlfw_client(&mut self, local_port: u32) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self.adapter.enable_wlfw_client(self.generation, local_port) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    /// Reserve WLFW while requiring a source-checked ICNSS power permit before
    /// the session can queue indication registration. The production Sunfish
    /// runtime uses this path; observer/restart probes may explicitly retain
    /// `enable_wlfw_client` when they only reproduce Linux client behavior.
    pub fn enable_wlfw_client_power_gated(&mut self, local_port: u32) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self
            .adapter
            .enable_wlfw_client_power_gated(self.generation, local_port)
        {
            Ok(()) => Ok(()),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    /// Release the first WLFW request only after the ICNSS power sequencer
    /// proves that its generation-matched vote plan is active.
    pub fn authorize_wlfw_power(
        &mut self,
        permit: crate::qcom_wlan_power::PowerReadyPermit,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.authorize_wlfw_power(self.generation, permit) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Reserve the native owner's local QRTR port for the one-shot IPA
    /// `INIT_MODEM_DRIVER` client. This does not initialize the IPA provider,
    /// send QMI, or imply that WLAN is ready.
    pub fn enable_ipa_client(&mut self, local_port: u32) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self.adapter.enable_ipa_client(self.generation, local_port) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    pub fn ipa_state(&self) -> Option<crate::qcom_ipa_client::State> {
        self.adapter.ipa_state()
    }
    pub fn ipa_outcome(&self) -> Option<crate::qcom_ipa_client::ResponseOutcome> {
        self.adapter.ipa_outcome()
    }
    /// Queue the single IPA request from caller-supplied, source-validated
    /// runtime fields and schedule the serialized owner. Callers must obtain
    /// those fields from a trusted live IPA resource provider; this method
    /// does not discover or grant IPA resources itself.
    pub fn begin_ipa_init_modem_driver(
        &mut self,
        fields: &crate::qcom_ipa::InitRequestFields,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self
            .adapter
            .begin_ipa_init_modem_driver(self.generation, fields)
        {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Reserve one local QRTR endpoint for the read-only `wlan/fw`
    /// SERVREG_NOTIF state query before control starts.
    pub fn enable_servreg_notif_client(&mut self, local_port: u32) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self
            .adapter
            .enable_servreg_notif_client(self.generation, local_port)
        {
            Ok(()) => Ok(()),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    pub fn servreg_notif_state(&self) -> Option<crate::qcom_servreg_notif_client::State> {
        self.adapter.servreg_notif_state()
    }
    pub fn servreg_notif_outcome(&self) -> Option<crate::qcom_servreg_notif_client::Outcome> {
        self.adapter.servreg_notif_outcome()
    }
    /// Queue one `msm/modem/wlan_pd` state query to the announced SERVREG_NOTIF service,
    /// then schedule the serialized owner.
    pub fn begin_servreg_notif_query(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.begin_servreg_notif_query(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Queue the one-shot `wlan/fw` PD restart only after the SERVREG client
    /// has accepted a successful UNINIT query response, then schedule owner
    /// publication through the same validated FIFO path.
    pub fn begin_servreg_notif_restart_pd(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.begin_servreg_notif_restart_pd(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Register for SERVREG state notifications after a successful UNINIT
    /// query, matching the pinned Linux client sequence before PD restart.
    pub fn begin_servreg_notif_listener(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.begin_servreg_notif_listener(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    pub fn wlfw_state(&self) -> Option<crate::qcom_wlfw_client::State> {
        self.adapter.wlfw_state()
    }
    pub fn wlfw_outcome(&self) -> Option<crate::qcom_wlfw_client::RegistrationOutcome> {
        self.adapter.wlfw_outcome()
    }
    pub fn wlfw_msa_info_outcome(&self) -> Option<crate::qcom_wlfw_client::MsaInfoOutcome> {
        self.adapter.wlfw_msa_info_outcome()
    }
    pub fn wlfw_capability_summary(&self) -> Option<crate::qcom_wlfw_client::CapabilitySummary> {
        self.adapter.wlfw_capability_summary()
    }
    pub fn wlfw_dynamic_feature_mask_done(&self) -> bool {
        self.adapter.wlfw_dynamic_feature_mask_done()
    }
    pub fn wlfw_last_indication(&self) -> Option<crate::qcom_wlfw_client::IndicationOutcome> {
        self.adapter.wlfw_last_indication()
    }

    /// Reports the bound WLFW server's FW_READY status without conflating it
    /// with CE, WMI, or network-interface readiness.
    pub fn wlfw_firmware_ready_seen(&self) -> bool {
        self.adapter.wlfw_firmware_ready_seen()
    }
    /// Advance the registered WLFW client through MSA_INFO using only the
    /// validated fixed-pool token. This queues QMI metadata; it does not grant
    /// ownership, configure SMMU, or make MSA_READY eligible.
    pub fn begin_wlfw_msa_info(
        &mut self,
        reservation: MsaReservation,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self
            .adapter
            .begin_wlfw_msa_info(self.generation, reservation)
        {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Continue the startup exchange with a permit issued after both fixed MSA
    /// assignments returned exact success statuses. The matched interface has
    /// no independent ownership readback. The owner revalidates its FIFO
    /// lifecycle before queuing the fixed MSA_READY request.
    pub fn begin_wlfw_msa_ready(
        &mut self,
        permit: &mut crate::qcom_msa::MsaReadyPermit,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.begin_wlfw_msa_ready(self.generation, permit) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Queue WLFW capability discovery after an accepted MSA_READY response.
    pub fn begin_wlfw_capability_request(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.begin_wlfw_capability_request(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Report the bounded WLFW transport's observed timeout for the
    /// best-effort ICNSS feature-mask request so startup may continue.
    pub fn complete_wlfw_dynamic_feature_mask_timeout(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self
            .adapter
            .complete_wlfw_dynamic_feature_mask_timeout(self.generation)
        {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    /// Queue the first BDF segment only with a board-data token issued by the
    /// future device/source verifier. This method does not create that token.
    pub fn begin_wlfw_bdf_download(
        &mut self,
        image: &crate::qcom_wlfw_bdf::VerifiedBdfImage<'_>,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.begin_wlfw_bdf_download(self.generation, image) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    pub fn continue_wlfw_bdf_download(
        &mut self,
        image: &crate::qcom_wlfw_bdf::VerifiedBdfImage<'_>,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self
            .adapter
            .continue_wlfw_bdf_download(self.generation, image)
        {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    pub fn begin_wlfw_empty_cal_report(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.begin_wlfw_empty_cal_report(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.schedule(interrupts)
    }
    pub fn control_status(&self) -> crate::qcom_qrtr_control::Status {
        self.adapter.control_status()
    }
    pub fn last_completion_error(&self) -> Option<crate::qcom_glink_qmi::CompletionDiagnostic> {
        self.adapter.last_completion_error()
    }
    pub fn last_credit_diagnostic(&self) -> Option<crate::qcom_glink_qmi::CreditDiagnostic> {
        self.adapter.last_credit_diagnostic()
    }
    pub fn client_gone_diagnostic(&self) -> Option<crate::qcom_glink_qmi::ClientGoneDiagnostic> {
        self.adapter.client_gone_diagnostic()
    }
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    pub fn deferred_close_diagnostic(&self) -> Option<crate::qcom_glink_qmi::DeferredCloseDiagnostic> {
        self.adapter.deferred_close_diagnostic()
    }
    /// Kinds in the last pre-poll snapshot for the closing peer: request=1,
    /// legacy retained reply=2, per-peer retained reply=4.
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    pub fn deferred_close_owner_kinds(&self) -> Option<u32> {
        let close = self.adapter.deferred_close_diagnostic()?;
        Some(self.tftp_owner_snapshot.iter().fold(0, |mask, (address, kind)| {
            if address.is_some_and(|peer| peer.node == close.peer_node && peer.port == close.peer_port) { mask | u32::from(*kind) } else { mask }
        }))
    }
    /// Refresh while holding exclusive access to the server and before any
    /// transport polling. A stale snapshot cannot authorize close recovery.
    #[cfg(feature = "sunfish-tftp-close-owner-fence")]
    pub fn snapshot_tftp_firmware_peers<S: crate::qcom_tftp::ReadSource + ?Sized>(
        &mut self, server: &crate::qcom_qrtr_tftp::Server<'_, S>,
    ) -> Result<(), Error> {
        self.check()?;
        self.firmware_peers = server.active_peers().map(|peer| peer.map(|p|
            crate::qcom_qmi_router::Address { node: p.node, port: p.port }));
        Ok(())
    }
    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
    pub fn pending_firmware_close(&self) -> Option<crate::qcom_qmi_router::Address> { self.pending_firmware_close }

    /// The transport holds the close and prohibits publication until this
    /// exclusive, checked owner/server transition either commits or stops.
    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
    pub fn resolve_firmware_close(&mut self, server: &mut crate::qcom_qrtr_tftp::Server<'_>) -> Result<(), Error> {
        if self.stopped { return Err(Error::Stopped); }
        let peer = self.pending_firmware_close.ok_or(Error::Stopped)?;
        let kinds = self.tftp_owner_snapshot.iter().filter(|(p, _)| *p == Some(peer)).fold(0u8, |m, (_, k)| m | *k);
        let key = crate::qcom_qrtr_tftp::Peer { node: peer.node, port: peer.port };
        let retained = if kinds == (8 | 4) {
            let mut matching = self.peer_tftp_replies.iter().enumerate()
                .filter(|(_, r)| r.as_ref().is_some_and(|r| r.meta.route.destination == peer));
            let first = matching.next().and_then(|(index, reply)| reply.as_ref().map(|r| (index, r)));
            match first {
                Some((index, reply)) if matching.next().is_none()
                    && reply.meta.route.source.node == 1
                    && server.owns_transfer(key, reply.meta.route.source.port) => Some(index),
                _ => return Err(self.stop(Error::Transport(fifo::Error::Session(crate::qcom_glink_qmi::Error::Control(
                    crate::qcom_qrtr_control::Error::ClientGone))))),
            }
        } else { None };
        if !matches!(kinds, 8 | 12) || !server.active_peers().contains(&Some(key)) {
            return Err(self.stop(Error::Transport(fifo::Error::Session(crate::qcom_glink_qmi::Error::Control(
                crate::qcom_qrtr_control::Error::ClientGone)))));
        }
        self.stopped = true;
        if let Err(error) = self.adapter.commit_firmware_close(self.generation, peer) { return Err(self.stop(error.into())); }
        if server.close_peer(key) != 1 { return Err(self.stop(Error::Stopped)); }
        if let Some(slot) = retained { self.peer_tftp_replies[slot] = None; }
        self.firmware_peers = server.active_peers().map(|p| p.map(|p| crate::qcom_qmi_router::Address { node: p.node, port: p.port }));
        self.pending_firmware_close = None;
        self.stopped = false;
        Ok(())
    }
    /// Read-only preflight for the narrow server-check cancellation. The
    /// caller must independently validate the close and transport state, and
    /// must hold exclusive access until commit. This does not cancel anything.
    #[cfg(all(feature = "sunfish-tftp-peer-close-experiment", feature = "sunfish-tftp-per-peer-reply-experiment", feature = "sunfish-native-tftp-server-check"))]
    pub fn can_cancel_server_check_reply(&mut self, peer: crate::qcom_qmi_router::Address,
        server: &crate::qcom_tftp_server_check::ServerCheck) -> bool {
        if self.check().is_err() || self.pending_tftp_reply.is_some_and(|reply| reply.route.destination == peer) {
            return false;
        }
        for handle in self.tftp_requests.iter().flatten() {
            match self.adapter.tftp_datagram(self.generation, handle) {
                Ok((route, _)) if route.destination != peer => {},
                _ => return false,
            }
        }
        let mut replies = self.peer_tftp_replies.iter().flatten().filter(|reply| reply.meta.route.destination == peer);
        let Some(reply) = replies.next() else { return false; };
        if replies.next().is_some() || reply.meta.route.source.node != 1
            || !crate::qcom_tftp_server_check::is_transfer_port(reply.meta.route.source.port)
            || reply.meta.bytes != crate::qcom_tftp_server_check::OACK.len()
            || reply.bytes[..reply.meta.bytes] != *crate::qcom_tftp_server_check::OACK {
            return false;
        }
        server.can_abandon_empty_peer_at(crate::qcom_tftp_server_check::Address { node: peer.node, port: peer.port }, reply.meta.route.source.port).is_ok()
    }

    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pub fn pending_tftp_close(&self) -> Option<crate::qcom_qmi_router::Address> { self.pending_tftp_close }
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pub fn tftp_close_commits(&self) -> u32 { self.tftp_close_commits }

    #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
    pub(crate) fn test_fail_close_stage(&mut self, stage: u8) { self.adapter.test_fail_close_stage(stage); }
    #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
    pub(crate) fn test_fail_close_owner_stage(&mut self, stage: u8) { self.test_fail_close_owner_stage = stage; }

    /// Host-only prototype; commits while the transport is held and the owner
    /// and server are exclusively borrowed. Uncertain failures stop the owner.
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pub fn resolve_pending_tftp_close(&mut self, server: &mut crate::qcom_tftp_server_check::ServerCheck) -> Result<(), Error> {
        self.check()?;
        let peer = self.pending_tftp_close.ok_or(Error::Stopped)?;
        if !self.can_cancel_server_check_reply(peer, server) {
            return Err(self.stop(Error::Transport(fifo::Error::Session(crate::qcom_glink_qmi::Error::Control(
                crate::qcom_qrtr_control::Error::ClientGone)))));
        }
        let slot = self.peer_tftp_replies.iter().position(|reply| reply.as_ref().is_some_and(|r| r.meta.route.destination == peer)).ok_or(Error::Stopped)?;
        self.stopped = true;
        if let Err(error) = self.adapter.commit_tftp_close(self.generation, peer) {
            return Err(self.stop(error.into()));
        }
        #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
        if self.test_fail_close_owner_stage == 1 { return Err(self.stop(Error::Stopped)); }
        if server.abandon_empty_peer(crate::qcom_tftp_server_check::Address { node: peer.node, port: peer.port }).is_err() {
            return Err(self.stop(Error::Stopped));
        }
        #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
        if self.test_fail_close_owner_stage == 2 { return Err(self.stop(Error::Stopped)); }
        self.peer_tftp_replies[slot] = None;
        self.pending_tftp_close = None;
        self.tftp_close_commits = self.tftp_close_commits.saturating_add(1);
        self.stopped = false;
        Ok(())
    }
    pub fn retired_rmtfs_peer_count(&self) -> usize {
        self.adapter.retired_rmtfs_peer_count()
    }
    pub fn remote_server(&self, index: usize) -> Option<crate::qcom_qrtr_control::Server> {
        self.adapter.remote_server(index)
    }
    pub fn wlfw_endpoint(&mut self) -> Result<Option<crate::qcom_qmi_router::Address>, Error> {
        self.check()?;
        match self.adapter.wlfw_endpoint(self.generation) {
            Ok(endpoint) => Ok(endpoint),
            Err(error) => Err(self.stop(error.into())),
        }
    }

    /// Trusted backend readiness, not decoder registration. A withdrawing
    /// backend retains outstanding requests/leases until orderly teardown.
    pub fn set_service_available(
        &mut self,
        service: Service,
        ready: bool,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if service == Service::IpaServer && ready {
            return Err(Error::IpaReadinessRequired);
        }
        self.set_service_available_inner(service, ready, interrupts)
    }
    fn set_service_available_inner(
        &mut self,
        service: Service,
        ready: bool,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        match self
            .adapter
            .set_service_available(self.generation, service, ready)
        {
            Ok(()) => {}
            Err(
                error @ fifo::Error::Session(crate::qcom_glink_qmi::Error::Control(
                    crate::qcom_qrtr_control::Error::Busy,
                )),
            ) => return Err(error.into()),
            Err(error) => return Err(self.stop(error.into())),
        }
        self.schedule(interrupts)
    }
    fn schedule(&mut self, interrupts: &mut impl Interrupts) -> Result<(), Error> {
        if !self.scheduled {
            self.stopped = true;
            if let Err(error) = interrupts.schedule() {
                return Err(self.stop(error));
            }
            self.scheduled = true;
            self.stopped = false;
        }
        Ok(())
    }
    pub fn pending_request(&self) -> Option<RequestHandle> {
        self.requests.iter().flatten().next().copied()
    }
    /// Find the oldest queued request owned by one service dispatcher. Requests
    /// for other services share this queue and must not block SERVREG or IPA
    /// while their dedicated runtime adapters are waiting to run.
    pub fn pending_request_for_service(
        &mut self,
        service: Service,
    ) -> Result<Option<RequestHandle>, Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.stopped = true;
        let result: Result<Option<RequestHandle>, fifo::Error> = (|| {
            for handle in self.requests.iter().flatten().copied() {
                let (context, _) = self.adapter.request_qmi(self.generation, &handle)?;
                if context.service == service {
                    return Ok(Some(handle));
                }
            }
            Ok(None)
        })();
        self.stopped = false;
        match result {
            Ok(handle) => Ok(handle),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    pub fn intent_ack_summary(&self) -> (u32, Option<u64>) {
        self.adapter.intent_ack_summary()
    }
    pub fn service_registry_trace(&self) -> ServiceRegistryTrace {
        let trace = self.service_registry_trace;
        #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
        let trace = ServiceRegistryTrace {
            elf_exchange: self.adapter.elf_exchange(),
            #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
            first_server_check: self.adapter.first_server_check(),
            #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
            first_server_check_tail: self.adapter.first_server_check_tail(),
            ..trace
        };
        trace
    }
    pub fn tftp_observer_trace(&self) -> TftpObserverTrace {
        self.tftp_trace
    }
    /// First queue that filled: 1 is routed QMI, 2 is held TFTP.
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn queue_full_kind(&self) -> u32 { self.queue_full_kind }
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_wrq_coalesced(&self) -> u32 { self.tftp_wrq_coalesced }
    /// Incoming WRQ, RRQ, WLAN-image RRQ and DATA packet counts. Metadata only.
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_wire_counts(&self) -> [u32; 4] { self.tftp_wire_counts }
    #[cfg(feature = "sunfish-wlan-read-sequence")]
    pub fn read_sequence(&self) -> &ReadSequence { &self.read_sequence }

    #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
    pub fn wlan_discovery_times(&self) -> (Option<u64>, Option<u64>) {
        (self.first_wlan_rrq_at, self.first_wlfw_server_at)
    }
    #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
    pub fn first_rrq(&self) -> Option<RequestObservation> { self.first_rrq }
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_wait_diagnostic(&self) -> (u32, u32, u32, u32) {
        (self.tftp_wait_reason, self.tftp_wait_tx, self.tftp_wait_flows,
         self.pending_tftp_peer_port())
    }
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    fn pending_tftp_peer_port(&self) -> u32 {
        #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
        if let Some(reply) = self.peer_tftp_replies.iter().flatten().next() { return reply.meta.route.destination.port; }
        self.pending_tftp_reply.map_or(u32::MAX, |r| r.route.destination.port)
    }
    fn has_pending_tftp_reply(&self) -> bool {
        #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
        if self.peer_tftp_replies.iter().any(Option::is_some) { return true; }
        self.pending_tftp_reply.is_some()
    }
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_queue_snapshot(&self) -> (crate::qcom_glink_qmi::TftpQueueSnapshot, bool) {
        (self.adapter.tftp_queue_snapshot(), self.has_pending_tftp_reply())
    }
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_tx_progress(&self) -> [u32; 3] { self.adapter.tftp_tx_progress() }
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    pub fn tftp_endpoint_trace(&self) -> Option<crate::qcom_glink_qmi::TftpEndpointTrace> { self.adapter.tftp_endpoint_trace() }
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    pub fn tftp_final_ack_trace(&self) -> Option<crate::qcom_glink_qmi::TftpFinalAckTrace> { self.adapter.tftp_final_ack_trace() }
    #[cfg(feature = "sunfish-tftp-oack-reserve")]
    pub fn reserved_oack(&self) -> Option<(
        crate::qcom_glink_qmi::ReservedOack, Option<crate::qcom_glink_qmi::ServerCheckOackTrace>,
    )> {
        self.adapter.reserved_oack()
    }

    #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
    pub fn oack_close_fifo(&self) -> Option<fifo::ServerCheckRxDone> {
        self.adapter.oack_close_fifo()
    }

    #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
    pub fn reserved_oack_ahead(&self) -> Option<&fifo::OackAheadCapture> {
        self.adapter.reserved_oack_ahead()
    }
    #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
    pub fn reserved_writer_io(&self) -> fifo::ReservedWriterIoTrace { self.adapter.reserved_writer_io() }

    #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
    pub fn reserved_oack_rx(&self) -> Option<fifo::ServerCheckRxDone> {
        self.adapter.reserved_oack_rx()
    }

    #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
    pub fn oack_peer_tail(&self) -> Option<fifo::OackPeerTail> {
        self.adapter.oack_peer_tail()
    }

    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
    pub fn oack_slot_follow(&self) -> Option<crate::qcom_glink_qmi::OackSlotFollow> {
        self.adapter.oack_slot_follow()
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    pub fn oack_resources(&self, transfer_port: u32) -> Option<&crate::qcom_glink_qmi::OackResources> {
        self.adapter.oack_resources(transfer_port)
    }

    #[cfg(feature = "sunfish-tftp-server-inventory")]
    pub fn server_check_inventory(&self) -> &[Option<crate::qcom_glink_qmi::ServerCheckOackTrace>] {
        self.adapter.server_check_inventory()
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    pub fn server_check_49(&self) -> Option<crate::qcom_glink_qmi::ServerCheckOackTrace> {
        self.adapter.server_check_49()
    }

    #[cfg(feature = "sunfish-first-oack-grant-window")]
    pub fn first_grant_trace(&self) -> fifo::FirstGrantTrace { self.adapter.first_grant_trace() }

    #[cfg(feature = "sunfish-first-oack-grant-window")]
    pub fn first_oack_tail(&self) -> fifo::ServerCheckTail { self.adapter.first_server_check_tail() }

    #[cfg(feature = "sunfish-post-reg-oack-turn")]
    pub fn post_reg_oack_turns(&self) -> &crate::qcom_glink_qmi::PostRegOackTurns {
        self.adapter.post_reg_oack_turns()
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    pub fn first_oack_eligibility(&self) -> crate::qcom_glink_qmi::FirstOackEligibility {
        self.adapter.first_oack_eligibility()
    }

    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    pub fn server_check_oacks(&self) -> [Option<crate::qcom_glink_qmi::ServerCheckOackTrace>; 2] {
        self.adapter.server_check_oacks()
    }
    #[cfg(feature = "sunfish-wlan-first-wrq-window")]
    pub fn first_wrq_window(&self) -> crate::qcom_glink_qmi::FirstWrqWindow {
        self.adapter.first_wrq_window()
    }
    #[cfg(feature = "sunfish-tftp-first-oack-headers")]
    pub fn first_oack_headers(&self) -> Option<fifo::FirstOackHeaders> {
        self.adapter.first_oack_headers()
    }
    #[cfg(feature = "sunfish-ipa-registration-rx-first")]
    pub fn first_oack_rx_passes(&self) -> u32 {
        self.first_oack_rx_passes
    }
    #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
    pub fn registration_rx_visibility(&self) -> Option<fifo::ServerCheckRxDone> {
        self.adapter.registration_rx_visibility()
    }
    #[cfg(feature = "sunfish-ipa-ingress-work")]
    pub fn ipa_ingress_work(&self) -> &fifo::IpaIngressWork { self.adapter.ipa_ingress_work() }
    #[cfg(feature = "sunfish-wrq-prefix-close")]
    pub fn wrq_prefix(&self) -> &fifo::WrqPrefix { self.adapter.wrq_prefix() }

    #[cfg(feature = "sunfish-ipa-announcement-fifo")]
    pub fn ipa_announcement_rx(&self) -> Option<fifo::ServerCheckRxDone> {
        self.adapter.ipa_announcement_rx()
    }

    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    pub fn ipa_wrq_timing(&self) -> (Option<u64>, Option<fifo::ServerCheckRxDone>) {
        self.adapter.ipa_wrq_timing()
    }
    #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
    pub fn first_oack_rx_done(&self) -> Option<fifo::ServerCheckRxDone> {
        self.adapter.first_oack_rx_done()
    }
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    pub fn mcfg_trace(&self) -> (Option<crate::qcom_glink_qmi::TftpEndpointTrace>, Option<crate::qcom_glink_qmi::McfgDataTrace>) {
        self.adapter.mcfg_trace()
    }
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub fn tftp_missing_reply_trace(&self) -> Option<crate::qcom_glink_qmi::TftpEndpointTrace> { self.adapter.tftp_missing_reply_trace() }


    #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
    pub fn tftp_schedule_trace(&self) -> crate::qcom_glink_qmi::TftpScheduleTrace { self.adapter.tftp_schedule_trace() }
    #[cfg(feature = "sunfish-tftp-timing-observer")]
    pub fn tftp_timing(&self) -> TftpTiming { self.tftp_timing }
    #[cfg(feature = "sunfish-tftp-timing-observer")]
    fn observe_tftp_timing(&mut self) {
        if let (Some(clock), Some(trace)) = (self.rmtfs_event_clock, self.adapter.tftp_endpoint_trace()) {
            let now = clock();
            self.tftp_timing.observe(trace.flags, now);
            #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
            if let Some((bytes, early)) = self.adapter.tftp_rx_snapshot() {
                self.tftp_timing.observe_rx(bytes, early, now);
            }
            #[cfg(feature = "sunfish-tftp-tail-observer")]
            if self.adapter.tftp_tail_seen() && self.tftp_timing.queued.is_some()
                && self.tftp_timing.peer_tail.is_none() {
                self.tftp_timing.peer_tail = Some(now);
            }
        }
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn outbound_intent_diagnostic(&self) -> (u32, u32) {
        self.adapter.outbound_intent_diagnostic()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn outbound_intent_attempt_diagnostic(&self) -> (u32, u32) {
        self.adapter.outbound_intent_attempt_diagnostic()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_diagnostic(&self) -> (u32, u32, u32) {
        self.adapter.intent_gate_diagnostic()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_combinations(&self) -> (u32, u32) {
        self.adapter.intent_gate_combinations()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_queue_snapshot(&self) -> (u32, u32, u32) {
        self.adapter.intent_gate_queue_snapshot()
    }
    /// Return the oldest complete TFTP datagram held for the bounded server
    /// dispatcher. Its receive slot remains unavailable until explicitly
    /// finished, so the payload cannot be overwritten while being served.
    pub fn pending_tftp_request(&self) -> Option<TftpHandle> {
        self.tftp_requests.iter().flatten().next().copied()
    }
    pub fn tftp_datagram(
        &mut self,
        handle: &TftpHandle,
    ) -> Result<(crate::qcom_glink_qmi::TftpReplyRoute, &[u8]), Error> {
        self.check()?;
        if !self
            .tftp_requests
            .iter()
            .flatten()
            .any(|queued| queued == handle)
        {
            return Err(self.stop(Error::Transport(fifo::Error::Session(
                crate::qcom_glink_qmi::Error::InvalidHandle,
            ))));
        }
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.stopped = true;
        match self.adapter.tftp_datagram(self.generation, handle) {
            Ok(value) => {
                self.stopped = false;
                Ok(value)
            }
            Err(error) => {
                let error = error.into();
                self.error = Some(error);
                Err(error)
            }
        }
    }
    /// Release one TFTP receive slot after the dispatcher has either queued a
    /// response or retained that response for retry in its own bounded state.
    pub fn finish_tftp_request(
        &mut self,
        handle: &TftpHandle,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        let Some(slot) = self
            .tftp_requests
            .iter()
            .position(|queued| queued.as_ref() == Some(handle))
        else {
            return Err(self.stop(Error::Transport(fifo::Error::Session(
                crate::qcom_glink_qmi::Error::InvalidHandle,
            ))));
        };
        match self.adapter.finish_tftp_request(self.generation, handle) {
            Ok(()) => self.tftp_requests[slot] = None,
            Err(error) => return Err(self.stop(error.into())),
        }
        self.schedule(interrupts)
    }
    /// Dispatch one held QRTR TFTP packet through the fixed-capacity transfer
    /// table. A generated response is retained in owner memory if the GLINK
    /// flow or transmit table is temporarily busy; it is never regenerated
    /// from the already-advanced transfer state.
    pub fn service_tftp_step<S: crate::qcom_tftp::ReadSource + ?Sized>(
        &mut self,
        server: &mut crate::qcom_qrtr_tftp::Server<'_, S>,
        interrupts: &mut impl Interrupts,
    ) -> Result<TftpServiceProgress, Error> {
        self.check()?;
        if self.pending_tftp_reply.is_some() {
            return self.flush_tftp_reply(interrupts);
        }
        let Some(handle) = self.pending_tftp_request() else {
            return Ok(TftpServiceProgress::Idle);
        };
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.stopped = true;
        let (reply_route, packet) = match self.adapter.tftp_datagram(self.generation, &handle) {
            Ok(value) => value,
            Err(error) => {
                let error = error.into();
                self.error = Some(error);
                return Err(error);
            }
        };
        let incoming = crate::qcom_qrtr_tftp::IncomingRoute {
            destination_node: reply_route.source.node,
            destination_port: reply_route.source.port,
            source: crate::qcom_qrtr_tftp::Peer {
                node: reply_route.destination.node,
                port: reply_route.destination.port,
            },
        };
        let dispatch = server.receive(incoming, packet, &mut self.tftp_output);
        self.stopped = false;

        let outcome = match dispatch {
            crate::qcom_qrtr_tftp::Dispatch::Send { route, bytes } => {
                self.pending_tftp_reply = Some(PendingTftpReply {
                    route: crate::qcom_glink_qmi::TftpReplyRoute {
                        source: crate::qcom_qmi_router::Address {
                            node: route.source_node,
                            port: route.source_port,
                        },
                        destination: crate::qcom_qmi_router::Address {
                            node: route.destination.node,
                            port: route.destination.port,
                        },
                    },
                    bytes,
                });
                None
            }
            crate::qcom_qrtr_tftp::Dispatch::Closed { reason, .. } => {
                Some(TftpServiceProgress::Closed(reason))
            }
            crate::qcom_qrtr_tftp::Dispatch::Ignored(reason) => {
                Some(TftpServiceProgress::Ignored(reason))
            }
        };
        self.finish_tftp_request(&handle, interrupts)?;
        if let Some(outcome) = outcome {
            return Ok(outcome);
        }
        self.flush_tftp_reply(interrupts)
    }

    /// Service only the bounded, RAM-backed server-check write. A response is
    /// retained before releasing the receive slot, including under backpressure.
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn service_tftp_server_check_step(
        &mut self,
        server: &mut crate::qcom_tftp_server_check::ServerCheck,
        interrupts: &mut impl Interrupts,
    ) -> Result<TftpServiceProgress, Error> {
        #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
        { return self.service_tftp_server_check_per_peer(server, #[cfg(feature = "sunfish-tftp-wlan-mux")] None, interrupts); }
        #[cfg(not(feature = "sunfish-tftp-per-peer-reply-experiment"))]
        {
        self.check()?;
        if self.pending_tftp_reply.is_some() {
            return self.flush_tftp_reply(interrupts);
        }
        let Some(handle) = self.pending_tftp_request() else {
            return Ok(TftpServiceProgress::Idle);
        };
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.stopped = true;
        let (route, packet) = match self.adapter.tftp_datagram(self.generation, &handle) {
            Ok(value) => value,
            Err(error) => {
                let error = error.into();
                self.error = Some(error);
                return Err(error);
            }
        };
        let incoming = crate::qcom_tftp_server_check::Route {
            source: crate::qcom_tftp_server_check::Address { node: route.destination.node, port: route.destination.port },
            destination: crate::qcom_tftp_server_check::Address { node: route.source.node, port: route.source.port },
        };
        let dispatch = server.receive(incoming, packet, &mut self.tftp_output);
        self.stopped = false;
        if let Some(reply) = dispatch.ok().filter(|reply| reply.len != 0) {
            self.pending_tftp_reply = Some(PendingTftpReply {
                route: crate::qcom_glink_qmi::TftpReplyRoute {
                    source: crate::qcom_qmi_router::Address { node: reply.route.source.node, port: reply.route.source.port },
                    destination: crate::qcom_qmi_router::Address { node: reply.route.destination.node, port: reply.route.destination.port },
                },
                bytes: reply.len,
            });
        }
        self.finish_tftp_request(&handle, interrupts)?;
        if dispatch.is_err() {
            return Ok(TftpServiceProgress::Ignored(crate::qcom_qrtr_tftp::IgnoreReason::InvalidTransferPacket));
        }
        self.flush_tftp_reply(interrupts)
        }
    }

    #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
    fn flush_peer_tftp_reply(&mut self, slot: usize, interrupts: &mut impl Interrupts)
        -> Result<bool, Error> {
        let Some(reply) = self.peer_tftp_replies[slot].as_ref() else { return Ok(false); };
        #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
        if self.adapter.cancel_closed_tftp_retry(&reply.request) {
            self.peer_tftp_replies[slot] = None;
            return Ok(false);
        }
        #[cfg(feature = "sunfish-tftp-timing-observer")]
        let first_oack_peer = (self.adapter.tftp_endpoint_trace().is_none()
            && reply.bytes[..reply.meta.bytes].starts_with(&[0, 6]))
            .then_some(reply.meta.route.destination);
        if let Err(error) = self.adapter.revalidate(self.generation) { return Err(self.stop(error.into())); }
        #[cfg(not(feature = "sunfish-wlan-first-server-check-repeat"))]
        let queued = self.adapter.queue_tftp_response(self.generation, reply.meta.route, &reply.bytes[..reply.meta.bytes]);
        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
        let queued = {
            #[cfg(feature = "sunfish-tftp-wlan-mux")]
            let firmware = (0x5100..=0x51ff).contains(&reply.meta.route.source.port);
            #[cfg(not(feature = "sunfish-tftp-wlan-mux"))]
            let firmware = false;
            if firmware { self.adapter.queue_tftp_response(self.generation, reply.meta.route, &reply.bytes[..reply.meta.bytes]) }
            else { self.adapter.queue_traced_tftp_response(self.generation, &reply.request, reply.meta.route, &reply.bytes[..reply.meta.bytes]) }
        };
        match queued {
            Ok(()) => {
                #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                if reply.bytes[..reply.meta.bytes].starts_with(&[0, 6]) {
                    self.adapter.observe_registration_tftp_oack_queued(reply.request);
                }
                #[cfg(feature = "sunfish-tftp-timing-observer")]
                if let (Some(peer), Some(clock)) = (first_oack_peer, self.rmtfs_event_clock) {
                    self.tftp_timing.queued(peer, clock());
                }
                self.peer_tftp_replies[slot] = None;
                self.peer_tftp_cursor = (slot + 1) % MAX_REQUESTS;
                self.schedule(interrupts)?;
                #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
                if self.adapter.early_reserved_oack_pending() {
                    if let Err(error) = self.adapter.publish_oack_followup(
                        self.generation, crate::qcom_glink_qmi::OackFollowup::EarlyReserve,
                    ) {
                        return Err(self.stop(error.into()));
                    }
                }
                Ok(true)
            }
            Err(fifo::Error::Session(error @ (crate::qcom_glink_qmi::Error::FlowBusy | crate::qcom_glink_qmi::Error::NoCapacity))) => {
                let snapshot = self.adapter.tftp_queue_snapshot();
                self.tftp_wait_reason = if error == crate::qcom_glink_qmi::Error::FlowBusy { 1 } else { 2 };
                self.tftp_wait_tx = snapshot.all_tx_used;
                self.tftp_wait_flows = snapshot.flow_used;
                Ok(false)
            }
            Err(error) => Err(self.stop(error.into())),
        }
    }

    /// Bounded experiment: one immutable retained response per peer, without
    /// allowing a blocked peer to prevent other held requests from dispatching.
    #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
    fn service_tftp_server_check_per_peer(&mut self, server: &mut crate::qcom_tftp_server_check::ServerCheck,
        #[cfg(feature = "sunfish-tftp-wlan-mux")] firmware: Option<&mut crate::qcom_qrtr_tftp::Server<'_>>,
        interrupts: &mut impl Interrupts) -> Result<TftpServiceProgress, Error> {
        self.check()?;
        for offset in 0..MAX_REQUESTS {
            let slot = (self.peer_tftp_cursor + offset) % MAX_REQUESTS;
            if self.flush_peer_tftp_reply(slot, interrupts)? { return Ok(TftpServiceProgress::ResponseQueued); }
        }
        let Some(free) = self.peer_tftp_replies.iter().position(Option::is_none) else {
            return Ok(TftpServiceProgress::WaitingReply);
        };
        let mut selected = None;
        for handle in self.tftp_requests.iter().flatten().copied() {
            let (route, _) = match self.adapter.tftp_datagram(self.generation, &handle) {
                Ok(value) => value,
                Err(error) => return Err(self.stop(error.into())),
            };
            if !self.peer_tftp_replies.iter().flatten().any(|reply| reply.meta.route.destination == route.destination) {
                selected = Some(handle);
                break;
            }
        }
        let Some(handle) = selected else {
            return Ok(if self.has_pending_tftp_reply() { TftpServiceProgress::WaitingReply } else { TftpServiceProgress::Idle });
        };
        if let Err(error) = self.adapter.revalidate(self.generation) { return Err(self.stop(error.into())); }
        #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
        self.adapter.observe_elf_server_check_dispatch_start(&handle);
        self.stopped = true;
        let (route, packet) = match self.adapter.tftp_datagram(self.generation, &handle) {
            Ok(value) => value,
            Err(error) => { let error = error.into(); self.error = Some(error); return Err(error); }
        };
        let incoming = crate::qcom_tftp_server_check::Route {
            source: crate::qcom_tftp_server_check::Address { node: route.destination.node, port: route.destination.port },
            destination: crate::qcom_tftp_server_check::Address { node: route.source.node, port: route.source.port },
        };
        #[cfg(feature = "sunfish-tftp-wlan-mux")]
        if let Some(firmware) = firmware
            && ((incoming.destination.port == crate::qcom_tftp_server_check::SERVICE_PORT
                && crate::qcom_tftp::decode_read_request(packet).is_ok())
                || (0x5100..=0x51ff).contains(&incoming.destination.port))
        {
            use crate::qcom_qrtr_tftp::{Dispatch, IncomingRoute, Peer};
            let result = firmware.receive(IncomingRoute {
                destination_node: incoming.destination.node, destination_port: incoming.destination.port,
                source: Peer { node: incoming.source.node, port: incoming.source.port },
            }, packet, &mut self.tftp_output);
            self.firmware_peers = firmware.active_peers().map(|p| p.map(|p|
                crate::qcom_qmi_router::Address { node: p.node, port: p.port }));
            self.stopped = false;
            let outcome = match result {
                Dispatch::Send { route, bytes: len } => {
                    let mut bytes = [0; crate::qcom_tftp::MAX_REQUEST_BYTES];
                    bytes[..len].copy_from_slice(&self.tftp_output[..len]);
                    self.peer_tftp_replies[free] = Some(PeerTftpReply { meta: PendingTftpReply {
                        route: crate::qcom_glink_qmi::TftpReplyRoute {
                            source: crate::qcom_qmi_router::Address { node: route.source_node, port: route.source_port },
                            destination: crate::qcom_qmi_router::Address { node: route.destination.node, port: route.destination.port },
                        }, bytes: len }, bytes,
                        #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
                        request: handle,
                    });
                    None
                },
                Dispatch::Closed { reason, .. } => Some(TftpServiceProgress::Closed(reason)),
                Dispatch::Ignored(reason) => Some(TftpServiceProgress::Ignored(reason)),
            };
            self.finish_tftp_request(&handle, interrupts)?;
            if let Some(outcome) = outcome { return Ok(outcome); }
            return Ok(if self.flush_peer_tftp_reply(free, interrupts)? { TftpServiceProgress::ResponseQueued } else { TftpServiceProgress::WaitingReply });
        }
        #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
        let occupied = server.active_count() as u8;
        let dispatch = server.receive(incoming, packet, &mut self.tftp_output);
        self.stopped = false;
        #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
        let server_check_wrq = packet.starts_with(crate::qcom_tftp_server_check::REQUEST_PREFIX);
        if let Some(reply) = dispatch.ok().filter(|reply| reply.len != 0) {
            #[cfg(feature = "sunfish-tftp-mcfg-trace")]
            if packet.starts_with(b"\0\x02/readwrite/mcfg.tmp\0")
                && &self.tftp_output[..reply.len] == crate::qcom_tftp_server_check::MCFG_WRITE_OACK
            {
                self.adapter.observe_mcfg_oack(crate::qcom_glink_qmi::TftpReplyRoute {
                    source: crate::qcom_qmi_router::Address { node: reply.route.source.node, port: reply.route.source.port },
                    destination: crate::qcom_qmi_router::Address { node: reply.route.destination.node, port: reply.route.destination.port },
                });
            }
            #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
            if reply.missing_kind != 0 {
                self.adapter.observe_tftp_missing_reply(crate::qcom_glink_qmi::TftpReplyRoute {
                    source: crate::qcom_qmi_router::Address { node: reply.route.source.node, port: reply.route.source.port },
                    destination: crate::qcom_qmi_router::Address { node: reply.route.destination.node, port: reply.route.destination.port },
                });
            }
            #[cfg(feature = "sunfish-tftp-final-ack-observer")]
            if reply.completed {
                self.adapter.observe_tftp_final_ack(crate::qcom_glink_qmi::TftpReplyRoute {
                    source: crate::qcom_qmi_router::Address { node: reply.route.source.node, port: reply.route.source.port },
                    destination: crate::qcom_qmi_router::Address { node: reply.route.destination.node, port: reply.route.destination.port },
                }, &self.tftp_output[..reply.len]);
            }
            let mut bytes = [0; crate::qcom_tftp::MAX_REQUEST_BYTES];
            bytes[..reply.len].copy_from_slice(&self.tftp_output[..reply.len]);
            self.peer_tftp_replies[free] = Some(PeerTftpReply {
                meta: PendingTftpReply {
                    route: crate::qcom_glink_qmi::TftpReplyRoute {
                        source: crate::qcom_qmi_router::Address { node: reply.route.source.node, port: reply.route.source.port },
                        destination: crate::qcom_qmi_router::Address { node: reply.route.destination.node, port: reply.route.destination.port },
                    }, bytes: reply.len,
                }, bytes,
                #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
                request: handle,
            });
        }
        #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
        self.adapter.observe_elf_server_check_dispatch(
            &handle, route, server_check_wrq, dispatch, &self.tftp_output,
        );
        #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
        self.adapter.observe_registration_tftp_dispatch(
            handle, dispatch.err().map_or(0, |error| error.code() as u8), occupied,
        );
        self.finish_tftp_request(&handle, interrupts)?;
        if dispatch.is_err() { return Ok(TftpServiceProgress::Ignored(crate::qcom_qrtr_tftp::IgnoreReason::InvalidTransferPacket)); }
        if dispatch.is_ok_and(|reply| reply.len == 0) { return Ok(TftpServiceProgress::Idle); }
        Ok(if self.flush_peer_tftp_reply(free, interrupts)? { TftpServiceProgress::ResponseQueued } else { TftpServiceProgress::WaitingReply })
    }

    #[cfg(feature = "sunfish-tftp-wlan-mux")]
    pub fn service_combined_tftp_step(&mut self, server: &mut crate::qcom_tftp_server_check::ServerCheck,
        firmware: &mut crate::qcom_qrtr_tftp::Server<'_>, interrupts: &mut impl Interrupts)
        -> Result<TftpServiceProgress, Error> {
        self.snapshot_tftp_firmware_peers(firmware)?;
        self.service_tftp_server_check_per_peer(server, Some(firmware), interrupts)
    }

    fn flush_tftp_reply(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<TftpServiceProgress, Error> {
        let Some(reply) = self.pending_tftp_reply else {
            return Ok(TftpServiceProgress::Idle);
        };
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self.adapter.queue_tftp_response(
            self.generation,
            reply.route,
            &self.tftp_output[..reply.bytes],
        ) {
            Ok(()) => {
                self.pending_tftp_reply = None;
                self.schedule(interrupts)?;
                Ok(TftpServiceProgress::ResponseQueued)
            }
            Err(fifo::Error::Session(error @ (
                crate::qcom_glink_qmi::Error::FlowBusy | crate::qcom_glink_qmi::Error::NoCapacity
            ))) => {
                #[cfg(feature = "sunfish-native-tftp-server-check")]
                {
                    let snapshot = self.adapter.tftp_queue_snapshot();
                    self.tftp_wait_reason = if error == crate::qcom_glink_qmi::Error::FlowBusy { 1 } else { 2 };
                    self.tftp_wait_tx = snapshot.all_tx_used;
                    self.tftp_wait_flows = snapshot.flow_used;
                }
                #[cfg(not(feature = "sunfish-native-tftp-server-check"))]
                let _ = error;
                Ok(TftpServiceProgress::WaitingReply)
            },
            Err(error) => Err(self.stop(error.into())),
        }
    }
    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
    fn record_registry_queue_wait(&mut self, handle: &RequestHandle) {
        #[cfg(feature = "sunfish-wlan-tx4-pressure")]
        if self.registry_txn4_handle == Some(*handle)
            && let Some(timing) = self.service_registry_trace.txn4.as_mut()
        {
            let pressure = self.adapter.registry_queue_pressure(handle);
            let reason = pressure.map_or(0, |p| usize::from(p.reason));
            timing.waits_by_reason[reason] = timing.waits_by_reason[reason].saturating_add(1);
            timing.last_pressure = pressure;
        }
        if self.elf_lookup_handle != Some(*handle) { return; }
        let pressure = self.adapter.registry_queue_pressure(handle);
        #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
        if let Some(blocker) = self.registry_txn4_handle
            && pressure.and_then(|sample| sample.peer_request) == Some(blocker)
            && let Some(timing) = self.service_registry_trace.txn4.as_mut()
        {
            timing.elf_waits = timing.elf_waits.saturating_add(1);
        }
        if let Some(elf) = self.service_registry_trace.elf.as_mut() {
            let index = pressure.map_or(0, |value| usize::from(value.reason));
            elf.waits_by_reason[index] = elf.waits_by_reason[index].saturating_add(1);
            elf.last_pressure = pressure;
            #[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
            if elf.blocker.is_none()
                && let Some(sample) = pressure
                && sample.reason == 1 && sample.peer_kind == 4 && sample.peer_flags & 2 == 0
                && let (Some(handle), Some(publication)) = (sample.peer_request, sample.peer_publication)
            {
                elf.blocker = Some(ElfReplyBlocker {
                    handle,
                    transaction: publication.transaction,
                    flags_at_wait: sample.peer_flags,
                    wait: self.rmtfs_event_clock.map(|clock| clock()),
                    first_turn: None,
                    published: None,
                    done: None,
                    #[cfg(feature = "sunfish-wlan-elf-completion-turn")]
                    yielded: false,
                    #[cfg(feature = "sunfish-wlan-elf-completion-turn")]
                    yielded_at: None,
                });
            }
        }
    }

    /// One normal registry dispatch, and at most one extra dispatch after
    /// successful queueing while the exact ELF request is still pending.
    /// The opaque retained handle, not diagnostic timestamps, gates the extra
    /// turn. Existing FIFO ordering, peer credit and terminal errors apply.
    #[cfg(feature = "sunfish-servreg-bounded-second-turn")]
    pub fn dispatch_service_registry_bounded(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<(ServiceProgress, Option<ServiceProgress>), Error> {
        let first = self.dispatch_service_registry_step(interrupts)?;
        let elf_pending = self.elf_lookup_handle
            .is_some_and(|handle| self.requests.contains(&Some(handle)));
        if first != ServiceProgress::ReplyQueued || !elf_pending {
            return Ok((first, None));
        }
        let second = self.dispatch_service_registry_step(interrupts)?;
        Ok((first, Some(second)))
    }

    /// Answer one manifest-backed SERVREG LOC request without requiring the
    /// privileged storage/heap backend to be active. Other services remain in
    /// the shared bounded request queue for their normal handler.
    pub fn dispatch_service_registry_step(
        &mut self,
        interrupts: &mut impl Interrupts,
    ) -> Result<ServiceProgress, Error> {
        self.check()?;
        #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
        if let Some(elf) = self.service_registry_trace.elf.as_mut()
            && elf.dispatch.selected.is_none()
            && (elf.dispatch.registry_entered.is_none()
                || cfg!(feature = "sunfish-wlan-elf-registry-turns"))
            && let Some(clock) = self.rmtfs_event_clock
        {
            let now = clock();
            elf.dispatch.registry_entered.get_or_insert(now);
            #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
            elf.dispatch.turns.enter(now);
        }
        let Some(handle) = self.pending_request_for_service(Service::ServiceRegistry)? else {
            return Ok(ServiceProgress::Idle);
        };
        #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
        if self.elf_lookup_handle == Some(handle)
            && let Some(elf) = self.service_registry_trace.elf.as_mut()
            && elf.dispatch.selected.is_none()
        {
            elf.dispatch.selected = self.rmtfs_event_clock.map(|clock| clock());
        }
        self.stopped = true;
        let decoded = self
            .adapter
            .request(self.generation, &handle)
            .map(|(context, request)| {
                if context.service != Service::ServiceRegistry {
                    return Ok(None);
                }
                match request {
                    Request::ServiceRegistry(query) => Ok(Some((
                        crate::qcom_servreg::respond(query),
                        crate::qcom_servreg::service_name_code(query.service_name),
                        #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
                        context.transaction,
                    ))),
                    _ => Err(fifo::Error::Session(
                        crate::qcom_glink_qmi::Error::InvalidReply,
                    )),
                }
            });
        self.stopped = false;
        let response = match decoded {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => return Err(self.stop(error.into())),
            Err(error) => return Err(self.stop(error.into())),
        };
        let Some(response) = response else {
            return Ok(ServiceProgress::Idle);
        };
        #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
        let (response, name_code, transaction) = response;
        #[cfg(not(feature = "sunfish-wlan-elf-registry-turns"))]
        let (response, name_code) = response;
        let queue_result = self.queue_reply_inner(
            &handle,
            Reply::Success {
                optional_tlvs: response.optional_tlvs(),
            },
            interrupts,
        );
        #[cfg(feature = "sunfish-wlan-elf-registry-turns")]
        if let Some(elf) = self.service_registry_trace.elf.as_mut()
            && self.elf_lookup_handle != Some(handle) && elf.dispatch.selected.is_none()
        {
            let outcome = match &queue_result {
                Ok(()) => 1,
                Err(Error::Transport(fifo::Error::Session(crate::qcom_glink_qmi::Error::FlowBusy))) => 2,
                Err(Error::Transport(fifo::Error::Session(crate::qcom_glink_qmi::Error::NoCapacity))) => 3,
                Err(_) => 4,
            };
            elf.dispatch.turns.record(transaction, name_code, outcome, self.rmtfs_event_clock.map(|clock| clock()));
        }
        match queue_result {
            Ok(()) => {
                if self.service_registry_trace.replies_queued == 0 {
                    self.first_service_registry_reply_handle = Some(handle);
                    self.first_service_registry_reply_qrtr_resume_handle = Some(handle);
                }
                #[cfg(feature = "sunfish-wlan-startup-trace")]
                {
                    let name = crate::qcom_servreg::name_counter_index(name_code);
                    let mask = 1 << name;
                    if self.service_registry_trace.queued_names & mask == 0 {
                        self.first_registry_replies_by_name[name] = Some(handle);
                        self.service_registry_trace.queued_names |= mask;
                    }
                }
                #[cfg(feature = "sunfish-wlan-elf-timing")]
                if self.elf_lookup_handle == Some(handle) {
                    #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
                    self.adapter.observe_elf_reply(&handle);
                    if let Some(elf) = self.service_registry_trace.elf.as_mut() {
                        elf.total_domains = Some(response.total_domains());
                        elf.returned_rows = Some(response.domain_list_tlv().and_then(|list| list.first().copied()).unwrap_or(0));
                        elf.queued = self.rmtfs_event_clock.map(|clock| clock());
                        #[cfg(feature = "sunfish-wlan-elf-phase-io")]
                        { elf.io.queued = self.adapter.elf_io_totals(); }
                    }
                }
                #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
                if self.registry_txn4_handle == Some(handle)
                    && let Some(timing) = self.service_registry_trace.txn4.as_mut()
                {
                    timing.queued = self.rmtfs_event_clock.map(|clock| clock());
                }
                self.service_registry_trace.record_reply_queued(name_code);
                Ok(ServiceProgress::ReplyQueued)
            }
            Err(Error::Transport(fifo::Error::Session(crate::qcom_glink_qmi::Error::FlowBusy))) => {
                #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
                self.record_registry_queue_wait(&handle);
                self.service_registry_trace.record_reply_wait(name_code);
                self.service_registry_trace.reply_flow_busy = self
                    .service_registry_trace
                    .reply_flow_busy
                    .saturating_add(1);
                Ok(ServiceProgress::WaitingReply)
            }
            Err(Error::Transport(fifo::Error::Session(
                crate::qcom_glink_qmi::Error::NoCapacity,
            ))) => {
                #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
                self.record_registry_queue_wait(&handle);
                self.service_registry_trace.record_reply_wait(name_code);
                self.service_registry_trace.reply_no_capacity = self
                    .service_registry_trace
                    .reply_no_capacity
                    .saturating_add(1);
                Ok(ServiceProgress::WaitingReply)
            }
            Err(error) => Err(error),
        }
    }
    pub fn request(&mut self, handle: &RequestHandle) -> Result<(Context, Request<'_>), Error> {
        self.check()?;
        // The native gate is checked again by the adapter before lending data.
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.stopped = true;
        match self.adapter.request(self.generation, handle) {
            Ok(value) => {
                self.stopped = false;
                Ok(value)
            }
            Err(error) => {
                let error = error.into();
                self.error = Some(error);
                Err(error)
            }
        }
    }
    /// Borrow the original QMI request bytes for a source-specific service
    /// core while retaining the validated request capability.
    pub fn request_qmi(&mut self, handle: &RequestHandle) -> Result<(Context, &[u8]), Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        self.stopped = true;
        match self.adapter.request_qmi(self.generation, handle) {
            Ok(value) => {
                self.stopped = false;
                Ok(value)
            }
            Err(error) => {
                let error = error.into();
                self.error = Some(error);
                Err(error)
            }
        }
    }
    pub fn queue_reply(
        &mut self,
        handle: &RequestHandle,
        reply: Reply<'_>,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        #[cfg(feature = "sunfish-linux-rmtfs")]
        if self.linux_rmtfs_id.is_some()
            && self.request_qmi(handle)?.0.service == Service::RemoteStorage
        {
            return Err(self.stop(Error::LinuxRmtfs(
                crate::sunfish_linux_rmtfs_server::Error::Request,
            )));
        }
        if self.handlers_id.is_some() {
            return Err(self.stop(Error::Service(actions::Error::InvalidRequest)));
        }
        self.queue_reply_inner(handle, reply, interrupts)
    }
    /// Queue a source-matched AP IPA response. The service token remains
    /// uncommitted until `take_ipa_service_response_completion` reports both
    /// transport acknowledgements.
    pub fn queue_ipa_service_reply(
        &mut self,
        handle: &RequestHandle,
        response: &crate::qcom_ipa_service::PreparedResponse,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        if self.handlers_id.is_some() {
            return Err(self.stop(Error::Service(actions::Error::InvalidRequest)));
        }
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self
            .adapter
            .queue_ipa_service_reply(self.generation, handle, response)
        {
            Ok(()) => {}
            Err(
                error @ fifo::Error::Session(
                    crate::qcom_glink_qmi::Error::FlowBusy
                    | crate::qcom_glink_qmi::Error::NoCapacity,
                ),
            ) => return Err(error.into()),
            Err(error) => return Err(self.stop(error.into())),
        }
        for slot in &mut self.requests {
            if slot.as_ref() == Some(handle) {
                *slot = None;
            }
        }
        self.schedule(interrupts)
    }
    pub fn queue_ipa_master_init_indication(
        &mut self,
        indication: &crate::qcom_ipa_service::PreparedIndication,
        readiness: &crate::qcom_ipa_service::ApReadyPermit,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        readiness
            .check(self.generation)
            .map_err(|_| Error::IpaReadinessRequired)?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self
            .adapter
            .queue_ipa_master_init_indication(self.generation, indication)
        {
            Ok(()) => self.schedule(interrupts),
            Err(
                error @ fifo::Error::Session(
                    crate::qcom_glink_qmi::Error::FlowBusy
                    | crate::qcom_glink_qmi::Error::NoCapacity,
                ),
            ) => Err(error.into()),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    pub fn take_ipa_indication_completion(&mut self) -> Result<Option<u64>, Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self.adapter.take_ipa_indication_completion(self.generation) {
            Ok(value) => Ok(value),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    pub fn take_ipa_service_response_completion(&mut self) -> Result<Option<u64>, Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self
            .adapter
            .take_ipa_service_response_completion(self.generation)
        {
            Ok(value) => Ok(value),
            Err(error) => Err(self.stop(error.into())),
        }
    }
    pub fn queue_tftp_response(
        &mut self,
        route: crate::qcom_glink_qmi::TftpReplyRoute,
        packet: &[u8],
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self
            .adapter
            .queue_tftp_response(self.generation, route, packet)
        {
            Ok(()) => {}
            Err(
                error @ fifo::Error::Session(
                    crate::qcom_glink_qmi::Error::FlowBusy
                    | crate::qcom_glink_qmi::Error::NoCapacity,
                ),
            ) => return Err(error.into()),
            Err(error) => return Err(self.stop(error.into())),
        }
        self.schedule(interrupts)
    }
    fn queue_reply_inner(
        &mut self,
        handle: &RequestHandle,
        reply: Reply<'_>,
        interrupts: &mut impl Interrupts,
    ) -> Result<(), Error> {
        self.check()?;
        if let Err(error) = self.adapter.revalidate(self.generation) {
            return Err(self.stop(error.into()));
        }
        match self.adapter.queue_reply(self.generation, handle, reply) {
            Ok(()) => {}
            Err(
                error @ fifo::Error::Session(
                    crate::qcom_glink_qmi::Error::FlowBusy
                    | crate::qcom_glink_qmi::Error::NoCapacity,
                ),
            ) => return Err(error.into()),
            Err(error) => return Err(self.stop(error.into())),
        }
        for slot in &mut self.requests {
            if slot.as_ref() == Some(handle) {
                *slot = None;
            }
        }
        self.schedule(interrupts)
    }
    pub fn offer_receive(&mut self) -> Result<u32, Error> {
        self.check()?;
        match self.adapter.offer_receive(self.generation) {
            Ok(id) => Ok(id),
            Err(error) => Err(self.stop(error.into())),
        }
    }

    pub fn notified(
        &mut self,
        channels: u64,
        interrupts: &mut impl Interrupts,
    ) -> Result<Progress, Error> {
        if self.handlers_id.is_some() {
            return Err(self.stop(Error::Service(actions::Error::InvalidRequest)));
        }
        self.notified_inner(channels, interrupts)
    }
    fn notified_inner(
        &mut self,
        channels: u64,
        interrupts: &mut impl Interrupts,
    ) -> Result<Progress, Error> {
        self.check()?;
        if channels & !(GLINK | STATUS | CONTINUE) != 0 {
            return Err(self.stop(Error::UnexpectedNotification));
        }
        self.pending_irqs |= channels & (GLINK | STATUS);
        if channels & CONTINUE != 0 {
            self.scheduled = false;
        }
        // Remain stopped if IO unwinds. Only a complete known outcome clears it.
        self.stopped = true;
        let result = self.work(interrupts);
        match result {
            Ok(progress) => {
                #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
                if let Some(elf) = self.service_registry_trace.elf.as_mut()
                    && elf.dispatch.receive_pass_end.is_none()
                {
                    elf.dispatch.receive_pass_end = self.rmtfs_event_clock.map(|clock| clock());
                    #[cfg(feature = "sunfish-wlan-elf-phase-io")]
                    { elf.io.receive_pass_end = self.adapter.elf_io_totals(); }
                }
                self.stopped = false;
                Ok(progress)
            }
            Err(error) => {
                #[cfg(feature = "sunfish-linux-rmtfs")]
                self.mark_rmtfs_event(4);
                Err(self.stop(error))
            },
        }
    }

    fn work(&mut self, interrupts: &mut impl Interrupts) -> Result<Progress, Error> {
        #[cfg(feature = "sunfish-ipa-init-tx-trace")]
        self.adapter.observe_ipa_init_router();
        #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
        self.adapter.observe_retry_turn();
        #[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
        if let Some(blocker) = self.service_registry_trace.elf.as_mut().and_then(|elf| elf.blocker.as_mut())
            && blocker.first_turn.is_none()
        {
            blocker.first_turn = self.rmtfs_event_clock.map(|clock| clock());
        }
        self.adapter.revalidate(self.generation)?;
        let mut progress = Progress::default();
        let mut exhausted = true;
        for _ in 0..WORK_BUDGET {
            #[cfg(all(feature = "sunfish-tftp-deferred-close-experiment", not(feature = "sunfish-tftp-peer-close-experiment")))]
            {
                let busy = self.has_pending_tftp_reply() || self.tftp_requests.iter().any(Option::is_some);
                self.adapter.set_tftp_owner_busy(self.generation, busy)?;
            }
            #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
            {
                let mut peers = [None; crate::qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS];
                let mut snapshot = [(None, 0); crate::qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS];
                let mut count = 0;
                #[cfg(feature = "sunfish-tftp-close-owner-fence")]
                let mut firmware_guard = [None; crate::qcom_glink_qmi::TFTP_OWNER_PEER_SLOTS];
                #[cfg(feature = "sunfish-tftp-close-owner-fence")]
                let mut firmware_count = 0;
                for handle in self.tftp_requests.iter().flatten() {
                    let (route, packet) = self.adapter.tftp_datagram(self.generation, handle)?;
                    #[cfg(feature = "sunfish-tftp-close-owner-fence")]
                    if (route.source.port == crate::qcom_tftp_server_check::SERVICE_PORT && crate::qcom_tftp::decode_read_request(packet).is_ok())
                        || (0x5100..=0x51ff).contains(&route.source.port) {
                        *firmware_guard.get_mut(firmware_count).ok_or(Error::QueueFull)? = Some(route.destination);
                        firmware_count += 1;
                    }
                    #[cfg(not(feature = "sunfish-tftp-close-owner-fence"))]
                    let _ = packet;
                    *peers.get_mut(count).ok_or(Error::QueueFull)? = Some(route.destination);
                    snapshot[count] = (Some(route.destination), 1);
                    count += 1;
                }
                if let Some(reply) = self.pending_tftp_reply {
                    *peers.get_mut(count).ok_or(Error::QueueFull)? = Some(reply.route.destination);
                    snapshot[count] = (Some(reply.route.destination), 2);
                    count += 1;
                }
                #[cfg(feature = "sunfish-tftp-per-peer-reply-experiment")]
                for reply in self.peer_tftp_replies.iter().flatten() {
                    #[cfg(feature = "sunfish-tftp-close-owner-fence")]
                    if (0x5100..=0x51ff).contains(&reply.meta.route.source.port) {
                        *firmware_guard.get_mut(firmware_count).ok_or(Error::QueueFull)? = Some(reply.meta.route.destination);
                        firmware_count += 1;
                    }
                    *peers.get_mut(count).ok_or(Error::QueueFull)? = Some(reply.meta.route.destination);
                    snapshot[count] = (Some(reply.meta.route.destination), 4);
                    count += 1;
                }
                #[cfg(feature = "sunfish-tftp-close-owner-fence")]
                for peer in self.firmware_peers.iter().flatten() {
                    *firmware_guard.get_mut(firmware_count).ok_or(Error::QueueFull)? = Some(*peer);
                    firmware_count += 1;
                    *peers.get_mut(count).ok_or(Error::QueueFull)? = Some(*peer);
                    snapshot[count] = (Some(*peer), 8);
                    count += 1;
                }
                let _ = count;
                self.adapter.set_tftp_owner_peers(self.generation, peers)?;
                #[cfg(feature = "sunfish-tftp-close-owner-fence")]
                { let _ = firmware_count; self.adapter.set_tftp_firmware_peers(self.generation, firmware_guard)?; }
                self.tftp_owner_snapshot = snapshot;
            }
            let rx = self.adapter.poll_receive(self.generation)?;
            #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
            if let Receive::Session(Event::FirmwareClosePending { peer }) = rx {
                self.pending_firmware_close = Some(peer);
                progress.received += 1;
                break;
            }
            #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
            self.adapter.observe_retry_poll_end();
            #[cfg(feature = "sunfish-tftp-timing-observer")]
            self.observe_tftp_timing();
            #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
            if let Receive::Session(Event::TftpClosePending { peer }) = rx {
                #[cfg(feature = "sunfish-tftp-peer-close-observer")]
                if let Some(clock) = self.rmtfs_event_clock { self.tftp_timing.peer_closed(peer.port, clock()); }
                self.pending_tftp_close = Some(peer);
                progress.received += 1;
                break;
            }
            let rx_progress = !matches!(rx, Receive::Idle | Receive::Pending { .. });
            if rx_progress {
                progress.received += 1;
            }
            #[cfg(feature = "sunfish-linux-ipa-qmi")]
            if let Receive::Session(event) = rx {
                self.observe_linux_ipa(event)?;
            }
            if let Receive::Session(Event::Request(handle)) = rx {
                let (context, request) = self.adapter.request(self.generation, &handle)?;
                #[cfg(feature = "sunfish-linux-ipa-qmi")]
                let ipa_request = (context.service == Service::IpaServer).then_some(context.message_id);
                if context.service == Service::ServiceRegistry {
                    match request {
                        Request::ServiceRegistry(query) => {
                            #[cfg(feature = "sunfish-wlan-elf-timing")]
                            if query.service_name == b"kernel/elf_loader" && self.service_registry_trace.elf.is_none() {
                                self.elf_lookup_handle = Some(handle);
                                self.service_registry_trace.elf = Some(ElfLookupTrace {
                                    peer_port: context.peer.port,
                                    domain_offset: query.domain_offset,
                                    total_domains: None, returned_rows: None,
                                    received: self.rmtfs_event_clock.map(|clock| clock()),
                                    queued: None, published: None, done: None,
                                    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
                                    waits_by_reason: [0; 6],
                                    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
                                    last_pressure: None,
                                    #[cfg(feature = "sunfish-wlan-elf-dispatch-timing")]
                                    dispatch: ElfDispatchTiming::default(),
                                    #[cfg(feature = "sunfish-wlan-elf-phase-io")]
                                    io: ElfIoTrace::default(),
                                    #[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
                                    blocker: None,
                                });
                            }
                            #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
                            if context.transaction == 4 && context.peer.node == 0
                                && context.peer.port == 9 && self.service_registry_trace.txn4.is_none()
                            {
                                self.registry_txn4_handle = Some(handle);
                                self.service_registry_trace.txn4 = Some(RegistryTxn4Timing {
                                    name_code: crate::qcom_servreg::service_name_code(query.service_name),
                                    received: self.rmtfs_event_clock.map(|clock| clock()),
                                    ..RegistryTxn4Timing::default()
                                });
                            }
                            self.service_registry_trace.record_query(query);
                        }
                        _ => return Err(Error::Service(actions::Error::InvalidRequest)),
                    }
                }
                // Request decoding above borrows the adapter's packet. No I/O
                // occurs between its receive timestamp and this counter copy.
                #[cfg(feature = "sunfish-wlan-elf-phase-io")]
                if self.elf_lookup_handle == Some(handle)
                    && let Some(elf) = self.service_registry_trace.elf.as_mut()
                    && elf.io.received.is_none()
                {
                    elf.io.received = self.adapter.elf_io_totals();
                }
                let Some(slot) = self.requests.iter_mut().find(|slot| slot.is_none()) else {
                    #[cfg(feature = "sunfish-native-tftp-server-check")]
                    { self.queue_full_kind = 1; }
                    return Err(Error::QueueFull);
                };
                *slot = Some(handle);
                #[cfg(feature = "sunfish-linux-ipa-qmi")]
                if let (Some(message_id), Some(runtime)) = (ipa_request, self.linux_ipa.as_mut()) {
                    runtime.record_server_request(message_id);
                }
                #[cfg(feature = "sunfish-ipa-registration-owner-trace")]
                if ipa_request == Some(crate::sunfish_ipa_linux_qmi::INDICATION_REGISTER) {
                    self.observe_ipa_registration_route(handle);
                }
                progress.requests += 1;
            }
            if let Receive::Session(Event::ReplyRxDone(handle)) = rx {
                #[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
                if let Some(blocker) = self.service_registry_trace.elf.as_mut().and_then(|elf| elf.blocker.as_mut())
                    && blocker.handle == handle
                {
                    blocker.done = self.rmtfs_event_clock.map(|clock| clock());
                }
                #[cfg(feature = "sunfish-wlan-elf-timing")]
                if self.elf_lookup_handle == Some(handle) {
                    if let Some(elf) = self.service_registry_trace.elf.as_mut() {
                        elf.done = self.rmtfs_event_clock.map(|clock| clock());
                    }
                    self.elf_lookup_handle = None;
                }
                #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
                if self.registry_txn4_handle == Some(handle) {
                    if let Some(timing) = self.service_registry_trace.txn4.as_mut() {
                        timing.done = self.rmtfs_event_clock.map(|clock| clock());
                    }
                    self.registry_txn4_handle = None;
                }
                #[cfg(feature = "sunfish-linux-rmtfs")]
                if self.rmtfs_large_reply == Some(handle) { self.rmtfs_large_reply_flags |= 4; self.mark_rmtfs_event(2); }
                #[cfg(feature = "sunfish-wlan-startup-trace")]
                for (name, first) in self.first_registry_replies_by_name.iter_mut().enumerate() {
                    if *first == Some(handle) {
                        self.service_registry_trace.completed_names |= 1 << name;
                        *first = None;
                    }
                }
                if self.first_service_registry_reply_handle == Some(handle) {
                    self.service_registry_trace.first_reply_glink_rx_done = true;
                    self.first_service_registry_reply_handle = None;
                }
            }
            if let Receive::Session(Event::ReplyQrtrResume(handle)) = rx {
                #[cfg(feature = "sunfish-linux-rmtfs")]
                if self.rmtfs_large_reply == Some(handle) { self.rmtfs_large_reply_flags |= 8; self.mark_rmtfs_event(3); }
                if self.first_service_registry_reply_qrtr_resume_handle == Some(handle) {
                    self.service_registry_trace.first_reply_qrtr_resume = true;
                    self.first_service_registry_reply_qrtr_resume_handle = None;
                }
            }
            if let Receive::Session(Event::TftpObserved { peer, observation }) = rx {
                self.tftp_trace.record(peer, observation);
            }
            #[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
            let mut new_tftp_request = false;
            if let Receive::Session(Event::TftpRequest(handle)) = rx {
                #[cfg(feature = "sunfish-native-tftp-server-check")]
                {
                    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
                    let first_endpoint = self.adapter.tftp_endpoint_trace();
                    let (route, packet) = self.adapter.tftp_datagram(self.generation, &handle)?;
                    let opcode = if packet.len() >= 2 { u16::from_be_bytes([packet[0], packet[1]]) } else { 0 };
                    #[cfg(feature = "sunfish-tftp-data-peer-observer")]
                    if opcode == tftp::OP_DATA && packet.len() >= 4 {
                        if let Some(clock) = self.rmtfs_event_clock {
                            self.tftp_timing.data(first_endpoint, route, clock());
                        }
                    }
                    #[cfg(feature = "sunfish-tftp-timing-observer")]
                    if packet == crate::qcom_tftp_server_check::REQUEST {
                        if let Some(clock) = self.rmtfs_event_clock { self.tftp_timing.request(route.destination, clock()); }
                    }
                    #[cfg(not(feature = "sunfish-tftp-timing-observer"))]
                    let _ = route;
                    #[cfg(feature = "sunfish-tftp-first-rrq-observer")]
                    if opcode == tftp::OP_RRQ && self.first_rrq.is_none() {
                        self.first_rrq = tftp::observe_request(packet).ok();
                    }
                    let wlan = opcode == tftp::OP_RRQ && tftp::observe_request(packet)
                        .is_ok_and(|o| o.path_text().ends_with("/wlanmdsp.mbn"));
                    #[cfg(feature = "sunfish-wlan-read-sequence")]
                    if opcode == tftp::OP_RRQ && route.source.port == crate::qcom_tftp_server_check::SERVICE_PORT {
                        let kind = match packet[2..].split(|b| *b == 0).next() {
                            Some(b"/readwrite/ota_firewall/ruleset") => 1,
                            Some(b"/readwrite/modem_efs") => 2,
                            Some(b"/readwrite/mcfg.tmp") => 3,
                            _ if wlan => 4,
                            _ => 5,
                        };
                        let reads = &mut self.read_sequence;
                        if let Some(slot) = reads.first.get_mut(reads.total as usize) {
                            *slot = Some((kind, route.destination.port, self.rmtfs_event_clock.map(|clock| clock())));
                        }
                        reads.total = reads.total.saturating_add(1);
                        reads.kinds |= 1 << kind;
                    }
                    let index = match opcode { tftp::OP_WRQ => Some(0), tftp::OP_RRQ => Some(1), tftp::OP_DATA => Some(3), _ => None };
                    if let Some(index) = index { self.tftp_wire_counts[index] = self.tftp_wire_counts[index].saturating_add(1); }
                    if wlan {
                        self.tftp_wire_counts[2] = self.tftp_wire_counts[2].saturating_add(1);
                        #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
                        if self.first_wlan_rrq_at.is_none() {
                            self.first_wlan_rrq_at = self.rmtfs_event_clock.map(|clock| clock());
                        }
                    }
                }
                #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
                let mut original = None;
                // An exact WRQ retransmission from the same peer can share an
                // already-held request or an OACK retained for that peer. It
                // still receives GLINK RX_DONE; other peers remain distinct.
                #[cfg(feature = "sunfish-native-tftp-server-check")]
                let coalesce_pending_legacy = if let Some(reply) = self.pending_tftp_reply {
                    let (route, packet) = self.adapter.tftp_datagram(self.generation, &handle)?;
                    route.source.node == 1 && route.source.port == crate::qcom_tftp_server_check::SERVICE_PORT
                        && route.destination == reply.route.destination
                        && reply.route.source.node == 1
                        && crate::qcom_tftp_server_check::is_transfer_port(reply.route.source.port)
                        && reply.bytes == crate::qcom_tftp_server_check::OACK.len()
                        && self.tftp_output[..reply.bytes] == *crate::qcom_tftp_server_check::OACK
                        && packet == crate::qcom_tftp_server_check::REQUEST
                } else { false };
                #[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-per-peer-reply-experiment"))]
                let coalesce_pending = if coalesce_pending_legacy { true } else {
                    let (route, packet) = self.adapter.tftp_datagram(self.generation, &handle)?;
                    if route.source.node == 1 && route.source.port == crate::qcom_tftp_server_check::SERVICE_PORT
                        && packet == crate::qcom_tftp_server_check::REQUEST
                    {
                        let retained = self.peer_tftp_replies.iter().flatten().find(|reply|
                            reply.meta.route.destination == route.destination
                            && reply.meta.route.source.node == 1
                            && crate::qcom_tftp_server_check::is_transfer_port(reply.meta.route.source.port)
                            && reply.meta.bytes == crate::qcom_tftp_server_check::OACK.len()
                            && reply.bytes[..reply.meta.bytes] == *crate::qcom_tftp_server_check::OACK);
                        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
                        { original = retained.map(|reply| (reply.request, 2)); }
                        retained.is_some()
                    } else { false }
                };
                #[cfg(all(feature = "sunfish-native-tftp-server-check", not(feature = "sunfish-tftp-per-peer-reply-experiment")))]
                let coalesce_pending = coalesce_pending_legacy;
                #[cfg(feature = "sunfish-native-tftp-server-check")]
                let coalesce = if coalesce_pending { true } else {
                    let (route, matches_request) = {
                        let (route, packet) = self.adapter.tftp_datagram(self.generation, &handle)?;
                        (route, packet == crate::qcom_tftp_server_check::REQUEST)
                    };
                    let mut duplicate = false;
                    if route.source.node == 1 && route.source.port == crate::qcom_tftp_server_check::SERVICE_PORT
                        && matches_request {
                        for held in self.tftp_requests.iter().flatten() {
                            let (held_route, held_packet) = self.adapter.tftp_datagram(self.generation, held)?;
                            if held_route == route && held_packet == crate::qcom_tftp_server_check::REQUEST {
                                #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
                                { original = Some((*held, 1)); }
                                duplicate = true;
                                break;
                            }
                        }
                    }
                    duplicate
                };
                #[cfg(not(feature = "sunfish-native-tftp-server-check"))]
                let coalesce = false;
                #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
                let coalesce = coalesce || self.adapter.coalesce_followed_wrq(&handle);
                if coalesce {
                    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                    self.adapter.observe_registration_tftp_dispatch(handle, 0xfe, u8::MAX);
                    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
                    self.adapter.observe_registration_tftp_duplicate(handle, original);
                    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
                    self.adapter.observe_elf_server_check_coalesced(&handle);
                    self.adapter.finish_tftp_request(self.generation, &handle)?;
                    #[cfg(feature = "sunfish-native-tftp-server-check")]
                    { self.tftp_wrq_coalesced = self.tftp_wrq_coalesced.saturating_add(1); }
                } else {
                let Some(slot) = self.tftp_requests.iter_mut().find(|slot| slot.is_none()) else {
                    #[cfg(feature = "sunfish-native-tftp-server-check")]
                    { self.queue_full_kind = 2; }
                    return Err(Error::QueueFull);
                };
                *slot = Some(handle);
                #[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
                { new_tftp_request = true; }
                }
            }
            if matches!(rx, Receive::Session(Event::QrtrControl(_))) {
                progress.controls += 1;
            }
            #[cfg(feature = "sunfish-wlan-elf-tftp-timeline")]
            {
                use crate::qcom_qrtr_control::{Event as ControlEvent, WLFW_SERVICE_ID, WLFW_VERSION_INSTANCE};
                if let Receive::Session(Event::QrtrControl(ControlEvent::Server { server, added: true, .. })) = rx
                    && self.first_wlfw_server_at.is_none()
                    && server.address.node == 0
                    && server.service == WLFW_SERVICE_ID
                    && server.instance == WLFW_VERSION_INSTANCE
                {
                    self.first_wlfw_server_at = self.rmtfs_event_clock.map(|clock| clock());
                }
            }
            #[cfg(feature = "sunfish-tftp-peer-close-observer")]
            if let Receive::Session(Event::QrtrControl(crate::qcom_qrtr_control::Event::ClientRemoved(port))) = rx {
                if let Some(clock) = self.rmtfs_event_clock { self.tftp_timing.peer_closed(port, clock()); }
            }
            // Yield at most once before the next publication attempt, even
            // under a continuous stream of distinct TFTP requests.
            #[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
            if new_tftp_request && !self.tftp_yield_debt {
                self.tftp_yield_debt = true;
                break;
            }
            #[cfg(feature = "sunfish-ipa-registration-rx-first")]
            let first_oack_queued = self.adapter.first_server_check().is_some_and(|trace|
                trace.before_elf && trace.exact_wrq && !trace.coalesced
                    && trace.oack.phase == 2 && trace.timing.closes == 0);
            #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
            let cold_ipa_yield = rx_progress && self.yield_for_cold_ipa_registration();
            #[cfg(feature = "sunfish-ipa-registration-cold-handoff")]
            if cold_ipa_yield {
                #[cfg(not(feature = "sunfish-first-oack-owner-turn"))]
                break;
                #[cfg(feature = "sunfish-first-oack-owner-turn")]
                if !first_oack_queued { break; }
            }
            #[cfg(feature = "sunfish-ipa-registration-rx-first")]
            if rx_progress && self.first_oack_rx_passes < 8 && first_oack_queued
                && self.adapter.ipa_registration_trace().is_none()
                && !matches!(rx, Receive::Session(Event::ReceiveIntentRequest { .. }
                    | Event::TftpRequest(_) | Event::ReplyRxDone(_) | Event::TransmitDone))
            {
                // Lend the normal arbiter one pass before scanning more RX.
                // It can release the first WRQ or handle a pending grant.
                #[cfg(feature = "sunfish-first-oack-owner-turn")]
                let skip_writer = core::mem::replace(&mut self.first_oack_initial_turn_used, true);
                #[cfg(not(feature = "sunfish-first-oack-owner-turn"))]
                let skip_writer = true;
                if skip_writer {
                    self.first_oack_rx_passes += 1;
                    continue;
                }
            }
            #[cfg(feature = "sunfish-tftp-oack-followup")]
            let repeat_was_done = self.adapter.oack_slot_follow()
                .is_some_and(|trace| trace.exact_repeat_done_at.is_some());
            #[cfg(feature = "sunfish-ipa-init-empty-rx")]
            let awaiting_registration = self.adapter.registration_close_fifo().is_none();
            #[cfg(feature = "sunfish-first-oack-grant-window")]
            let tx = {
                let (ordinary, extra) = self.adapter.publish_first_grant_turn(self.generation)?;
                progress.published += extra;
                ordinary
            };
            #[cfg(not(feature = "sunfish-first-oack-grant-window"))]
            let tx = self.adapter.publish_next(self.generation)?;
            #[cfg(feature = "sunfish-tftp-oack-credit-early")]
            if self.adapter.take_oack_credit_turn() {
                let credit = self.adapter.publish_oack_followup(
                    self.generation, crate::qcom_glink_qmi::OackFollowup::CreditReplay,
                )?;
                if matches!(credit, Transmit::Session(crate::qcom_glink_qmi::Published::Data { .. })) {
                    progress.published += 1;
                }
            }
            #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
            if self.adapter.take_oack_credit_retry() {
                let retry = self.adapter.publish_oack_followup(
                    self.generation, crate::qcom_glink_qmi::OackFollowup::Reply,
                )?;
                if matches!(retry, Transmit::Session(crate::qcom_glink_qmi::Published::Data { .. })) {
                    progress.published += 1;
                }
            }
            #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
            if self.adapter.take_oack_after_done() {
                use crate::qcom_glink_qmi::{OackFollowup, Published};
                #[cfg(feature = "sunfish-tftp-after-done-grant-turn")]
                let ready = if self.adapter.oack_followup_grant_pending() {
                    let ad = self.adapter.publish_oack_followup(self.generation, OackFollowup::Advertise)?;
                    if matches!(ad, Transmit::Session(Published::ReceiveIntent(_))) {
                        progress.published += 1;
                        let ack = self.adapter.publish_oack_followup(self.generation, OackFollowup::Grant)?;
                        let committed = matches!(ack, Transmit::Session(Published::IntentRequestAck { granted: true }));
                        progress.published += u32::from(committed);
                        committed
                    } else {
                        false
                    }
                } else {
                    true
                };
                #[cfg(not(feature = "sunfish-tftp-after-done-grant-turn"))]
                let ready = true;
                if ready {
                    let retry = self.adapter.publish_oack_followup(self.generation, OackFollowup::Reply)?;
                    if matches!(retry, Transmit::Session(Published::Data { .. })) {
                        progress.published += 1;
                    }
                }
            }
            #[cfg(feature = "sunfish-tftp-oack-followup")]
            if !repeat_was_done && matches!(tx, Transmit::Session(crate::qcom_glink_qmi::Published::ReceiveDone(_)))
                && self.adapter.oack_slot_follow().is_some_and(|trace| trace.exact_repeat_done_at.is_some())
            {
                use crate::qcom_glink_qmi::{OackFollowup, Published};
                let ad = self.adapter.publish_oack_followup(self.generation, OackFollowup::Advertise)?;
                if matches!(ad, Transmit::Session(Published::ReceiveIntent(_))) {
                    progress.published += 1;
                    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
                    let grant_done = if self.adapter.oack_followup_grant_pending() {
                        let ack = self.adapter.publish_oack_followup(self.generation, OackFollowup::Grant)?;
                        let committed = matches!(ack, Transmit::Session(Published::IntentRequestAck { .. }));
                        progress.published += u32::from(committed);
                        committed
                    } else {
                        true
                    };
                    #[cfg(not(feature = "sunfish-tftp-oack-grant-followup"))]
                    let grant_done = true;
                    if grant_done {
                        let reply = self.adapter.publish_oack_followup(self.generation, OackFollowup::Reply)?;
                        if matches!(reply, Transmit::Session(Published::Data { .. })) {
                            progress.published += 1;
                        }
                    }
                }
            }
            #[cfg(feature = "sunfish-wlan-elf-blocker-timing")]
            if let Some(blocker) = self.service_registry_trace.elf.as_mut().and_then(|elf| elf.blocker.as_mut())
                && blocker.published.is_none()
                && self.adapter.reply_fully_published(&blocker.handle)
            {
                blocker.published = self.rmtfs_event_clock.map(|clock| clock());
            }
            #[cfg(feature = "sunfish-wlan-registry-txn4-timing")]
            if let (Some(handle), Some(timing)) =
                (self.registry_txn4_handle, self.service_registry_trace.txn4.as_mut())
                && timing.published.is_none()
                && self.adapter.reply_fully_published(&handle)
            {
                timing.published = self.rmtfs_event_clock.map(|clock| clock());
            }
            #[cfg(feature = "sunfish-wlan-elf-timing")]
            if self.elf_lookup_handle.is_some_and(|handle| self.adapter.reply_fully_published(&handle)) {
                if let Some(elf) = self.service_registry_trace.elf.as_mut() {
                    if elf.published.is_none() {
                        elf.published = self.rmtfs_event_clock.map(|clock| clock());
                        #[cfg(feature = "sunfish-wlan-elf-phase-io")]
                        { elf.io.published = self.adapter.elf_io_totals(); }
                    }
                }
            }
            #[cfg(feature = "sunfish-tftp-rx-yield-experiment")]
            { self.tftp_yield_debt = false; }
            #[cfg(feature = "sunfish-tftp-timing-observer")]
            self.observe_tftp_timing();
            #[cfg(feature = "sunfish-linux-rmtfs")]
            if self.rmtfs_large_reply.is_some_and(|handle| self.adapter.reply_fully_published(&handle)) {
                self.rmtfs_large_reply_flags |= 2;
                self.mark_rmtfs_event(1);
            }
            let tx_progress = matches!(
                tx,
                Transmit::Session(_)
                    | Transmit::Blocked {
                        read_notification_sent: true
                    }
            );
            if tx_progress {
                progress.published += 1;
            }
            #[cfg(feature = "sunfish-ipa-init-empty-rx")]
            if awaiting_registration && self.adapter.ready_for_init_at_registration() {
                // End the checked writer iteration at the exact registration commit.
                // The router admits INIT after work returns. The existing continuation
                // resumes with normal RX processing and GLINK/QRTR arbitration.
                progress.init_at_registration = true;
                break;
            }
            // Give the armed handoff its service turn after the one
            // publication attempt, even when the FIFO was blocked.
            #[cfg(feature = "sunfish-first-oack-owner-turn")]
            if cold_ipa_yield { break; }
            // Keep the IRQ and schedule the normal continuation, as on budget
            // expiry. The service pass can now take the registration turn.
            #[cfg(feature = "sunfish-ipa-registration-early-turn")]
            if let Receive::Session(Event::Request(handle)) = rx
                && self.yield_for_ipa_registration(handle)
            {
                break;
            }
            // Let the regular service pass try ELF after the exact blocking
            // reply completes. Finish this checked iteration and keep the
            // continuation and any IRQ ownership, just as on budget expiry.
            #[cfg(feature = "sunfish-wlan-elf-completion-turn")]
            if let Receive::Session(Event::ReplyRxDone(handle)) = rx
                && self.elf_lookup_handle.is_some_and(|elf| self.requests.contains(&Some(elf)))
                && let Some(elf) = self.service_registry_trace.elf.as_mut()
                && elf.last_pressure.is_some_and(|sample|
                    sample.reason == 1 && sample.peer_request == Some(handle))
                && let Some(blocker) = elf.blocker.as_mut()
                && blocker.handle == handle
                && blocker.flags_at_wait & (1 << 3) != 0 // QRTR side already done
                && !blocker.yielded
            {
                blocker.yielded = true;
                blocker.yielded_at = self.rmtfs_event_clock.map(|clock| clock());
                break;
            }
            // The server-check dispatcher runs after this bounded transport
            // turn. Give it a turn on a new request or a TX completion that
            // may unblock a retained reply before polling another request.
            // Leaving `exhausted` true retains IRQ ownership and schedules
            // the existing continuation, so control/completion work resumes.
            #[cfg(feature = "sunfish-native-tftp-server-check")]
            if matches!(rx, Receive::Session(Event::TftpRequest(_)))
                || (self.has_pending_tftp_reply()
                    && matches!(rx, Receive::Session(Event::TransmitDone))) {
                break;
            }
            if !rx_progress && !tx_progress {
                exhausted = false;
                break;
            }
        }
        if !exhausted {
            // Status/lifecycle check precedes EACH rearm. A fault leaves the
            // corresponding IRQ masked; no GIC mapping or direct GIC ACK exists.
            for channel in [STATUS_CHANNEL, GLINK_CHANNEL] {
                let bit = 1 << channel;
                if self.pending_irqs & bit != 0 {
                    self.adapter.revalidate(self.generation)?;
                    interrupts.acknowledge(channel)?;
                    self.pending_irqs &= !bit;
                    progress.acknowledged |= bit;
                }
            }
        }
        // A post-ACK continuation closes the empty-check/rearm window without
        // relying on a second peer edge. Busy queues keep IRQs masked and yield.
        if (exhausted || progress.acknowledged != 0) && !self.scheduled {
            interrupts.schedule()?;
            self.scheduled = true;
            progress.deferred = true;
        }
        Ok(progress)
    }
}

/// One read-only WLAN-PD query per runtime owner. A failed attempt is not
/// retried, and this helper never registers a listener or requests PD restart.
#[cfg(feature = "sunfish-wlan-state-query")]
#[derive(Default)]
pub struct WlanStateQuery { attempted: bool }
#[cfg(feature = "sunfish-wlan-state-query")]
impl WlanStateQuery {
    pub fn step<I: Io>(&mut self, owner: &mut Owner<I>, ipa_ready: bool, interrupts: &mut impl Interrupts) -> Result<(), Error> {
        if self.attempted || !ipa_ready
            || owner.servreg_notif_state() != Some(crate::qcom_servreg_notif_client::State::Ready) { return Ok(()); }
        // Latch before queueing/scheduling so any failure cannot cause retry.
        self.attempted = true;
        owner.begin_servreg_notif_query(interrupts)
    }
    /// Snapshot already-owned software state only. Fields: phase, raw PD state,
    /// QMI result/error, exact NOTIF count/node/port, exact WLFW count/node/port.
    /// Phase 1 waits discovery, 2 waits IPA, 3 attempted/pending, 4 valid state,
    /// 5 QMI failure, 6 successful response without a state. MAX means unavailable.
    pub fn snapshot<I: Io>(&self, owner: &Owner<I>) -> [u32; 10] {
        let mut out = [u32::MAX; 10];
        out[0] = if self.attempted { 3 } else if owner.servreg_notif_state() == Some(crate::qcom_servreg_notif_client::State::Ready) { 2 } else { 1 };
        out[4] = 0; out[7] = 0;
        for i in 0..crate::qcom_qrtr_control::MAX_SERVERS {
            let Some(server) = owner.remote_server(i) else { continue; };
            if server.address.node != 0 { continue; }
            let base = if server.service == 0x42 && server.instance == crate::qcom_servreg_notif_client::VERSION_INSTANCE { 4 }
                else if server.service == crate::qcom_qrtr_control::WLFW_SERVICE_ID && server.instance == crate::qcom_qrtr_control::WLFW_VERSION_INSTANCE { 7 }
                else { continue; };
            out[base] += 1;
            if out[base] == 1 { out[base + 1] = server.address.node; out[base + 2] = server.address.port; }
        }
        if let Some(response) = owner.servreg_notif_outcome().and_then(|v| v.query) {
            out[2] = u32::from(response.result); out[3] = u32::from(response.error);
            out[0] = if response.result != 0 || response.error != 0 { 5 } else if response.state.is_none() { 6 } else { 4 };
            if response.result == 0 && response.error == 0 {
                use crate::qcom_servreg_notif::ServiceState;
                out[1] = match response.state {
                    Some(ServiceState::Down) => 0x0fff_ffff,
                    Some(ServiceState::Up) => 0x1fff_ffff,
                    Some(ServiceState::EarlyDown) => 0x2fff_ffff,
                    Some(ServiceState::Uninit) => 0x7fff_ffff,
                    Some(ServiceState::Unknown(raw)) => raw as u32,
                    None => u32::MAX,
                };
            }
        }
        out
    }
}

/// One listener registration after a fresh successful UNINIT query. State
/// indications use the existing validated transport ACK path. No PD restart.
#[cfg(feature = "sunfish-wlan-state-listener")]
#[derive(Default)]
pub struct WlanStateListener { attempted: bool }
#[cfg(feature = "sunfish-wlan-state-listener")]
impl WlanStateListener {
    pub fn step<I: Io>(&mut self, owner: &mut Owner<I>, ipa_ready: bool, interrupts: &mut impl Interrupts) -> Result<(), Error> {
        use crate::qcom_servreg_notif_client::State;
        use crate::qcom_servreg_notif::ServiceState;
        if self.attempted || !ipa_ready || owner.servreg_notif_state() != Some(State::Complete) { return Ok(()); }
        let ready = owner.servreg_notif_outcome().and_then(|v| v.query).is_some_and(|q|
            q.result == 0 && q.error == 0 && q.state == Some(ServiceState::Uninit));
        if !ready { return Ok(()); }
        // Consume the one registration attempt before queueing or scheduling.
        self.attempted = true;
        owner.begin_servreg_notif_listener(interrupts)
    }
    /// Phase, listener state/result/error, update transaction/state, ACK
    /// result/error. MAX denotes absent data, not DOWN. This reads no hardware.
    pub fn snapshot<I: Io>(&self, owner: &Owner<I>) -> [u32; 8] {
        use crate::qcom_servreg_notif::ServiceState;
        let raw = |state| match state {
            ServiceState::Down => 0x0fff_ffff,
            ServiceState::Up => 0x1fff_ffff,
            ServiceState::EarlyDown => 0x2fff_ffff,
            ServiceState::Uninit => 0x7fff_ffff,
            ServiceState::Unknown(v) => v as u32,
        };
        let mut out = [u32::MAX; 8];
        out[0] = if self.attempted { 3 } else { 1 };
        if let Some(v) = owner.servreg_notif_outcome() {
            if !self.attempted && v.query.is_some_and(|q|
                q.result != 0 || q.error != 0 || q.state != Some(ServiceState::Uninit)) { out[0] = 2; }
            if let Some(reply) = v.listener {
                out[2] = u32::from(reply.result); out[3] = u32::from(reply.error);
                out[0] = if reply.result != 0 || reply.error != 0 { 5 } else if reply.state.is_none() { 6 } else { 4 };
                if reply.result == 0 && reply.error == 0 { out[1] = reply.state.map_or(u32::MAX, raw); }
            }
            if let Some(update) = v.state_update { out[4] = u32::from(update.indication_transaction); out[5] = raw(update.state); }
            if let Some(ack) = v.set_ack { out[6] = u32::from(ack.result); out[7] = u32::from(ack.error); }
        }
        out
    }
}
