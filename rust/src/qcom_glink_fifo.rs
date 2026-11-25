//! Native SMEM FIFO adapter for the bounded, already-negotiated IPCRTR session.
//!
//! Reference: LineageOS sunfish 28f9290ae067d3e7857126eb6312720dbe9c7c80,
//! drivers/rpmsg/qcom_glink_smem.c and qcom_glink_native.c. Descriptor words:
//! peer TX tail, AP TX head, AP RX tail, peer RX head, four reserved words.
//! No allocation, FIFO initialization, channel negotiation, or IRQ registration.
#![forbid(unsafe_code)]

#[cfg(feature = "sunfish-linux-mpss-glink")]
#[path = "sunfish_mpss_linux_transport.rs"]
mod boot;
#[cfg(feature = "sunfish-linux-ipa-qmi")]
#[path = "sunfish_ipa_linux_qmi_fifo.rs"]
mod linux_ipa_fifo;
#[cfg(feature = "sunfish-linux-mpss-glink")]
pub use boot::{BootDrive, BootIo, BootPhase, BootReport, BootTransport};

#[cfg(feature = "sunfish-grant-buffer-reads")]
#[path = "qcom_glink_grant_reads.rs"]
mod grant_reads;
#[cfg(feature = "sunfish-grant-buffer-reads")]
pub use grant_reads::GrantBufferReads;
#[cfg(feature = "sunfish-oack-grant-outcome")]
pub use grant_reads::OackGrantTrace;

use crate::qcom_glink_qmi::{
    self as core, Event, Published, Reply, RequestHandle, Session, TftpHandle, WriteError,
};
use crate::qcom_modem_services::{Request, Service};
use crate::qcom_msa::Reservation as MsaReservation;
use crate::qcom_qmi_router::Context;

pub const FIFO_BYTES: usize = 0x4000;
pub const DESCRIPTOR_BYTES: usize = 32;
pub const FULL_RESERVE: usize = 8;
pub const TX_RESERVE: usize = 16;
pub const MAX_RX_RECORD: usize = 16 + core::MAX_DATAGRAM_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fault;

/// Values supplied by a trusted lifecycle/directory observer, never by a GLINK
/// packet. Identity pins the mapped SMEM route and exact allocation identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub generation: u64,
    pub identity: u64,
    pub ready: bool,
    pub fatal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Barrier {
    Acquire,
    Release,
    Complete,
}

/// IO must faithfully implement single-copy LE cursors, uncached/coherent FIFO
/// accesses, device-scope ordering, and the fixed peer doorbell. Calls are
/// serialized with all AP writers, IRQ handlers, mapping changes, and resets.
/// A failing operation may have taken effect; errors are never retried here.
pub trait Io {
    /// Diagnostic timestamps only; never reuses authority or performs I/O.
    #[cfg(feature = "sunfish-wrq-validation-path")]
    fn validation_cost_begin(&mut self, _clock: Option<fn()->u64>) {}
    #[cfg(feature = "sunfish-wrq-validation-path")]
    fn validation_cost_end(&mut self) -> Option<[[u64;4];3]> {None}
    #[cfg(feature="sunfish-wrq-domain-costs")]
    fn validation_cost_parts(&self)->Option<[[[u64;7];2];3]> {None}
    /// Copy existing CPU-side counters only, with no hardware observation.
    #[cfg(feature = "sunfish-wlan-elf-phase-io")]
    fn elf_io_totals(&self) -> Option<[u64; 12]> { None }
    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    fn registry_prearm_timing(&mut self,_enabled:bool) {}
    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    fn registry_prearm_totals(&self)->Option<[u64;12]>{
        #[cfg(feature="sunfish-wlan-elf-phase-io")] {self.elf_io_totals()}
        #[cfg(not(feature="sunfish-wlan-elf-phase-io"))] {None}
    }
    /// Only return counters when the native profiler is enabled for this call.
    #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
    fn reserved_io_totals(&self) -> Option<[u64; 12]> { None }
    /// Optional trusted backend diagnostic for a terminal access failure.
    /// It is observational only and cannot make a failed IO operation usable.
    fn diagnostic(&self) -> u32 {
        0
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    fn crash_evidence(&self) -> Option<crate::qcom_modem_crash::Evidence> { None }
    fn observe(&mut self) -> Result<Observation, Fault>;
    fn descriptor(&mut self, word: usize) -> Result<u32, Fault>;
    /// Override for one coherent native descriptor observation.
    fn descriptors(&mut self) -> Result<[u32; 8], Fault> {
        let mut out = [0; 8];
        for (n, v) in out.iter_mut().enumerate() {
            *v = self.descriptor(n)?;
        }
        Ok(out)
    }
    /// Selected Linux discovers the peer FIFO lazily, using its actual size.
    /// Legacy negotiated transports retain their established 16 KiB default.
    fn rx_length(&mut self) -> Result<Option<usize>, Fault> {
        Ok(Some(FIFO_BYTES))
    }
    /// One bounded read-only sample. Native implementations may put the local
    /// length lookup and descriptor read inside one pair of domain checks.
    /// This is not an atomic peer snapshot; Ring still validates cursor motion.
    #[cfg(feature = "sunfish-glink-combined-cursor-sample")]
    fn cursor_snapshot(&mut self) -> Result<(Option<usize>, [u32; 8]), Fault> {
        let length = self.rx_length()?;
        let words = self.descriptors()?;
        Ok((length, words))
    }
    fn read_rx(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), Fault>;
    fn read_tx(&mut self, offset: usize, bytes: &mut [u8]) -> Result<(), Fault>;
    fn write_tx(&mut self, offset: usize, bytes: &[u8]) -> Result<(), Fault>;
    fn store_tx_head(&mut self, value: u32) -> Result<(), Fault>;
    fn store_rx_tail(&mut self, value: u32) -> Result<(), Fault>;
    fn barrier(&mut self, kind: Barrier) -> Result<(), Fault>;
    /// Adjacent barrier/status operations, in their original order. The native
    /// implementation shares one domain pre/post pair, not cached authority.
    #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
    fn barrier_observe(&mut self, kind: Barrier) -> Result<Observation, Fault> {
        self.barrier(kind)?;
        self.observe()
    }
    fn notify_peer(&mut self) -> Result<(), Fault>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Session(core::Error),
    StaleGeneration,
    Quarantined,
    Access,
    NotReady,
    Fatal,
    IdentityChanged,
    CursorBounds,
    OwnedCursorChanged,
    PeerOverrun,
    ReservedChanged,
    UnsupportedCommand,
    InvalidRecord,
    ChangedRecord,
    Readback,
    WouldBlock,
    SessionNotFresh,
    ResetNotEmpty,
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    Negotiation,
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    Timeout,
}
impl From<Fault> for Error {
    fn from(_: Fault) -> Self {
        Self::Access
    }
}
impl From<core::Error> for Error {
    fn from(error: core::Error) -> Self {
        Self::Session(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Receive {
    Idle,
    Pending {
        available: usize,
        needed: usize,
    },
    Session(Event),
    ReadNotification,
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    Control,
}

/// Read-only copy of the late OPEN that failed, or its invalid length header.
#[cfg(feature = "sunfish-linux-mpss-glink")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenDiagnostic {
    pub stage: u32,
    pub channel: u32,
    pub advertised_length: u32,
    /// Bitmask of rejected length, id, name, duplicate and capacity checks.
    pub reason: u32,
    pub other_channels: u32,
    pub remote_channel: u32,
    pub name: [u8; 24],
    pub name_len: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Transmit {
    Idle,
    Session(Published),
    AwaitingIntent,
    Blocked { read_notification_sent: bool },
}

fn distance(from: u32, to: u32) -> usize {
    (to as usize + FIFO_BYTES - from as usize) % FIFO_BYTES
}
fn advance(cursor: u32, size: usize) -> u32 {
    ((cursor as usize + size) % FIFO_BYTES) as u32
}
fn ring_distance(from: u32, to: u32, length: usize) -> usize {
    (to as usize + length - from as usize) % length
}
fn ring_advance(cursor: u32, bytes: usize, length: usize) -> u32 {
    ((cursor as usize + bytes) % length) as u32
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn ready(observation: Observation, generation: u64) -> Result<(), Error> {
    if observation.generation != generation {
        return Err(Error::StaleGeneration);
    }
    if observation.fatal {
        return Err(Error::Fatal);
    }
    if !observation.ready {
        return Err(Error::NotReady);
    }
    Ok(())
}
fn descriptors(io: &mut impl Io, rx: Option<usize>) -> Result<[u32; 8], Error> {
    validate_descriptors(io.descriptors()?, rx)
}

fn validate_descriptors(words: [u32; 8], rx: Option<usize>) -> Result<[u32; 8], Error> {
    if words[..4].iter().any(|v| v % 8 != 0)
        || words[..2].iter().any(|v| *v as usize >= FIFO_BYTES)
        || rx.is_some_and(|length| {
            length < 8
                || length > u32::MAX as usize
                || length % 8 != 0
                || words[2..4].iter().any(|v| *v as usize >= length)
        })
        || (rx.is_none() && words[2..4] != [0, 0])
    {
        return Err(Error::CursorBounds);
    }
    Ok(words)
}

#[cfg(feature = "sunfish-wlan-first-server-check-tail")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ServerCheckTail {
    #[default]
    Untracked,
    Pending(usize),
    Consumed(Option<u64>),
}

/// Bounds for visibility of the first OACK's exact peer RX_DONE.
#[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerCheckRxDone {
    pub last_absent: Option<u64>,
    pub first_visible: Option<u64>,
    pub visible_at_close: Option<bool>,
    pub bytes_ahead: Option<u32>,
}

#[cfg(feature = "sunfish-ipa-ingress-work")]
#[derive(Clone, Copy)]
/// A checked receive or writer operation. Kinds encode RX (0), committed TX
/// (1), yielded TX (2), full TX (3), server add/remove (4/5), TX error (6),
/// writer with no FIFO write (7), empty/incomplete RX (8/9), and QRTR close (A).
pub struct IpaIngressRecord {
    pub start: u64,
    pub end: u64,
    position: Option<u64>,
    pub detail: u32,
    pub size: u16,
    pub cmd: u8,
    pub kind: u8,
    #[cfg(feature = "sunfish-wrq-stage-costs")]
    pub protocol: Option<[u32; 4]>,
}

#[cfg(feature = "sunfish-ipa-ingress-work")]
#[derive(Default)]
pub struct IpaIngressWork {
    entries: [Option<IpaIngressRecord>; 24],
    next: usize,
    evicted_end: Option<u64>,
    target: Option<u64>,
}

#[cfg(feature = "sunfish-ipa-ingress-work")]
impl IpaIngressWork {
    fn push(&mut self, record: IpaIngressRecord) {
        if let Some(old) = self.entries[self.next].replace(record) {
            self.evicted_end = Some(old.end);
        }
        self.next = (self.next + 1) % self.entries.len();
    }

    pub fn records(&self, visible: u64, accepted: u64) -> impl Iterator<Item = &IpaIngressRecord> {
        self.entries[self.next..].iter().chain(&self.entries[..self.next]).flatten()
            .filter(move |e| e.end >= visible && e.start <= accepted)
    }

    pub fn lost(&self, visible: u64) -> bool {
        self.evicted_end.is_some_and(|end| end >= visible)
    }

    pub fn distance(&self, record: &IpaIngressRecord) -> Option<u64> {
        self.target?.checked_sub(record.position?)
    }
}

#[cfg(feature = "sunfish-spare-wrq-ingress")]
pub struct WrqReceiveWork {
    #[cfg(feature = "sunfish-wrq-validation-path")]
    pub validation: WrqValidationWork,
    pub records: IpaIngressWork,
    rx_start: Option<u64>,
    samples: [(u64, u64); 96],
    sample_index: usize,
    evicted_sample_end: Option<u64>,
    #[cfg(feature = "sunfish-wrq-stage-costs")]
    pub finished: Option<u64>,
}

#[cfg(feature = "sunfish-spare-wrq-ingress")]
impl Default for WrqReceiveWork {
    fn default() -> Self {
        Self {
            #[cfg(feature = "sunfish-wrq-validation-path")] validation:Default::default(),
            records: Default::default(), rx_start: None, samples: [(0, 0); 96],
            sample_index: 0, evicted_sample_end: None,
            #[cfg(feature = "sunfish-wrq-stage-costs")] finished: None }
    }
}

#[cfg(feature = "sunfish-spare-wrq-ingress")]
impl WrqReceiveWork {
    fn sample(&mut self, start: u64, end: u64) {
        #[cfg(feature = "sunfish-wrq-stage-costs")]
        if self.finished.is_some() { return; }
        let slot = &mut self.samples[self.sample_index];
        if slot.1 > slot.0 { self.evicted_sample_end = Some(slot.1); }
        *slot = (start, end);
        self.sample_index = (self.sample_index + 1) % self.samples.len();
    }

    /// Count and sum only the part of each validated sample inside the window.
    pub fn checks(&self, from: u64, to: u64) -> (u32, u64, bool) {
        let mut count = 0;
        let mut ticks = 0;
        for &(a, b) in &self.samples {
            let overlap = b.min(to).saturating_sub(a.max(from));
            if overlap > 0 {
                count += 1;
                ticks += overlap;
            }
        }
        (count, ticks, self.evicted_sample_end.is_some_and(|end| end > from))
    }

    fn record(&mut self, start: u64, end: u64, kind: u8, bytes: &[u8]) {
        #[cfg(feature = "sunfish-wrq-stage-costs")]
        let protocol = if bytes.len() >= 8 {
            let header = RegistrationHeader::read(bytes, kind == 1, Some(start));
            header.qrtr.map(|[packet, source, destination]| {
                let tag = if matches!(packet, 4 | 5) && bytes.len() >= 56 && word(bytes, 8) >= 40 {
                    word(bytes, 52)
                } else { header.qmi.map_or(u32::MAX, |(_, _, message)| u32::from(message)) };
                [packet, source, destination, tag]
            })
        } else { None };
        let detail = if bytes.len() >= 8 { word(bytes, 4) } else { 0 };
        let record=IpaIngressRecord { start, end, kind, cmd: bytes.first().copied().unwrap_or(0xff),
            size: bytes.len() as u16, detail, position: None,
            #[cfg(feature = "sunfish-wrq-stage-costs")] protocol,
        };
        #[cfg(feature = "sunfish-wrq-validation-path")]
        self.validation.record(record);
        #[cfg(feature = "sunfish-wrq-stage-costs")]
        if self.finished.is_some() { return; }
        self.records.push(record);
    }
}

#[cfg(feature = "sunfish-wrq-prefix-close")]
#[derive(Clone, Copy, Default)]
pub struct WrqPrefixHeader {
    pub command: u16,
    pub bytes: u16,
    pub parameter: u32,
    pub extra: u32,
    pub qrtr: Option<[u32; 3]>,
}

#[cfg(feature = "sunfish-wrq-prefix-close")]
impl WrqPrefixHeader {
    fn read(record: &[u8]) -> Self {
        let header = RegistrationHeader::read(record, false, None);
        Self {
            command: header.command, bytes: header.bytes, parameter: header.parameter,
            qrtr: header.qrtr,
            extra: if header.command == 5 && record.len() >= 16 {
                word(record, 8)
            } else if let Some([_, port]) = header.del_client {
                port
            } else if let Some([kind @ (4 | 5), _, _]) = header.qrtr
                && word(record, 8) == 52 && word(record, 12) == 0
                && word(record, 36) == 20 && word(record, 48) == kind
            {
                word(record, 52) // NEW_SERVER or DEL_SERVER service ID
            } else {
                header.qmi.map_or(u32::MAX, |(_, txn, id)| u32::from(txn) << 16 | u32::from(id))
            },
        }
    }
}

/// Metadata from an already validated record overlapping the captured window.
#[cfg(feature = "sunfish-wrq-return-headers")]
#[derive(Clone, Copy)]
pub struct WrqWindowHeader {
    pub offset: u32,
    pub header: WrqPrefixHeader,
    pub channel: u16,
    pub qmi_kind: u8,
    pub name: u32,
    pub checked: Option<u64>,
    pub clear: Option<u64>,
}

/// The byte interval already visible at the original WRQ return. Follow only
/// the AP-owned tail. A later peer append cannot extend this interval.
#[cfg(feature = "sunfish-wrq-return-pending")]
pub struct WrqReturnPending {
    pub bytes: u32,
    pub early: u32,
    pub first: Option<WrqPrefixHeader>,
    pub first_at: Option<u64>,
    pub first_clear: Option<u64>,
    pub backlog_clear: Option<u64>,
    tail: u32,
    remaining: u32,
    #[cfg(feature = "sunfish-wrq-return-headers")]
    headers: [Option<WrqWindowHeader>; 13],
    #[cfg(feature = "sunfish-wrq-return-headers")]
    pub header_count: u32,
    #[cfg(feature = "sunfish-wrq-return-headers")]
    pub header_omitted: u32,
}

#[cfg(feature = "sunfish-wrq-return-pending")]
impl WrqReturnPending {
    fn new(tail: u32, bytes: u32, early: u32) -> Self {
        Self { tail, bytes, early, remaining: bytes, first: None,
            first_at: None, first_clear: None, backlog_clear: None,
            #[cfg(feature = "sunfish-wrq-return-headers")]
            headers: [None; 13],
            #[cfg(feature = "sunfish-wrq-return-headers")]
            header_count: 0,
            #[cfg(feature = "sunfish-wrq-return-headers")]
            header_omitted: 0,
        }
    }

    #[cfg(feature = "sunfish-wrq-return-headers")]
    pub fn headers(&self) -> impl Iterator<Item = &WrqWindowHeader> {
        self.headers.iter().flatten()
    }

    #[cfg(feature = "sunfish-wrq-return-headers")]
    fn record_header(&mut self, record: &[u8], at: Option<u64>) {
        if self.remaining == 0 { return; }
        let offset = self.bytes - self.remaining;
        if self.headers.iter().flatten().any(|h| h.offset == offset) { return; }
        let header = RegistrationHeader::read(record, false, at);
        let name = if header.command == 2 && record.len() >= 12 {
            u32::from_be_bytes(record[8..12].try_into().unwrap())
        } else { u32::MAX };
        let entry = WrqWindowHeader {
            offset, header: WrqPrefixHeader::read(record), channel: header.channel,
            qmi_kind: header.qmi.map_or(u8::MAX, |(kind, _, _)| kind),
            name, checked: at, clear: None,
        };
        if let Some(slot) = self.headers.get_mut(self.header_count as usize) {
            *slot = Some(entry);
        } else {
            self.header_omitted = self.header_omitted.saturating_add(1);
        }
        self.header_count = self.header_count.saturating_add(1);
    }

    fn follows(&self, tail: u32) -> bool {
        self.remaining != 0 && self.tail == tail
    }

    fn advanced(&mut self, next: u32, bytes: u32, at: Option<u64>) {
        #[cfg(feature = "sunfish-wrq-return-headers")]
        {
            let offset = self.bytes - self.remaining;
            if let Some(entry) = self.headers.iter_mut().flatten().find(|h| h.offset == offset) {
                entry.clear = at;
            }
        }
        if self.remaining == self.bytes {
            self.first_clear = at;
        }
        self.tail = next;
        self.remaining = self.remaining.saturating_sub(bytes);
        if self.remaining == 0 {
            self.backlog_clear = at;
        }
    }
}

#[cfg(feature = "sunfish-wrq-prefix-close")]
#[derive(Default)]
pub struct WrqPrefix {
    active: bool,
    pub complete: bool,
    pub records: u32,
    pub bytes: u32,
    entries: [WrqPrefixHeader; 12],
    watch_head: bool,
    last_data_head: Option<u64>,
    pub init_head: Option<u64>,
    pub oack_head: Option<u64>,
}

#[cfg(feature = "sunfish-wrq-prefix-close")]
impl WrqPrefix {
    pub fn headers(&self) -> &[WrqPrefixHeader] {
        &self.entries[..(self.records as usize).min(self.entries.len())]
    }

    fn consumed(&mut self, record: &[u8]) {
        if let Some(entry) = self.entries.get_mut(self.records as usize) {
            *entry = WrqPrefixHeader::read(record);
        }
        self.records = self.records.saturating_add(1);
        self.bytes = self.bytes.saturating_add(record.len() as u32);
    }
}

#[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
const RX_HEAD_HISTORY: usize = 32;

#[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
#[derive(Clone, Copy)]
struct RxHeadAdvance {
    previous: u64,
    head: u64,
    tail: u64,
    last_absent: Option<u64>,
    first_visible: Option<u64>,
}

#[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
#[derive(Clone, Copy)]
pub struct OackAheadHeader {
    pub position: u64,
    pub at: Option<u64>,
    pub command: u16,
    pub channel: u16,
    pub parameter: u32,
    pub bytes: u16,
    pub qrtr: Option<[u32; 3]>,
    pub qmi: Option<(u8, u16, u16)>,
    pub del_client: Option<[u32; 2]>,
    pub tftp: Option<(u16, u16)>,
}

#[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
pub struct OackAheadCapture {
    headers: [Option<OackAheadHeader>; 16],
    pub armed_at: Option<u64>,
    pub initial_unread: u32,
    pub completion_id: Option<u32>,
    next: usize,
    base: Option<u64>,
    end: Option<u64>,
}

#[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
impl OackAheadCapture {
    fn new(armed_at: Option<u64>, initial_unread: usize) -> Self {
        Self { headers: [None; 16], armed_at,
            initial_unread: u32::try_from(initial_unread).unwrap_or(u32::MAX),
            completion_id: None, next: 0, base: None, end: None }
    }

    fn record(&mut self, position: u64, header: RegistrationHeader, raw: &[u8]) {
        if self.end.is_some() { return; }
        let tftp = header.qrtr.and_then(|[kind, source, destination]| {
            let transfer = crate::qcom_tftp_server_check::is_transfer_port(source)
                || crate::qcom_tftp_server_check::is_transfer_port(destination);
            if kind != 1 || !transfer || header.command != 9
                || raw.len() < 52 || word(raw, 8) < 36
                || word(raw, 12) != 0 || word(raw, 36) < 4
            {
                return None;
            }
            let opcode = u16::from_be_bytes([raw[48], raw[49]]);
            matches!(opcode, 3 | 4 | 5).then(||
                (opcode, u16::from_be_bytes([raw[50], raw[51]])))
        });
        self.headers[self.next] = Some(OackAheadHeader {
            position, at: header.at, command: header.command, channel: header.channel,
            parameter: header.parameter, bytes: header.bytes, qrtr: header.qrtr,
            qmi: header.qmi, del_client: header.del_client, tftp,
        });
        self.next = (self.next + 1) % self.headers.len();
    }

    fn finish(&mut self, window: &ServerCheckRxWindow, unread: usize, size: usize) {
        let Some(start) = window.head.checked_sub(unread as u64) else { return };
        let record_end = start.saturating_add(size as u64);
        let span = window.advances.iter().flatten()
            .find(|span| span.previous < record_end && record_end <= span.head);
        self.base = span.map(|span| span.tail);
        self.end = Some(start);
    }

    pub fn records(&self) -> impl Iterator<Item = OackAheadHeader> + '_ {
        let base = self.base;
        let end = self.end;
        self.headers[self.next..].iter().chain(self.headers[..self.next].iter())
            .flatten().copied().filter(move |header| {
                base.zip(end).is_some_and(|(base, end)|
                    header.position >= base && header.position < end)
            })
    }
}

/// Callback-scoped deltas; detail/domain totals are nested within broad totals.
#[cfg(feature = "sunfish-tftp-reserved-writer-io")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReservedWriterIoTrace {
    pub totals: [u64; 12], pub profiled: u32, pub unavailable: u32, pub invalid: u32,
}
#[cfg(feature = "sunfish-tftp-reserved-writer-io")]
impl ReservedWriterIoTrace {
    fn record(&mut self, before: Option<[u64; 12]>, after: Option<[u64; 12]>, enabled: bool) {
        let Some((before, after)) = before.zip(after).filter(|_| enabled) else {
            self.unavailable = self.unavailable.saturating_add(1); return;
        };
        if (0..12).any(|i| before[i] == u64::MAX || after[i] == u64::MAX || after[i] < before[i]) {
            self.invalid = self.invalid.saturating_add(1); return;
        }
        for i in 0..12 { self.totals[i] = self.totals[i].saturating_add(after[i] - before[i]); }
        self.profiled = self.profiled.saturating_add(1);
    }
}

#[cfg(all(test, feature = "sunfish-tftp-reserved-writer-io"))]
#[test]
fn reserved_writer_io_counts_only_enabled_callback_deltas() {
    let mut trace = ReservedWriterIoTrace::default();
    trace.record(Some([10; 12]), Some([13; 12]), true);
    assert_eq!(trace.profiled, 1);
    assert_eq!(trace.totals, [3; 12]);
    trace.record(Some([13; 12]), Some([20; 12]), false);
    trace.record(None, None, true);
    assert_eq!(trace.profiled, 1);
    assert_eq!(trace.unavailable, 2);
    assert_eq!(trace.totals, [3; 12]);
    trace.record(Some([20; 12]), Some([19; 12]), true);
    assert_eq!(trace.invalid, 1);
    assert_eq!(trace.totals, [3; 12]);
}

#[cfg(all(test, feature = "sunfish-tftp-oack-ahead-headers"))]
#[test]
fn reserved_oack_headers_retain_only_records_ahead_of_completion() {
    let mut window = ServerCheckRxWindow::new(0, Some(100));
    let mut capture = OackAheadCapture::new(None, 0);
    window.sample(120, 120, Some(110), Some(120));
    for (position, len, command) in [(0, 72, 9), (72, 32, 5), (104, 8, 6), (112, 8, 14)] {
        let mut raw = vec![0u8; len];
        raw[0] = command;
        let header = RegistrationHeader::read(&raw, false, Some(130 + position));
        capture.record(position, header, &raw);
    }
    capture.finish(&window, 8, 8);
    window.finish(8, 8);
    let records: Vec<_> = capture.records().collect();
    assert_eq!(window.done.unwrap().bytes_ahead, Some(112));
    assert_eq!(records.iter().map(|record| u32::from(record.bytes)).sum::<u32>(), 112);
    assert_eq!(records.iter().map(|record| record.command).collect::<Vec<_>>(), [9, 5, 6]);
    assert!(records.iter().all(|record| record.position < 112));
}

#[cfg(all(test, feature = "sunfish-tftp-oack-ahead-headers"))]
#[test]
fn reserved_oack_window_preserves_arm_and_existing_unread_boundary() {
    let mut window = ServerCheckRxWindow::new(16, Some(200));
    let mut capture = OackAheadCapture::new(Some(200), 16);
    let mut raw = [0u8; 8];
    raw[0] = 14;
    capture.record(0, RegistrationHeader::read(&raw, false, Some(210)), &raw);
    capture.record(8, RegistrationHeader::read(&raw, false, Some(220)), &raw);
    capture.finish(&window, 8, 8);
    window.finish(8, 8);
    assert_eq!(capture.armed_at, Some(200));
    assert_eq!(capture.initial_unread, 16);
    assert_eq!(window.done.unwrap().first_visible, Some(200));
    assert_eq!(window.done.unwrap().bytes_ahead, Some(8));
    assert_eq!(capture.records().count(), 1);
}

#[cfg(all(test, feature = "sunfish-tftp-oack-ahead-headers"))]
#[test]
fn ahead_header_extracts_only_valid_complete_tftp_data_metadata() {
    let mut raw = [0u8; 56];
    for (offset, value) in [
        (0, 0x0002_0009u32), (4, 0x4d), (8, 36), (12, 0),
        (16, 1), (20, 1), (28, 0x49), (36, 4),
        (40, 1), (44, crate::qcom_tftp_server_check::TRANSFER_PORT),
    ] {
        raw[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    raw[48..52].copy_from_slice(&[0, 3, 0, 7]);
    let mut capture = OackAheadCapture::new(None, 0);
    capture.record(0, RegistrationHeader::read(&raw, false, None), &raw);
    assert_eq!(capture.headers[0].unwrap().tftp, Some((3, 7)));

    raw[8..12].copy_from_slice(&33u32.to_le_bytes());
    capture.record(56, RegistrationHeader::read(&raw, false, None), &raw);
    assert_eq!(capture.headers[1].unwrap().tftp, None);

    raw[8..12].copy_from_slice(&36u32.to_le_bytes());
    raw[49] = 2;
    capture.record(112, RegistrationHeader::read(&raw, false, None), &raw);
    assert_eq!(capture.headers[2].unwrap().tftp, None);
}

#[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
struct ServerCheckRxWindow {
    #[cfg(feature = "sunfish-tftp-first-oack-headers")]
    headers: FirstOackHeaders,
    head: u64,
    last_sample_start: Option<u64>,
    close_head: Option<u64>,
    advances: [Option<RxHeadAdvance>; RX_HEAD_HISTORY],
    next_advance: usize,
    done: Option<ServerCheckRxDone>,
}

#[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
impl ServerCheckRxWindow {
    fn new(unread: usize, observed: Option<u64>) -> Self {
        let mut trace = Self {
            head: 0, last_sample_start: None, close_head: None,
            advances: [None; RX_HEAD_HISTORY], next_advance: 0, done: None,
            #[cfg(feature = "sunfish-tftp-first-oack-headers")]
            headers: FirstOackHeaders::default(),
        };
        trace.sample(unread, unread, None, observed);
        trace
    }

    fn sample(&mut self, advanced: usize, unread: usize, start: Option<u64>, end: Option<u64>) {
        if advanced != 0 {
            let previous = self.head;
            self.head = self.head.saturating_add(advanced as u64);
            self.advances[self.next_advance] = Some(RxHeadAdvance {
                previous, head: self.head, tail: self.head.saturating_sub(unread as u64),
                last_absent: self.last_sample_start, first_visible: end,
            });
            self.next_advance = (self.next_advance + 1) % RX_HEAD_HISTORY;
        }
        self.last_sample_start = start;
    }

    fn finish(&mut self, unread: usize, size: usize) {
        self.done = self.bounds(unread, size);
    }

    fn bounds(&self, unread: usize, size: usize) -> Option<ServerCheckRxDone> {
        let record_start = self.head.checked_sub(unread as u64)?;
        let record_end = record_start.saturating_add(size as u64);
        let span = self.advances.iter().flatten()
            .find(|span| span.previous < record_end && record_end <= span.head);
        Some(ServerCheckRxDone {
            last_absent: span.and_then(|span| span.last_absent),
            first_visible: span.and_then(|span| span.first_visible),
            visible_at_close: self.close_head.map(|head| head >= record_end),
            bytes_ahead: span.and_then(|span| record_start.checked_sub(span.tail))
                .and_then(|bytes| u32::try_from(bytes).ok()),
        })
    }
}

#[cfg(all(test, feature = "sunfish-wlan-first-oack-rx-fifo"))]
#[test]
fn rx_done_visible_in_same_sample_as_close_keeps_first_bound() {
    let mut trace = ServerCheckRxWindow::new(0, Some(100));
    trace.sample(0, 0, Some(110), Some(120));
    trace.sample(80, 80, Some(130), Some(140));
    trace.close_head = Some(trace.head);
    // The host consumes the 72-byte close before processing the RX_DONE.
    trace.sample(0, 8, Some(150), Some(160));
    trace.finish(8, 8);
    assert_eq!(trace.done, Some(ServerCheckRxDone {
        last_absent: Some(110), first_visible: Some(140),
        visible_at_close: Some(true), bytes_ahead: Some(72),
    }));
}

#[cfg(all(test, feature = "sunfish-tftp-oack-fifo-outcome"))]
#[test]
fn reserved_oack_completion_visible_after_close_has_distinct_bounds() {
    let mut window = ServerCheckRxWindow::new(0, Some(100));
    window.sample(0, 0, Some(110), Some(120));
    window.close_head = Some(window.head);
    window.sample(8, 8, Some(130), Some(140));
    window.finish(8, 8);
    assert_eq!(window.done, Some(ServerCheckRxDone {
        last_absent: Some(110),
        first_visible: Some(140),
        visible_at_close: Some(false),
        bytes_ahead: Some(0),
    }));
}

#[cfg(feature = "sunfish-ipa-registration-reply-headers")]
#[derive(Clone, Copy)]
pub struct RegistrationHeader {
    pub at: Option<u64>,
    pub tx: bool,
    pub command: u16,
    pub channel: u16,
    pub parameter: u32,
    pub bytes: u16,
    pub qrtr: Option<[u32; 3]>,
    pub qmi: Option<(u8, u16, u16)>,
    pub del_client: Option<[u32; 2]>,
    #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
    pub tftp_request: Option<RegistrationTftpRequest>,
}

#[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
#[derive(Clone, Copy)]
pub struct RegistrationTftpRequest {
    peer: [u32; 2],
    pub opcode: u8,
    pub kind: u8,
    pub bytes: u16,
    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
    handle: Option<core::TftpHandle>,
    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
    pub server_result: u8,
    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
    pub occupied: u8,
    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
    pub oack_queued: bool,
    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
    oack_progress: u8,
    #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
    queue_block: core::TftpQueueBlock,
    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
    full_queue: Option<TftpFullQueue>,
    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
    pub original: Option<OriginalTftpRequest>,
}

#[cfg(feature = "sunfish-ipa-tftp-original-trace")]
#[derive(Clone, Copy)]
pub struct OriginalTftpRequest {
    handle: core::TftpHandle,
    pub id: u32,
    pub source: u8,
    pub result: u8,
    pub occupied: u8,
    pub phases: u8,
    #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
    pub queue_block: core::TftpQueueBlock,
    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
    pub full_queue: Option<TftpFullQueue>,
}

#[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
#[derive(Clone, Copy)]
pub struct TftpFullQueue {
    pub at: Option<u64>,
    pub slots: [core::TftpTxSlot; core::TX_SLOTS],
    pub first_free_mask: u8,
    pub first_free_at: Option<u64>,
}

#[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
impl TftpFullQueue {
    fn record_release(&mut self, freed: u8, at: Option<u64>) {
        if freed != 0 && self.first_free_mask == 0 {
            self.first_free_mask = freed;
            self.first_free_at = at;
        }
    }
}

#[cfg(all(test, feature = "sunfish-ipa-tftp-slot-snapshot"))]
#[test]
fn tftp_full_queue_latches_the_first_release() {
    let slot = core::TftpTxSlot { kind: 6, opcode: 6, flags: 3, peer_port: 0x49, remote_intent: 0 };
    let mut snapshot = TftpFullQueue {
        at: Some(10), slots: [slot; core::TX_SLOTS], first_free_mask: 0, first_free_at: None,
    };
    snapshot.record_release(0, Some(11));
    snapshot.record_release(0x20, Some(12));
    snapshot.record_release(0x04, Some(13));
    assert_eq!((snapshot.first_free_mask, snapshot.first_free_at), (0x20, Some(12)));
    assert_eq!(snapshot.slots[5], slot);
}


// Called for a checked GLINK DATA chunk containing a QRTR v1 header.
#[cfg(feature = "sunfish-ipa-registration-reply-headers")]
fn del_client_endpoint(record: &[u8], packet: &[u8]) -> Option<[u32; 2]> {
    if packet.len() == 52 && word(packet, 4) == 6 && word(record, 12) == 0
        && word(packet, 20) == 20 && word(packet, 32) == 6
    {
        Some([word(packet, 36), word(packet, 40)])
    } else {
        None
    }
}

#[cfg(feature = "sunfish-ipa-registration-reply-headers")]
impl RegistrationHeader {
    fn read(record: &[u8], tx: bool, at: Option<u64>) -> Self {
        let command = u16::from_le_bytes([record[0], record[1]]);
        let mut header = Self {
            at, tx, command,
            channel: u16::from_le_bytes([record[2], record[3]]),
            parameter: word(record, 4),
            bytes: record.len() as u16,
            qrtr: None,
            qmi: None,
            del_client: None,
            #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
            tftp_request: None,
        };
        if command == 9 && record.len() >= 48 {
            let chunk = word(record, 8) as usize;
            if chunk >= 32 && let Some(packet) = record.get(16..16 + chunk)
                && word(packet, 0) == 1
            {
                let kind = word(packet, 4);
                header.qrtr = Some([kind, word(packet, 12), word(packet, 28)]);
                header.del_client = del_client_endpoint(record, packet);
                #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
                if !tx && kind == 1 && word(record, 12) == 0
                    && word(packet, 28) == crate::qcom_tftp_server_check::SERVICE_PORT
                    && let Some(body) = packet[32..].get(..word(packet, 20) as usize)
                    && let [0, opcode @ (1 | 2), rest @ ..] = body
                    && let Some(end) = rest.iter().position(|byte| *byte == 0)
                    && end != 0
                {
                    let path = &rest[..end];
                    let kind = match path {
                        b"/readwrite/server_check.txt" => 1,
                        b"/readwrite/mcfg.tmp" => 2,
                        b"/readwrite/ota_firewall/ruleset" => 3,
                        b"/readwrite/modem_efs" => 4,
                        value if value.ends_with(b"/wlanmdsp.mbn") => 5,
                        _ => 6,
                    };
                    header.tftp_request = Some(RegistrationTftpRequest {
                        peer: [word(packet, 8), word(packet, 12)],
                        opcode: *opcode, kind, bytes: body.len() as u16,
                        #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                        handle: None,
                        #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                        server_result: u8::MAX,
                        #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                        occupied: u8::MAX,
                        #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                        oack_queued: false,
                        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
                        oack_progress: 0,
                        #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
                        queue_block: core::TftpQueueBlock::default(),
                        #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
                        full_queue: None,
                        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
                        original: None,
                    });
                }
                if kind == 1 && word(record, 12) == 0
                    && let Some(body) = packet[32..].get(..word(packet, 20) as usize)
                    && let Ok(message) = crate::qcom_qmi::Message::decode(body)
                {
                    header.qmi = Some((body[0], message.transaction, message.message_id));
                }
            }
        }
        header
    }
}

#[cfg(all(test, feature = "sunfish-ipa-grant-tftp-peer"))]
#[test]
fn registration_close_uses_only_the_matching_tftp_peer_request() {
    let mut record = [0; 128];
    let body = b"\0\x01/readonly/firmware/image/wlanmdsp.mbn\0octet\0";
    for (offset, value) in [
        (0, 0x0002_0009u32), (4, 0x4c), (8, (32 + body.len()) as u32),
        (16, 1), (20, 1), (24, 0), (28, 0x49), (36, body.len() as u32),
        (40, 1), (44, crate::qcom_tftp_server_check::SERVICE_PORT),
    ] {
        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    record[48..48 + body.len()].copy_from_slice(body);
    let mut request = RegistrationHeader::read(&record, false, None);
    let mut trace = RegistrationReplyFifo::default();
    trace.correlate_tftp_close(&mut request);
    let mut close = RegistrationHeader {
        del_client: Some([1, 0x49]), tftp_request: None, ..request
    };
    trace.correlate_tftp_close(&mut close);
    assert!(close.tftp_request.is_none());
    close.del_client = Some([0, 0x49]);
    trace.correlate_tftp_close(&mut close);
    let matched = close.tftp_request.take().unwrap();
    assert_eq!((matched.opcode, matched.kind, matched.bytes), (1, 5, body.len() as u16));
    trace.correlate_tftp_close(&mut close);
    assert!(close.tftp_request.is_none());
}

#[cfg(all(test, feature = "sunfish-ipa-registration-reply-headers"))]
#[test]
fn del_client_header_keeps_named_endpoint_separate_from_control_ports() {
    let mut record = [0; 72];
    for (offset, value) in [
        (0, 0x0002_0009u32), (4, 0x4f), (8, 52), (16, 1), (20, 6),
        (28, 0xfffffffe), (36, 20), (40, 1), (44, 0xfffffffe),
        (48, 6), (52, 0), (56, 0x49),
    ] {
        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let header = RegistrationHeader::read(&record, false, None);
    assert_eq!(header.qrtr, Some([6, 0xfffffffe, 0xfffffffe]));
    assert_eq!(header.del_client, Some([0, 0x49]));
    assert_eq!(header.qmi, None);
    record[12] = 4;
    assert_eq!(RegistrationHeader::read(&record, false, None).del_client, None);
    record[12] = 0;
    record[48] = 5;
    assert_eq!(RegistrationHeader::read(&record, false, None).del_client, None);
}

#[cfg(all(test, feature = "sunfish-ipa-registration-reply-headers"))]
#[test]
fn registration_header_capture_decodes_data_and_preserves_both_ends() {
    let mut record = [0; 64];
    for (offset, value) in [
        (0, 0x0001_0009u32), (4, 42), (8, 48), (16, 1), (20, 1),
        (28, 0x4002), (36, 14), (44, 0x1f),
    ] {
        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    record[48..62].copy_from_slice(&[2, 1, 0, 0x20, 0, 7, 0, 2, 4, 0, 0, 0, 0, 0]);
    let header = RegistrationHeader::read(&record, true, Some(123));
    assert_eq!((header.command, header.channel, header.parameter), (9, 1, 42));
    assert_eq!(header.qrtr, Some([1, 0x4002, 0x1f]));
    assert_eq!(header.qmi, Some((2, 1, 0x20)));

    record[12] = 8;
    assert_eq!(RegistrationHeader::read(&record, true, None).qmi, None);
    let mut trace = RegistrationReplyFifo::default();
    for at in 0..25 {
        trace.push_header(RegistrationHeader { at: Some(at), ..header });
    }
    assert_eq!(trace.dropped, 6);
    let retained = trace.headers.map(|entry| entry.unwrap().at.unwrap());
    assert_eq!(retained, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 18, 19, 20, 21, 22, 23, 24]);
}

/// Counter-only snapshots around queueing and first writer/arming. No device reads.
#[cfg(feature = "sunfish-registry-prearm-io-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryPrearmIo {
    pub queued_at: Option<u64>,
    pub first_enter_at: Option<u64>,
    pub first_end_at: Option<u64>,
    pub arm_at: Option<u64>,
    pub queued: Option<[u64;12]>,
    pub first_enter: Option<[u64;12]>,
    pub first_end: Option<[u64;12]>,
    pub arm: Option<[u64;12]>,
    entered: bool,
    ended: bool,
    armed: bool,
}
#[cfg(feature = "sunfish-registry-prearm-io-trace")]
impl RegistryPrearmIo {
    pub fn delta(&self, to_arm: bool) -> Option<[u64;4]> {
        let a=self.queued?;let b=if to_arm{self.arm?}else{self.first_enter?};
        let mut d=[0;4];
        for i in 0..4 { if a[i]==u64::MAX || b[i]==u64::MAX{return None;}d[i]=b[i].checked_sub(a[i])?; }
        Some(d)
    }
    pub fn total(&self, to_arm: bool) -> Option<u64> {
        self.delta(to_arm)?.iter().try_fold(0u64,|a,b|a.checked_add(*b))
    }
}


/// Small scalar projection: no TFTP payload, filenames or per-transfer snapshots.
#[cfg(feature="sunfish-registry-tx3-wire-headers")]
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RegistryWireHeader {
    pub at:Option<u64>,pub tx:bool,pub command:u16,pub channel:u16,pub parameter:u32,
    pub bytes:u16,pub qrtr:Option<[u32;3]>,pub qmi:Option<(u8,u16,u16)>,pub del_client:Option<[u32;2]>,
    #[cfg(feature="sunfish-registry-tftp-wire")]
    pub tftp:Option<[u32;2]>,
}
#[cfg(feature="sunfish-registry-tx3-wire-headers")]
impl RegistryWireHeader {
    fn read(record:&[u8],tx:bool,at:Option<u64>)->Option<Self>{
        if record.len()<8 || record.len()>u16::MAX as usize{return None;}
        let h=RegistrationHeader::read(record,tx,at);
        #[cfg(feature="sunfish-registry-tftp-wire")]
        let tftp=Self::tftp_ids(record,tx,&h);
        Some(Self{at:h.at,tx:h.tx,command:h.command,channel:h.channel,parameter:h.parameter,
            bytes:h.bytes,qrtr:h.qrtr,qmi:h.qmi,del_client:h.del_client,
            #[cfg(feature="sunfish-registry-tftp-wire")]
            tftp,
        })
    }
    /// Raw scalar IDs from complete existing bytes, not TFTP service acceptance.
    #[cfg(feature="sunfish-registry-tftp-wire")]
    fn tftp_ids(record:&[u8],tx:bool,h:&RegistrationHeader)->Option<[u32;2]>{
        let [kind,source,destination]=h.qrtr?;
        let local=if tx{source}else{destination};
        if kind!=1 || (local!=crate::qcom_tftp_server_check::SERVICE_PORT
            && !crate::qcom_tftp_server_check::is_transfer_port(local)
            && !(0x5100..=0x51ff).contains(&local)) || h.command!=9
            || record.len()<16 || word(record,12)!=0 {return None;}
        let end=16usize.checked_add(word(record,8)as usize)?;
        let packet=record.get(16..end)?;
        if packet.len()<32 || word(packet,0)!=1{return None;}
        let body=packet.get(32..32usize.checked_add(word(packet,20)as usize)?)?;
        let opcode=u16::from_be_bytes(body.get(..2)?.try_into().ok()?);
        if !(1..=6).contains(&opcode){return None;}
        let number=if matches!(opcode,3|4|5){u16::from_be_bytes(body.get(2..4)?.try_into().ok()?)}else{u16::MAX};
        let category=h.tftp_request.map_or(0,|r|u32::from(r.kind));
        Some([(u32::from(opcode)<<16)|u32::from(number),(category<<24)|body.len()as u32])
    }

}
#[cfg(feature="sunfish-registry-tx3-wire-headers")]
#[derive(Clone,Copy)]
pub struct RegistryWireTrace {
    pub headers:[Option<RegistryWireHeader>;79],pub completion:Option<RegistryWireHeader>,
    pub omitted:u32,pub unparsed:u32,pub finished:bool,
}
#[cfg(feature="sunfish-registry-tx3-wire-headers")]
impl Default for RegistryWireTrace{fn default()->Self{Self{headers:[None;79],completion:None,omitted:0,unparsed:0,finished:false}}}
#[cfg(feature="sunfish-registry-tx3-wire-headers")]
impl RegistryWireTrace {
    fn record(&mut self,header:Option<RegistryWireHeader>,matching:bool){
        if self.finished{return;}
        if matching{self.finished=true;self.completion=header;if header.is_none(){self.unparsed=self.unparsed.saturating_add(1);}return;}
        let Some(header)=header else{self.unparsed=self.unparsed.saturating_add(1);return;};
        if let Some(slot)=self.headers.iter_mut().find(|h|h.is_none()){*slot=Some(header);}
        else{let tail=&mut self.headers[40..];tail.rotate_left(1);tail[38]=Some(header);self.omitted=self.omitted.saturating_add(1);}
    }
}
#[cfg(feature="sunfish-registry-tx3-wire-headers")]
const _:()={assert!(::core::mem::size_of::<RegistryWireHeader>()<=128);assert!(::core::mem::size_of::<RegistryWireTrace>()<=12288);};

/// Observes the exact registry predecessor using already validated cursor samples.
/// Peer-tail crossing is transport consumption, not application acceptance.
#[cfg(feature = "sunfish-registry-post-reply-trace")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistryReplyFifo {
    pub peer_tail_at: Option<u64>,
    pub completion: Option<ServerCheckRxDone>,
    pub finished: bool,
    pub history_evictions: u32,
}
#[cfg(feature = "sunfish-registry-post-reply-trace")]
struct RegistryReplyWindow {
    remaining: usize,
    trace: RegistryReplyFifo,
    rx: ServerCheckRxWindow,
}
#[cfg(feature = "sunfish-registry-post-reply-trace")]
impl RegistryReplyWindow {
    fn new(remaining: usize, unread: usize, at: Option<u64>) -> Self {
        Self { remaining, trace: RegistryReplyFifo { peer_tail_at: at.filter(|_| remaining == 0),
            ..RegistryReplyFifo::default() }, rx: ServerCheckRxWindow::new(unread, at) }
    }
    fn sample(&mut self, consumed: usize, advanced: usize, unread: usize, start: Option<u64>, end: Option<u64>) {
        if self.trace.finished { return; }
        if self.remaining != 0 {
            self.remaining = self.remaining.saturating_sub(consumed);
            if self.remaining == 0 { self.trace.peer_tail_at = end; }
        }
        if advanced != 0 && self.rx.advances[self.rx.next_advance].is_some() {
            self.trace.history_evictions = self.trace.history_evictions.saturating_add(1);
        }
        self.rx.sample(advanced, unread, start, end);
    }
    fn finish(&mut self, unread: usize, size: usize) {
        if self.trace.finished { return; }
        self.rx.finish(unread, size);
        self.trace.completion = self.rx.done;
        self.trace.finished = true;
    }
}

#[cfg(feature = "sunfish-ipa-registration-reply-path")]
#[derive(Clone, Copy, Default)]
pub struct RegistrationReplyFifo {
    remaining: usize,
    pub peer_tail_at: Option<u64>,
    #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
    pub completion_visibility: Option<ServerCheckRxDone>,
    pub last_empty_at: Option<u64>,
    pub max_unread: usize,
    pub polls: u32,
    pub max_poll_gap: u64,
    last_poll_at: Option<u64>,
    pub(crate) finished: bool,
    #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
    pub headers: [Option<RegistrationHeader>; 19],
    #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
    pub completion: Option<RegistrationHeader>,
    #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
    pub dropped: u32,
    #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
    tftp_requests: [Option<RegistrationTftpRequest>; 16],
}

#[cfg(feature = "sunfish-ipa-registration-reply-headers")]
impl RegistrationReplyFifo {
    #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
    fn correlate_tftp_close(&mut self, header: &mut RegistrationHeader) {
        if let Some(request) = header.tftp_request {
            self.tftp_requests.rotate_left(1);
            self.tftp_requests[15] = Some(request);
        }
        if let Some(peer) = header.del_client {
            for request in self.tftp_requests.iter_mut().rev() {
                if request.is_some_and(|request| request.peer == peer) {
                    header.tftp_request = header.tftp_request.or(request.take());
                }
            }
        }
    }

    fn push_header(&mut self, header: RegistrationHeader) {
        if let Some(slot) = self.headers.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some(header);
        } else {
            // Keep the opening reply and nine subsequent records, then
            // retain the nine most recent records in chronological order.
            let recent = &mut self.headers[10..];
            recent.rotate_left(1);
            recent[8] = Some(header);
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

#[cfg(feature = "sunfish-tftp-first-oack-headers")]
#[derive(Clone, Copy, Default)]
pub struct FirstOackHeaders {
    pub recent: [Option<RegistrationHeader>; 12],
    pub completion: Option<RegistrationHeader>,
    pub omitted: u32,
}

#[cfg(feature = "sunfish-tftp-first-oack-headers")]
impl FirstOackHeaders {
    fn record(&mut self, header: RegistrationHeader, matching: bool) {
        if self.completion.is_some() {
            return;
        }
        if matching {
            self.completion = Some(header);
        } else if let Some(slot) = self.recent.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(header);
        } else {
            self.recent.rotate_left(1);
            self.recent[11] = Some(header);
            self.omitted = self.omitted.saturating_add(1);
        }
    }
}

#[cfg(all(test, feature = "sunfish-tftp-first-oack-headers"))]
#[test]
fn first_oack_header_window_wraps_and_freezes_only_on_matching_completion() {
    let mut trace = FirstOackHeaders::default();
    for id in 0..16u32 {
        let mut record = [6, 0, 2, 0, 0, 0, 0, 0];
        record[4..].copy_from_slice(&id.to_le_bytes());
        trace.record(RegistrationHeader::read(&record, false, None), false);
    }
    let completion = RegistrationHeader::read(&[14, 0, 2, 0, 42, 0, 0, 0], false, None);
    trace.record(completion, true);
    trace.record(completion, false);
    assert_eq!(trace.omitted, 4);
    assert_eq!(trace.recent.map(|entry| entry.unwrap().parameter),
        [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    assert_eq!(trace.completion.unwrap().parameter, 42);
}

#[cfg(feature = "sunfish-tftp-oack-peer-tail")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OackPeerTail {
    pub pending_bytes: usize,
    pub last_before: Option<u64>,
    pub first_crossed: Option<u64>,
    pub at_close: Option<usize>,
}

#[cfg(feature = "sunfish-tftp-oack-peer-tail")]
impl OackPeerTail {
    fn new(pending_bytes: usize, at: Option<u64>) -> Self {
        Self {
            pending_bytes,
            last_before: None,
            first_crossed: at.filter(|_| pending_bytes == 0),
            at_close: None,
        }
    }

    fn observe(&mut self, consumed: usize, before: Option<u64>, after: Option<u64>) {
        if self.pending_bytes == 0 { return; }
        self.pending_bytes = self.pending_bytes.saturating_sub(consumed);
        if self.pending_bytes == 0 { self.first_crossed = after; }
        else { self.last_before = before; }
    }

    fn close(&mut self) {
        if self.at_close.is_none() { self.at_close = Some(self.pending_bytes); }
    }
}

#[cfg(all(test, feature = "sunfish-tftp-oack-peer-tail"))]
#[test]
fn oack_peer_tail_latches_close_and_crossing_independently() {
    let mut trace = OackPeerTail::new(104, Some(10));
    assert_eq!(trace.last_before, None);
    trace.observe(16, Some(11), Some(12));
    assert_eq!(trace.pending_bytes, 88);
    assert_eq!(trace.last_before, Some(11));
    trace.close();
    trace.observe(120, Some(14), Some(15));
    trace.close();
    assert_eq!(trace.first_crossed, Some(15));
    assert_eq!(trace.at_close, Some(88));

    let mut already_read = OackPeerTail::new(0, Some(10));
    already_read.close();
    assert_eq!(already_read.first_crossed, Some(10));
    assert_eq!(already_read.at_close, Some(0));
}

#[cfg(feature = "sunfish-ipa-init-close-visibility")]
#[derive(Clone, Copy, Debug, Default)]
pub struct InitCloseFifo {
    pub fresh_unread: Option<u32>,
    pub sample_start: Option<u64>,
    pub sample_end: Option<u64>,
    fresh_head: Option<u64>,
    close_end: Option<u64>,
    pub bounds: Option<ServerCheckRxDone>,
}

#[cfg(feature = "sunfish-ipa-init-close-visibility")]
impl InitCloseFifo {
    pub fn visible_at_sample(&self) -> Option<bool> {
        Some(self.close_end? <= self.fresh_head?)
    }
}

#[cfg(feature = "sunfish-ipa-reg-close-fifo")]
#[derive(Clone, Copy, Debug, Default)]
pub struct RegistrationCloseFifo {
    pub pre: Option<u32>,
    pub post: Option<u32>,
    pub fresh: Option<u32>,
    pub fresh_start: Option<u64>,
    pub fresh_end: Option<u64>,
    pub consumed: u32,
    pub end: Option<u32>,
    pub record_bytes: Option<u32>,
    already_closed: bool,
}

#[cfg(feature = "sunfish-ipa-reg-close-fifo")]
impl RegistrationCloseFifo {
    // Both head samples use the same host-owned RX tail. Compare the end of
    // the exact, subsequently decoded record, including its GLINK padding.
    pub fn status(&self) -> u32 {
        if self.already_closed { return 4; }
        let Some(end) = self.end else { return 0; };
        match (self.pre, self.post) {
            (Some(pre), Some(post)) if pre <= post => {
                if end <= pre { 1 } else if end > post { 2 } else { 3 }
            }
            _ => u32::MAX,
        }
    }
}

#[cfg(feature = "sunfish-grant-rx-open")]
#[derive(Default)]
pub struct GrantReceiveTrace {
    awaiting_ack: bool,
    #[cfg(feature = "sunfish-wrq-grant-turns")]
    pub turn_count: u32,
    #[cfg(feature = "sunfish-wrq-grant-turns")]
    pub turn_ack: u32,
    #[cfg(feature = "sunfish-wrq-grant-turns")]
    pub primary_checked: u32,
    pub ack_count: u32,
    pub ack_at: Option<u64>,
    pub taken: bool,
    pub turn_at: Option<u64>,
    pub open: Option<(u16, u16)>,
    pub byte_bounds: Option<ServerCheckRxDone>,
    pub open_bounds: Option<ServerCheckRxDone>,
    pub return_tail: Option<ServerCheckTail>,
}

struct Ring<I> {
    #[cfg(feature = "sunfish-grant-rx-open")]
    grant_receive: GrantReceiveTrace,
    #[cfg(feature = "sunfish-wrq-return-pending")]
    wrq_return_pending: Option<WrqReturnPending>,
    #[cfg(feature = "sunfish-grant-buffer-reads")]
    grant_reads: grant_reads::GrantReadLedger,
    #[cfg(feature = "sunfish-grant-buffer-reads")]
    grant_session_write: bool,
    io: I,
    generation: u64,
    identity: u64,
    words: [u32; 8],
    read_notification_end: Option<u32>,
    record: [u8; MAX_RX_RECORD],
    #[cfg(feature = "sunfish-ipa-init-close-visibility")]
    init_close: InitCloseFifo,
    #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
    reg_close: Option<RegistrationCloseFifo>,
    #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
    reg_pre_unread: Option<u32>,
    rx_bytes: Option<usize>,
    #[cfg(feature = "sunfish-ipa-registration-reply-path")]
    registration_reply: Option<RegistrationReplyFifo>,
    #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
    registration_reply_rx: Option<ServerCheckRxWindow>,
    #[cfg(feature = "sunfish-registry-post-reply-trace")]
    registry_reply: Option<RegistryReplyWindow>,
    #[cfg(feature="sunfish-registry-tx3-wire-headers")]
    registry_wire: Option<RegistryWireTrace>,
    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    prearm_io: Option<RegistryPrearmIo>,
    #[cfg(feature = "sunfish-tftp-tail-observer")]
    tftp_tail_remaining: Option<usize>,
    #[cfg(feature = "sunfish-tftp-tail-observer")]
    tftp_tail_seen: bool,
    #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
    first_server_check_tail: ServerCheckTail,
    #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
    oack_peer_tail: Option<OackPeerTail>,
    #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
    oack_close_fifo: Option<ServerCheckRxWindow>,
    #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
    reserved_oack_rx: Option<ServerCheckRxWindow>,
    #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
    reserved_oack_ahead: Option<OackAheadCapture>,
    #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
    reserved_writer_io: ReservedWriterIoTrace,
    #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
    first_server_check_clock: Option<fn() -> u64>,
    #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
    server_check_rx: Option<ServerCheckRxWindow>,
    #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
    registration_rx: Option<ServerCheckRxWindow>,
    #[cfg(feature = "sunfish-ipa-ingress-work")]
    ipa_ingress_work: IpaIngressWork,
    #[cfg(feature = "sunfish-spare-wrq-ingress")]
    wrq_work: WrqReceiveWork,
    #[cfg(feature = "sunfish-wrq-prefix-close")]
    wrq_prefix: WrqPrefix,
    #[cfg(feature = "sunfish-ipa-announcement-fifo")]
    ipa_announcement_rx: Option<ServerCheckRxDone>,
    #[cfg(feature = "sunfish-ipa-announcement-fifo")]
    track_ipa_announcement: bool,
    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    first_wrq_rx: Option<ServerCheckRxDone>,
    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    track_first_wrq: bool,
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    control: Option<boot::Runtime>,
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    open_diagnostic: Option<OpenDiagnostic>,
}

impl<I: Io> Ring<I> {
    fn observation(&mut self) -> Result<(), Error> {
        let observation = self.io.observe()?;
        self.validate_observation(observation)
    }

    fn validate_observation(&self, observation: Observation) -> Result<(), Error> {
        ready(observation, self.generation)?;
        if observation.identity != self.identity {
            return Err(Error::IdentityChanged);
        }
        Ok(())
    }

    fn barrier_observation(&mut self, kind: Barrier) -> Result<(), Error> {
        #[cfg(feature = "sunfish-glink-barrier-observe-pair")]
        {
            let observation = self.io.barrier_observe(kind)?;
            self.validate_observation(observation)
        }
        #[cfg(not(feature = "sunfish-glink-barrier-observe-pair"))]
        {
            self.io.barrier(kind)?;
            self.observation()
        }
    }

    // Exactly the original barrier -> observation order at cursor/doorbell
    // boundaries. The opt-in native override shares only the outer domain
    // pre/post pair; both controller actions retain their own peer checks.
    fn adjacent_barrier_observation(&mut self, kind: Barrier) -> Result<(), Error> {
        #[cfg(feature = "sunfish-glink-adjacent-barrier-pairs")]
        { self.barrier_observation(kind) }
        #[cfg(not(feature = "sunfish-glink-adjacent-barrier-pairs"))]
        {
            self.io.barrier(kind)?;
            self.observation()
        }
    }

    #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
    fn capture_registration_rx(&mut self, size: usize, completion: bool, _event: &Receive) {
        if let Some(trace) = self.registration_reply.as_mut().filter(|trace| !trace.finished) {
            let header = RegistrationHeader::read(&self.record[..size], false,
                self.first_server_check_clock.map(|clock| clock()));
            #[cfg(feature = "sunfish-ipa-grant-tftp-peer")]
            let header = {
                let mut header = header;
                #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
                if let Some(request) = header.tftp_request.as_mut()
                    && let Receive::Session(core::Event::TftpRequest(handle)) = _event
                {
                    request.handle = Some(*handle);
                }
                trace.correlate_tftp_close(&mut header);
                header
            };
            if completion {
                trace.completion = Some(header);
            } else {
                trace.push_header(header);
            }
        }
    }

    #[cfg(feature="sunfish-registry-tx3-wire-headers")]
    fn capture_registry_wire_rx(&mut self,size:usize,matching:bool){
        if self.registry_wire.as_ref().is_some_and(|t|!t.finished){
            let h=RegistryWireHeader::read(&self.record[..size],false,self.first_server_check_clock.map(|clock|clock()));
            self.registry_wire.as_mut().unwrap().record(h,matching);
        }
    }

    #[cfg(feature = "sunfish-tftp-first-oack-headers")]
    fn capture_first_oack_rx(&mut self, size: usize, matching: bool) {
        if let Some(trace) = self.server_check_rx.as_mut().filter(|trace| trace.done.is_none()) {
            trace.headers.record(RegistrationHeader::read(&self.record[..size], false,
                self.first_server_check_clock.map(|clock| clock())), matching);
        }
    }

    #[cfg_attr(feature="sunfish-wrq-post-return-callers",track_caller)]
    fn sample(&mut self) -> Result<(), Error> {
        #[cfg(feature = "sunfish-wrq-stage-costs")]
        let wrq_sample_start = self.wrq_work_clock();
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        let oack_sample_start = self.oack_peer_tail.as_ref().filter(|trace| trace.pending_bytes != 0)
            .and(self.first_server_check_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
        let close_sample_start = self.oack_close_fifo.as_ref().filter(|trace| trace.done.is_none())
            .and(self.first_server_check_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
        let reserved_sample_start = self.reserved_oack_rx.as_ref().filter(|trace| trace.done.is_none())
            .and(self.first_server_check_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-registry-post-reply-trace")]
        let registry_reply_start = self.registry_reply.as_ref().filter(|w| !w.trace.finished)
            .and(self.first_server_check_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
        let registration_reply_start = self.registration_reply_rx.as_ref()
            .and(self.first_server_check_clock).map(|clock| clock());
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        let trace_sample = self.server_check_rx.as_ref().is_some_and(|trace| trace.done.is_none());
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        let sample_start = if trace_sample { self.first_server_check_clock.map(|clock| clock()) } else { None };
        #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
        let track_registration = self.registration_rx.as_ref().is_some_and(|trace| trace.done.is_none());
        #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
        let track_registration = track_registration || self.track_first_wrq;
        #[cfg(feature = "sunfish-ipa-announcement-fifo")]
        let track_registration = track_registration || self.track_ipa_announcement;
        // Keep the shared RX-head history alive until normal ingress identifies
        // the first close of the original writer. Earlier snapshots stay frozen.
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        let track_registration = track_registration
            || (self.registration_rx.is_some() && self.init_close.close_end.is_none());
        #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
        let registration_start = track_registration.then_some(self.first_server_check_clock)
            .flatten().map(|clock| clock());
        #[cfg(feature = "sunfish-wrq-validation-path")]
        self.io.validation_cost_begin(wrq_sample_start.and(self.first_server_check_clock));
        self.observation()?;
        #[cfg(feature = "sunfish-wrq-validation-path")]
        let status_end=self.wrq_work_clock();
        #[cfg(feature = "sunfish-glink-combined-cursor-sample")]
        let (rx, words) = self.io.cursor_snapshot()?;
        #[cfg(not(feature = "sunfish-glink-combined-cursor-sample"))]
        let rx = self.io.rx_length()?;
        #[cfg(feature = "sunfish-wrq-validation-path")]
        let cursor_end=self.wrq_work_clock();
        if self.rx_bytes.is_some() && rx != self.rx_bytes {
            return Err(Error::IdentityChanged);
        }
        #[cfg(feature = "sunfish-glink-combined-cursor-sample")]
        let new = validate_descriptors(words, rx)?;
        #[cfg(not(feature = "sunfish-glink-combined-cursor-sample"))]
        let new = descriptors(&mut self.io, rx)?;
        if new[1] != self.words[1] || new[2] != self.words[2] {
            return Err(Error::OwnedCursorChanged);
        }
        if new[4..] != self.words[4..] {
            return Err(Error::ReservedChanged);
        }
        let consumed = distance(self.words[0], new[0]);
        if consumed > distance(self.words[0], self.words[1])
            || rx.is_some_and(|length| {
                ring_distance(self.words[3], new[3], length)
                    > length - FULL_RESERVE - ring_distance(self.words[2], self.words[3], length)
            })
        {
            return Err(Error::PeerOverrun);
        }
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        let old_rx_head = self.words[3];
        #[cfg(feature = "sunfish-tftp-tail-observer")]
        if let Some(remaining) = self.tftp_tail_remaining {
            if consumed >= remaining {
                self.tftp_tail_seen = true;
                self.tftp_tail_remaining = None;
            } else {
                self.tftp_tail_remaining = Some(remaining - consumed);
            }
        }
        if self
            .read_notification_end
            .is_some_and(|end| distance(self.words[0], end) <= consumed)
        {
            self.read_notification_end = None;
        }
        self.words = new;
        self.rx_bytes = rx;
        self.barrier_observation(Barrier::Acquire)?;
        #[cfg(feature = "sunfish-wrq-validation-path")]
        {
            let end=self.wrq_work_clock();
            #[cfg(feature="sunfish-wrq-domain-costs")]
            let parts=self.io.validation_cost_parts();
            let domains=self.io.validation_cost_end();
            if let (Some(a),Some(b),Some(c),Some(d))=(wrq_sample_start,status_end,cursor_end,end) {
                #[cfg(not(feature="sunfish-wrq-domain-costs"))]
                self.wrq_work.validation.sample([a,b,c,d],domains);
                #[cfg(all(feature="sunfish-wrq-domain-costs",not(feature="sunfish-wrq-post-return-callers")))]
                self.wrq_work.validation.sample_parts([a,b,c,d],domains,parts);
                #[cfg(feature="sunfish-wrq-post-return-callers")]
                {
                    let pending=self.rx_bytes.map(|length|ring_distance(self.words[2],self.words[3],length) as u32);
                    self.wrq_work.validation.sample_parts_at([a,b,c,d],domains,parts,SampleSite::caller(),pending);
                }
            }
        }
        #[cfg(feature = "sunfish-spare-wrq-ingress")]
        if let (Some(start), Some(end)) = (
            { #[cfg(feature = "sunfish-wrq-stage-costs")] { wrq_sample_start }
              #[cfg(not(feature = "sunfish-wrq-stage-costs"))] { registration_start } },
            self.wrq_work_clock()) {
            self.wrq_work.sample(start, end);
        }
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        self.grant_reads.sample(consumed, registration_start, self.first_server_check_clock);
        #[cfg(feature = "sunfish-ipa-registration-reply-path")]
        if let Some(trace) = self.registration_reply.as_mut().filter(|trace| !trace.finished) {
            if trace.remaining != 0 {
                trace.remaining = trace.remaining.saturating_sub(consumed);
                if trace.remaining == 0 {
                    trace.peer_tail_at = self.first_server_check_clock.map(|clock| clock());
                }
            }
            if let Some(length) = rx {
                let unread = ring_distance(new[2], new[3], length);
                trace.max_unread = trace.max_unread.max(unread);
                if unread == 0 {
                    trace.last_empty_at = self.first_server_check_clock.map(|clock| clock());
                }
            }
        }
        #[cfg(feature = "sunfish-registry-post-reply-trace")]
        if let Some(trace) = self.registry_reply.as_mut().filter(|w| !w.trace.finished)
            && let Some(length) = rx
        {
            trace.sample(consumed, ring_distance(old_rx_head, new[3], length),
                ring_distance(new[2], new[3], length), registry_reply_start,
                self.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
        if let Some(start) = registration_start && let Some(length) = rx
        {
            let unread = ring_distance(new[2], new[3], length);
            let end = self.first_server_check_clock.map(|clock| clock());
            if let Some(trace) = self.registration_rx.as_mut() {
                trace.sample(ring_distance(old_rx_head, new[3], length), unread, Some(start), end);
            } else {
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                { self.registration_rx = Some(ServerCheckRxWindow::new(unread, end)); }
            }
        }
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        if trace_sample && let Some(trace) = self.server_check_rx.as_mut()
            && let Some(length) = rx
        {
            trace.sample(ring_distance(old_rx_head, new[3], length),
                ring_distance(new[2], new[3], length), sample_start,
                self.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
        if let Some(start) = registration_reply_start
            && let Some(length) = rx
            && let Some(trace) = self.registration_reply_rx.as_mut()
        {
            trace.sample(ring_distance(old_rx_head, new[3], length),
                ring_distance(new[2], new[3], length), Some(start),
                self.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
        if let Some(trace) = self.oack_close_fifo.as_mut().filter(|trace| trace.done.is_none())
            && let Some(length) = rx
        {
            trace.sample(ring_distance(old_rx_head, new[3], length),
                ring_distance(new[2], new[3], length), close_sample_start,
                self.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
        if let Some(trace) = self.reserved_oack_rx.as_mut().filter(|trace| trace.done.is_none())
            && let Some(length) = rx
        {
            trace.sample(ring_distance(old_rx_head, new[3], length),
                ring_distance(new[2], new[3], length), reserved_sample_start,
                self.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        if let Some(trace) = self.oack_peer_tail.as_mut().filter(|trace| trace.pending_bytes != 0) {
            trace.observe(consumed, oack_sample_start, self.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-grant-rx-open")]
        if let Some(ServerCheckTail::Pending(remaining)) = self.grant_receive.return_tail {
            self.grant_receive.return_tail = Some(if consumed >= remaining {
                ServerCheckTail::Consumed(self.first_server_check_clock.map(|clock| clock()))
            } else { ServerCheckTail::Pending(remaining - consumed) });
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        if let ServerCheckTail::Pending(remaining) = self.first_server_check_tail {
            self.first_server_check_tail = if consumed >= remaining {
                ServerCheckTail::Consumed(self.first_server_check_clock.map(|clock| clock()))
            } else {
                ServerCheckTail::Pending(remaining - consumed)
            };
        }
        Ok(())
    }

    fn read_rx_at(&mut self, relative: usize, out: &mut [u8]) -> Result<(), Error> {
        let length = self.rx_bytes.ok_or(Error::NotReady)?;
        let at = ring_advance(self.words[2], relative, length) as usize;
        if out.len() > length {
            return Err(Error::InvalidRecord);
        }
        let first = out.len().min(length - at);
        self.io.read_rx(at, &mut out[..first])?;
        if first != out.len() {
            self.io.read_rx(0, &mut out[first..])?;
        }
        Ok(())
    }

    fn peek(&mut self) -> Result<Result<usize, Receive>, Error> {
        self.sample()?;
        let Some(length) = self.rx_bytes else {
            return Ok(Err(Receive::Idle));
        };
        let available = ring_distance(self.words[2], self.words[3], length);
        if available == 0 {
            return Ok(Err(Receive::Idle));
        }
        let mut prefix = [0; 16];
        self.read_rx_at(0, &mut prefix[..8])?;
        let command = u16::from_le_bytes([prefix[0], prefix[1]]);
        let size = match command {
            #[cfg(feature = "sunfish-linux-mpss-glink")]
            2 if self.control.is_some() => match boot::open_length(&prefix) {
                Ok(size) => size,
                Err(error) => {
                    self.open_diagnostic = Some(self.control.as_ref().unwrap().channels.open_diagnostic(&prefix[..8], 1));
                    return Err(error);
                }
            },
            5 => {
                let count = word(&prefix, 4) as usize;
                if count == 0 || count > core::REMOTE_INTENTS {
                    return Err(Error::InvalidRecord);
                }
                8 + count * 8
            }
            6 | 7 | 14 => 8,
            #[cfg(feature = "sunfish-glink-intent-request-experiment")]
            8 => 8,
            13 if prefix[2..8] == [0; 6] => 8,
            13 => return Err(Error::InvalidRecord),
            9 | 12 => {
                if available < 16 {
                    return Ok(Err(Receive::Pending {
                        available,
                        needed: 16,
                    }));
                }
                self.read_rx_at(8, &mut prefix[8..])?;
                let chunk = word(&prefix, 8) as usize;
                let remaining = word(&prefix, 12) as usize;
                if chunk == 0
                    || chunk > core::MAX_DATAGRAM_BYTES
                    || remaining > core::MAX_DATAGRAM_BYTES
                    || chunk + remaining > core::MAX_DATAGRAM_BYTES
                {
                    return Err(Error::InvalidRecord);
                }
                (16 + chunk + 7) & !7
            }
            _ => return Err(Error::UnsupportedCommand),
        };
        if available < size {
            return Ok(Err(Receive::Pending {
                available,
                needed: size,
            }));
        }
        // Copy a bounded record without borrowing shared memory into the core.
        let at = self.words[2] as usize;
        let first = size.min(length - at);
        self.io.read_rx(at, &mut self.record[..first])?;
        if first != size {
            self.io.read_rx(0, &mut self.record[first..size])?;
        }
        let prefix_size = if matches!(command, 9 | 12) { 16 } else { 8 };
        if self.record[..prefix_size] != prefix[..prefix_size] {
            return Err(Error::ChangedRecord);
        }
        self.io.barrier(Barrier::Acquire)?;
        let mut check = [0; 256];
        for offset in (0..size).step_by(check.len()) {
            let count = (size - offset).min(check.len());
            self.read_rx_at(offset, &mut check[..count])?;
            if self.record[offset..offset + count] != check[..count] {
                return Err(Error::ChangedRecord);
            }
        }
        self.sample()?;
        Ok(Ok(size))
    }

    fn consume(&mut self, size: usize, notification: bool) -> Result<(), Error> {
        self.sample()?;
        self.adjacent_barrier_observation(Barrier::Release)?;
        let next = ring_advance(self.words[2], size, self.rx_bytes.ok_or(Error::NotReady)?);
        self.io.store_rx_tail(next)?;
        #[cfg(feature = "sunfish-oack-grant-ack-turn")]
        if self.grant_reads.pending_active(self.words[2]) {
            self.grant_reads.advance_pending(self.words[2], next, size as u32,
                self.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-wrq-return-pending")]
        if let Some(trace) = self.wrq_return_pending.as_mut().filter(|trace| trace.follows(self.words[2])) {
            trace.advanced(next, size as u32, self.first_server_check_clock.map(|clock| clock()));
        }
        self.words[2] = next;
        self.io.barrier(Barrier::Complete)?;
        self.sample()?;
        if notification {
            self.notify()?;
        }
        Ok(())
    }

    fn notify(&mut self) -> Result<(), Error> {
        self.adjacent_barrier_observation(Barrier::Complete)?;
        self.io.notify_peer()?;
        self.adjacent_barrier_observation(Barrier::Complete)
    }

    fn write(&mut self, record: &[u8], reserve: usize) -> Result<(), Error> {
        self.write_checked(record, reserve, false).map(|_| ())
    }

    // False leaves AP TX head untouched. Bytes staged for readback are
    // private to the writer until that head is published.
    fn write_checked(&mut self, record: &[u8], reserve: usize, yield_to_rx: bool) -> Result<bool, Error> {
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        let start = self.ingress_clock();
        #[cfg(feature = "sunfish-spare-wrq-ingress")]
        let wrq_start = self.wrq_work_clock();
        let result = self.write_checked_inner(record, reserve, yield_to_rx);
        #[cfg(feature = "sunfish-spare-wrq-ingress")]
        if let (Some(start), Some(end)) = (wrq_start, self.wrq_work_clock()) {
            let kind = match result { Ok(true) => 1, Ok(false) => 2, Err(Error::WouldBlock) => 3, Err(_) => 6 };
            self.wrq_work.record(start, end, kind, record);
        }
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        if let (Some(start), Some(end)) = (start, self.ingress_clock()) && record.len() >= 8 {
            let kind = match result {
                Ok(true) => 1, Ok(false) => 2, Err(Error::WouldBlock) => 3, Err(_) => 6,
            };
            self.ipa_ingress_work.push(IpaIngressRecord { start, end, position: None,
                kind, cmd: record[0], size: record.len() as u16, detail: word(record, 4),
            #[cfg(feature = "sunfish-wrq-stage-costs")] protocol: None,
        });
        }
        result
    }

    #[cfg(feature = "sunfish-ipa-ingress-work")]
    fn ingress_clock(&self) -> Option<u64> {
        self.track_ipa_announcement.then_some(self.first_server_check_clock).flatten().map(|clock| clock())
    }

    #[cfg(feature = "sunfish-spare-wrq-ingress")]
    fn wrq_work_clock(&self) -> Option<u64> {
        #[cfg(feature = "sunfish-wrq-stage-costs")]
        {
            return ((self.track_first_wrq || self.registration_rx.is_some())
                && {
                    #[cfg(feature = "sunfish-wrq-validation-path")] {self.wrq_work.validation.finished.is_none()}
                    #[cfg(not(feature = "sunfish-wrq-validation-path"))] {self.wrq_work.finished.is_none()}
                }).then_some(self.first_server_check_clock)
                .flatten().map(|clock| clock());
        }
        #[cfg(not(feature = "sunfish-wrq-stage-costs"))]
        self.track_first_wrq.then_some(self.first_server_check_clock).flatten().map(|clock| clock())
    }

    fn write_checked_inner(&mut self, record: &[u8], reserve: usize, yield_to_rx: bool) -> Result<bool, Error> {
        if record.is_empty() || record.len() % 8 != 0 || record.len() > core::MAX_TX_RECORD_BYTES {
            return Err(Error::InvalidRecord);
        }
        self.sample()?;
        if yield_to_rx && self.words[2] != self.words[3] { return Ok(false); }
        let free = FIFO_BYTES - distance(self.words[0], self.words[1]);
        if record.len() > free.saturating_sub(reserve) {
            return Err(Error::WouldBlock);
        }
        let at = self.words[1] as usize;
        let first = record.len().min(FIFO_BYTES - at);
        self.io.write_tx(at, &record[..first])?;
        if first != record.len() {
            self.io.write_tx(0, &record[first..])?;
        }
        self.io.barrier(Barrier::Release)?;
        let mut check = [0; core::MAX_TX_RECORD_BYTES];
        self.io.read_tx(at, &mut check[..first])?;
        if first != record.len() {
            self.io.read_tx(0, &mut check[first..record.len()])?;
        }
        if record != &check[..record.len()] {
            return Err(Error::Readback);
        }
        self.sample()?;
        if yield_to_rx && self.words[2] != self.words[3] { return Ok(false); }
        self.barrier_observation(Barrier::Release)?;
        let next = advance(self.words[1], record.len());
        #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
        { self.reg_pre_unread = self.rx_bytes.map(|length| ring_distance(self.words[2], self.words[3], length) as u32); }
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        let offer = self.grant_session_write && matches!(record[0], 5 | 14);
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        let before_head = (offer && self.grant_reads.clock_armed).then_some(self.first_server_check_clock).flatten().map(|now| now());
        self.io.store_tx_head(next)?;
        #[cfg(feature = "sunfish-oack-grant-outcome")]
        if self.grant_session_write {
            let oack = self.grant_reads.oack_pool.is_some() && record.len() >= 50
                && record[0] == 9 && word(record, 28) == 0x5001
                && word(record, 44) == 0x1f && record[48..50] == [0, 6];
            if oack || self.grant_reads.watches_grant_head(record) {
                let at = self.first_server_check_clock.map(|clock| clock());
                self.grant_reads.record_grant_head(record, distance(self.words[0], next), at);
                if oack && let Some((ids, states)) = self.grant_reads.oack_pool.take() {
                    let unread = self.rx_bytes.map_or(0, |length| ring_distance(self.words[2], self.words[3], length));
                    self.grant_reads.capture_oack(ids, states, unread as u32, at);
                    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
                    self.grant_reads.start_pending(self.words[2]);
                }
            }
        }
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        if offer {
            let end = self.grant_reads.clock_armed.then_some(self.first_server_check_clock).flatten().map(|now| now());
            self.grant_reads.offer(record, distance(self.words[0], next), before_head, end);
        }
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        if self.wrq_prefix.watch_head && matches!(record[0], 9 | 12)
        {
            self.wrq_prefix.last_data_head = self.first_server_check_clock.map(|now| now());
        }
        self.words[1] = next;
        self.io.barrier(Barrier::Complete)?;
        self.sample()?;
        self.notify()?;
        #[cfg(feature = "sunfish-tftp-first-oack-headers")]
        if let Some(trace) = self.server_check_rx.as_mut().filter(|trace| trace.done.is_none()) {
            trace.headers.record(RegistrationHeader::read(record, true,
                self.first_server_check_clock.map(|clock| clock())), false);
        }
        #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
        if let Some(trace) = self.registration_reply.as_mut().filter(|trace| !trace.finished) {
            trace.push_header(RegistrationHeader::read(record, true,
                self.first_server_check_clock.map(|clock| clock())));
        }
        #[cfg(feature="sunfish-registry-tx3-wire-headers")]
        if self.registry_wire.as_ref().is_some_and(|t|!t.finished){
            let h=RegistryWireHeader::read(record,true,self.first_server_check_clock.map(|clock|clock()));
            self.registry_wire.as_mut().unwrap().record(h,false);
        }
        Ok(true)
    }

    fn blocked_notice(&mut self) -> Result<bool, Error> {
        if self.read_notification_end.is_some() {
            return Ok(false);
        }
        match self.write(&[13, 0, 0, 0, 0, 0, 0, 0], FULL_RESERVE) {
            Ok(()) => {
                self.read_notification_end = Some(self.words[1]);
                Ok(true)
            }
            Err(Error::WouldBlock) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// Owns the session and FIFO writer together. No mutable session/IO escape is
/// provided. Peer simulation can use separately owned fake memory handles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionFailureDiagnostic {
    pub stage: u32,
    pub command: u32,
    pub parameter: u32,
}
#[cfg(feature = "sunfish-first-oack-grant-window")]
#[derive(Clone, Copy, Debug, Default)]
pub struct FirstGrantTrace {
    #[cfg(feature = "sunfish-first-wrq-return-oack")]
    pub wrq_return_outcome: u8,
    #[cfg(feature = "sunfish-first-wrq-return-oack")]
    pub wrq_return_mask: Option<u16>,
    #[cfg(feature = "sunfish-first-wrq-return-oack")]
    pub wrq_return_fitting: Option<u8>,
    #[cfg(feature = "sunfish-first-wrq-spare-oack")]
    pub direct_turn: u32,
    #[cfg(feature = "sunfish-first-wrq-spare-oack")]
    pub request_108_at: Option<u64>,
    #[cfg(feature = "sunfish-grant-ad-writer-trace")]
    pub backing: core::GrantBackingTrace,
    pub trigger: u32,
    pub advertisement_at: Option<u64>,
    pub ack_at: Option<u64>,
    pub outcome: u32,
    pub unread_bytes: u32,
    #[cfg(feature = "sunfish-first-oack-yield-rx")]
    pub window: GrantRxWindow,
}

#[cfg(feature = "sunfish-first-oack-yield-rx")]
#[derive(Clone, Copy, Debug, Default)]
pub struct GrantRxWindow {
    next_tail: u32,
    pub remaining: u32,
    pub records: u32,
    pub commands: u32,
    pub qrtr_types: u32,
    pub first_close: Option<[u32; 2]>,
    pub matched_close: bool,
    pub partial: bool,
    pub lost_cursor: bool,
}

#[cfg(feature = "sunfish-first-oack-yield-rx")]
impl GrantRxWindow {
    // Observe only records successfully consumed by the ordinary receiver.
    // A frame completed by later arrivals cannot identify a complete close at yield.
    fn consumed(&mut self, start: u32, end: u32, record: &[u8], peer: Option<[u32; 2]>) {
        if self.remaining == 0 || self.lost_cursor { return; }
        if start != self.next_tail {
            self.lost_cursor = true;
            return;
        }
        self.next_tail = end;
        if record.len() > self.remaining as usize {
            self.partial = true;
            self.remaining = 0;
            return;
        }
        self.remaining -= record.len() as u32;
        self.records += 1;
        let command = u16::from_le_bytes([record[0], record[1]]);
        self.commands |= 1u32.checked_shl(command.into()).unwrap_or(0);
        if command == 9 && record.len() >= 48 {
            let chunk = word(record, 8) as usize;
            if chunk >= 32 && let Some(packet) = record[16..].get(..chunk)
                && word(packet, 0) == 1
            {
                self.qrtr_types |= 1u32.checked_shl(word(packet, 4)).unwrap_or(0);
                if let Some(endpoint) = del_client_endpoint(record, packet) {
                    self.first_close.get_or_insert(endpoint);
                    self.matched_close |= peer == Some(endpoint);
                }
            }
        }
    }
}

pub struct Adapter<I> {
    #[cfg(feature = "sunfish-first-oack-grant-window")]
    first_grant: FirstGrantTrace,
    ring: Ring<I>,
    session: Session,
    quarantined: bool,
    session_failure: Option<SessionFailureDiagnostic>,
}

impl<I: Io> Adapter<I> {
    #[cfg(feature = "sunfish-wlan-elf-phase-io")]
    pub fn elf_io_totals(&self) -> Option<[u64; 12]> { self.ring.io.elf_io_totals() }

    #[allow(dead_code)] // Consumed by the combined runtime when it is enabled.
    pub fn with_io<T>(&mut self, action: impl FnOnce(&mut I) -> T) -> T {
        action(&mut self.ring.io)
    }

    /// Attach to established 16 KiB rings without clearing bytes or cursors.
    /// Caller must have completed GLINK channel negotiation in this generation.
    pub fn attach(mut io: I, session: Session, generation: u64) -> Result<Self, Error> {
        session.check(generation)?;
        if !session.transport_is_pristine() {
            return Err(Error::SessionNotFresh);
        }
        let observation = io.observe()?;
        ready(observation, generation)?;
        let rx_bytes = io.rx_length()?;
        if rx_bytes.is_none() {
            return Err(Error::NotReady);
        }
        let words = descriptors(&mut io, rx_bytes)?;
        let mut this = Self {
            #[cfg(feature = "sunfish-first-oack-grant-window")]
            first_grant: FirstGrantTrace::default(),
            ring: Ring {
                io,
                generation,
                identity: observation.identity,
                words,
                #[cfg(feature = "sunfish-grant-rx-open")]
                grant_receive: Default::default(),
                #[cfg(feature = "sunfish-wrq-return-pending")]
                wrq_return_pending: None,
                read_notification_end: None,
                record: [0; MAX_RX_RECORD],
                #[cfg(feature = "sunfish-ipa-init-close-visibility")]
                init_close: InitCloseFifo::default(),
                #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
                reg_close: None,
                #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
                reg_pre_unread: None,
                rx_bytes,
                #[cfg(feature = "sunfish-ipa-registration-reply-path")]
                registration_reply: None,
                #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
                registration_reply_rx: None,
                #[cfg(feature = "sunfish-registry-post-reply-trace")]
                registry_reply: None,
                #[cfg(feature="sunfish-registry-tx3-wire-headers")]
                registry_wire: None,
                #[cfg(feature = "sunfish-registry-prearm-io-trace")]
                prearm_io: None,
                #[cfg(feature = "sunfish-tftp-tail-observer")]
                tftp_tail_remaining: None,
                #[cfg(feature = "sunfish-tftp-tail-observer")]
                tftp_tail_seen: false,
                #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
                first_server_check_tail: ServerCheckTail::Untracked,
                #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
                oack_peer_tail: None,
                #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
                oack_close_fifo: None,
                #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
                reserved_oack_rx: None,
                #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
                reserved_oack_ahead: None,
                #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
                reserved_writer_io: ReservedWriterIoTrace::default(),
                #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
                first_server_check_clock: None,
                #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
                server_check_rx: None,
                #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
                registration_rx: None,
                #[cfg(feature = "sunfish-grant-buffer-reads")]
                grant_reads: Default::default(),
                #[cfg(feature = "sunfish-grant-buffer-reads")]
                grant_session_write: false,
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                first_wrq_rx: None,
                #[cfg(feature = "sunfish-ipa-announcement-fifo")]
                ipa_announcement_rx: None,
                #[cfg(feature = "sunfish-ipa-ingress-work")]
                ipa_ingress_work: Default::default(),
                #[cfg(feature = "sunfish-spare-wrq-ingress")]
                wrq_work: Default::default(),
                #[cfg(feature = "sunfish-wrq-prefix-close")]
                wrq_prefix: Default::default(),
                #[cfg(feature = "sunfish-ipa-announcement-fifo")]
                track_ipa_announcement: false,
                #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
                track_first_wrq: false,
                #[cfg(feature = "sunfish-linux-mpss-glink")]
                control: None,
                #[cfg(feature = "sunfish-linux-mpss-glink")]
                open_diagnostic: None,
            },
            session,
            quarantined: true,
            session_failure: None,
        };
        this.ring.sample()?;
        this.quarantined = false;
        Ok(this)
    }

    fn check(&self, generation: u64) -> Result<(), Error> {
        if generation != self.ring.generation {
            return Err(Error::StaleGeneration);
        }
        if self.quarantined {
            return Err(Error::Quarantined);
        }
        self.session.check(generation)?;
        Ok(())
    }

    /// Fixed identity captured at attachment and rechecked by every live gate.
    pub fn identity(&self) -> u64 {
        self.ring.identity
    }

    /// Backend-specific failure detail for diagnostics; zero means unknown.
    pub fn io_diagnostic(&self) -> u32 {
        self.ring.io.diagnostic()
    }
    pub fn capacity_diagnostic(&self) -> Option<core::CapacityDiagnostic> {
        self.session.capacity_diagnostic()
    }
    pub fn session_failure_diagnostic(&self) -> Option<SessionFailureDiagnostic> {
        self.session_failure
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn linux_ipa_tx_diagnostic(&self) -> Option<core::LinuxIpaTxDiagnostic> {
        self.session.linux_ipa_tx_diagnostic()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn linux_ipa_receipt_pending(&self) -> Option<bool> {
        self.session.linux_ipa_receipt_pending()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn linux_ipa_receive_available(&self) -> u32 {
        self.session.linux_ipa_receive_available()
    }
    #[cfg(feature = "sunfish-glink-intent-denial-snapshot")]
    pub fn intent_denial_window(&self) -> Option<crate::qcom_glink_qmi::IntentDenialWindow> {
        self.session.intent_denial_window()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn intent_denial_diagnostic(&self) -> Option<crate::qcom_glink_qmi::IntentDenialDiagnostic> {
        self.session.intent_denial_diagnostic()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn receive_snapshot(&self) -> crate::qcom_glink_qmi::ReceiveSnapshot {
        self.session.receive_snapshot()
    }
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_queue_snapshot(&self) -> crate::qcom_glink_qmi::TftpQueueSnapshot {
        self.session.tftp_queue_snapshot()
    }
    #[cfg(feature = "sunfish-native-tftp-server-check")]
    pub fn tftp_tx_progress(&self) -> [u32; 3] { self.session.tftp_tx_progress() }
    #[cfg(feature = "sunfish-wlan-registry-ack-trace")]
    pub fn set_registry_ack_clock(&mut self, clock: fn() -> u64) {
        self.session.set_registry_ack_clock(clock);
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        { self.ring.first_server_check_clock = Some(clock); }
    }
    #[cfg(feature = "sunfish-wlan-intent-ack-turn")]
    pub fn intent_ack_summary(&self) -> (u32, Option<u64>) {
        self.session.intent_ack_summary()
    }
    #[cfg(feature = "sunfish-registry-handoff-turn")]
    pub(crate) fn registry_handoff_pair(&self, older: &core::RequestHandle, waiting: &core::RequestHandle)
        -> Option<(u16, u16, crate::qcom_qmi_router::Address, bool)>
    { self.session.registry_handoff_pair(older, waiting) }
    #[cfg(feature = "sunfish-registry-window-trace")]
    pub fn registry_window_trace(&self) -> core::RegistryWindowTrace { self.session.registry_window_trace() }
    #[cfg(feature="sunfish-elf-first-slot-turn")]
    pub(crate) fn take_elf_slot_turn(&mut self,completed:&RequestHandle,waiting:&RequestHandle)->bool {
        self.session.take_elf_slot_turn(completed,waiting)
    }
    #[cfg(feature="sunfish-elf-window-trace")]
    pub fn elf_window_trace(&self)->Option<&core::ElfWindowTrace>{self.session.elf_window_trace()}
    #[cfg(feature="sunfish-elf-tx-slots")]
    pub fn elf_tx_slots(&self)->Option<&core::ElfTxSlots>{self.session.elf_tx_slots()}
    #[cfg(feature="sunfish-post-elf-mcfg-trace")]
    pub fn post_elf_mcfg(&self)->Option<&core::PostElfMcfgTrace>{self.session.post_elf_mcfg()}
    #[cfg(feature="sunfish-post-elf-mcfg-trace")]
    pub(crate) fn post_mcfg_watches(&self,h:&core::TftpHandle)->bool{self.session.post_mcfg_watches(h)}
    #[cfg(feature="sunfish-post-elf-mcfg-trace")]
    pub(crate) fn observe_post_mcfg_dispatch(&mut self,h:&core::TftpHandle,before:Option<usize>,r:Result<crate::qcom_tftp_server_check::Reply,crate::qcom_tftp_server_check::Error>,b:&[u8]){self.session.observe_post_mcfg_dispatch(h,before,r,b)}
    #[cfg(feature="sunfish-registry-tx3-wire-headers")]
    pub fn registry_wire_headers(&self)->Option<RegistryWireTrace>{self.ring.registry_wire}
    #[cfg(feature = "sunfish-registry-post-reply-trace")]
    pub fn registry_reply_fifo(&self) -> Option<RegistryReplyFifo> {
        self.ring.registry_reply.as_ref().map(|w| w.trace)
    }
    #[cfg(feature = "sunfish-registry-prearm-io-trace")]
    pub fn registry_prearm_io(&self) -> Option<RegistryPrearmIo> { self.ring.prearm_io }

    #[cfg(feature = "sunfish-wlan-elf-queue-trace")]
    pub fn registry_queue_pressure(&self, handle: &core::RequestHandle) -> Option<core::RegistryQueuePressure> {
        self.session.registry_queue_pressure(handle)
    }
    #[cfg(feature = "sunfish-tftp-endpoint-observer")]
    pub fn tftp_endpoint_trace(&self) -> Option<core::TftpEndpointTrace> { self.session.tftp_endpoint_trace() }
    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    pub fn tftp_final_ack_trace(&self) -> Option<core::TftpFinalAckTrace> { self.session.tftp_final_ack_trace() }
    #[cfg(feature = "sunfish-tftp-oack-repeat-turn")]
    pub(crate) fn coalesce_followed_wrq(&self, handle: &core::TftpHandle) -> bool {
        self.session.coalesce_followed_wrq(handle)
    }

    #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
    pub(crate) fn oack_close_fifo(&self) -> Option<ServerCheckRxDone> {
        self.ring.oack_close_fifo.as_ref().and_then(|trace| trace.done)
    }

    #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
    pub(crate) fn reserved_oack_ahead(&self) -> Option<&OackAheadCapture> {
        self.ring.reserved_oack_ahead.as_ref()
    }
    #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
    pub(crate) fn reserved_writer_io(&self) -> ReservedWriterIoTrace { self.ring.reserved_writer_io }

    #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
    pub(crate) fn reserved_oack_rx(&self) -> Option<ServerCheckRxDone> {
        self.ring.reserved_oack_rx.as_ref().and_then(|trace| trace.done)
    }

    #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
    pub(crate) fn oack_peer_tail(&self) -> Option<OackPeerTail> {
        self.ring.oack_peer_tail
    }

    #[cfg(feature = "sunfish-tftp-oack-slot-follow")]
    pub(crate) fn oack_slot_follow(&self) -> Option<core::OackSlotFollow> {
        self.session.oack_slot_follow()
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    pub(crate) fn oack_resources(&self, transfer_port: u32) -> Option<&core::OackResources> {
        self.session.oack_resources(transfer_port)
    }

    #[cfg(feature = "sunfish-tftp-server-inventory")]
    pub(crate) fn server_check_inventory(&self) -> &[Option<core::ServerCheckOackTrace>] {
        self.session.server_check_inventory()
    }

    #[cfg(feature = "sunfish-tftp-oack-resources")]
    pub(crate) fn server_check_49(&self) -> Option<core::ServerCheckOackTrace> {
        self.session.server_check_49()
    }

    #[cfg(feature = "sunfish-post-reg-oack-turn")]
    pub(crate) fn post_reg_oack_turns(&self) -> &core::PostRegOackTurns {
        self.session.post_reg_oack_turns()
    }

    #[cfg(feature = "sunfish-first-oack-after-ipa-publish")]
    pub(crate) fn first_oack_eligibility(&self) -> core::FirstOackEligibility {
        self.session.first_oack_eligibility()
    }

    #[cfg(feature = "sunfish-wlan-tftp-oack-compare")]
    pub(crate) fn server_check_oacks(&self) -> [Option<core::ServerCheckOackTrace>; 2] {
        self.session.server_check_oacks()
    }
    #[cfg(feature = "sunfish-first-data-ack-elf-trace")]
    pub(crate) fn original_tftp_write_trace(&self) -> Option<core::ElfServerCheckTrace> {
        self.session.original_tftp_write_trace()
    }
    #[cfg(feature = "sunfish-original-final-ack-turn")]
    pub(crate) fn original_final_ack_queued(&self) -> bool {
        self.session.original_final_ack_queued()
    }
    #[cfg(feature = "sunfish-wlan-first-wrq-window")]
    pub(crate) fn first_wrq_window(&self) -> core::FirstWrqWindow {
        self.session.first_wrq_window()
    }
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    pub fn mcfg_trace(&self) -> (Option<core::TftpEndpointTrace>, Option<core::McfgDataTrace>) {
        self.session.mcfg_trace()
    }
    #[cfg(feature = "sunfish-tftp-mcfg-trace")]
    pub(crate) fn observe_mcfg_oack(&mut self, route: core::TftpReplyRoute) {
        self.session.observe_mcfg_oack(route);
    }
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub fn tftp_missing_reply_trace(&self) -> Option<core::TftpEndpointTrace> { self.session.tftp_missing_reply_trace() }
    #[cfg(feature = "sunfish-tftp-missing-temp-replies")]
    pub(crate) fn observe_tftp_missing_reply(&mut self, route: core::TftpReplyRoute) { self.session.observe_tftp_missing_reply(route); }

    #[cfg(feature = "sunfish-tftp-final-ack-observer")]
    pub(crate) fn observe_tftp_final_ack(&mut self, route: core::TftpReplyRoute, ack: &[u8]) {
        self.session.observe_tftp_final_ack(route, ack);
    }

    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
    pub(crate) fn observe_elf_server_check_coalesced(&mut self, handle: &core::TftpHandle) {
        self.session.observe_elf_server_check_coalesced(handle);
    }

    #[cfg(feature = "sunfish-wlan-elf-server-check-timing")]
    pub(crate) fn observe_elf_server_check_dispatch_start(&mut self, handle: &core::TftpHandle) {
        self.session.observe_elf_server_check_dispatch_start(handle);
    }

    #[cfg(feature = "sunfish-wlan-elf-server-check-trace")]
    pub(crate) fn observe_elf_server_check_dispatch(
        &mut self,
        handle: &core::TftpHandle,
        route: core::TftpReplyRoute,
        server_check_wrq: bool,
        dispatch: Result<crate::qcom_tftp_server_check::Reply, crate::qcom_tftp_server_check::Error>,
        response: &[u8],
    ) {
        self.session.observe_elf_server_check_dispatch(handle, route, server_check_wrq, dispatch, response);
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-repeat")]
    pub(crate) fn queue_traced_tftp_response(
        &mut self,
        generation: u64,
        handle: &core::TftpHandle,
        route: core::TftpReplyRoute,
        packet: &[u8],
    ) -> Result<(), Error> {
        self.gate(generation)?;
        let result = self.session.queue_traced_tftp_response(generation, handle, route, packet,
            #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
            matches!(self.ring.first_server_check_tail, ServerCheckTail::Consumed(_)),
        );
        #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
        if result.is_err() {
            let block = self.session.tftp_queue_block();
            if block.check != 0 {
                #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
                let full = (block.check == 2).then(|| TftpFullQueue {
                    at: self.ring.first_server_check_clock.map(|clock| clock()),
                    slots: self.session.tftp_tx_slots(),
                    first_free_mask: 0,
                    first_free_at: None,
                });
                if let Some(request) = self.registration_tftp_request_mut(*handle) {
                    request.queue_block = block;
                    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
                    { request.full_queue = request.full_queue.or(full); }
                }
                self.update_registration_tftp_originals(*handle, |original| {
                    original.queue_block = block;
                    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
                    { original.full_queue = original.full_queue.or(full); }
                });
            }
        }
        Ok(result?)
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry")]
    pub(crate) fn cancel_closed_tftp_retry(&mut self, handle: &core::TftpHandle) -> bool {
        self.session.cancel_closed_tftp_retry(handle)
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
    pub(crate) fn observe_retry_turn(&mut self) {
        self.session.observe_retry_turn();
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-retry-trace")]
    pub(crate) fn observe_retry_poll_end(&mut self) {
        self.session.observe_retry_poll_end();
    }

    #[cfg(feature = "sunfish-glink-tftp-fairness-experiment")]
    pub fn tftp_schedule_trace(&self) -> core::TftpScheduleTrace { self.session.tftp_schedule_trace() }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn outbound_intent_diagnostic(&self) -> (u32, u32) {
        self.session.outbound_intent_diagnostic()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn outbound_intent_attempt_diagnostic(&self) -> (u32, u32) {
        self.session.outbound_intent_attempt_diagnostic()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_diagnostic(&self) -> (u32, u32, u32) {
        self.session.intent_gate_diagnostic()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_combinations(&self) -> (u32, u32) {
        self.session.intent_gate_combinations()
    }
    #[cfg(feature = "sunfish-glink-intent-request-experiment")]
    pub fn intent_gate_queue_snapshot(&self) -> (u32, u32, u32) {
        self.session.intent_gate_queue_snapshot()
    }
    #[cfg(feature = "sunfish-linux-ipa-qmi")]
    pub fn routing_diagnostic(&self) -> Option<crate::qcom_glink_qmi::RoutingDiagnostic> {
        self.session.routing_diagnostic()
    }
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    pub fn open_diagnostic(&self) -> Option<OpenDiagnostic> {
        self.ring.open_diagnostic
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn crash_evidence(&self) -> Option<crate::qcom_modem_crash::Evidence> {
        self.ring.io.crash_evidence()
    }

    #[cfg_attr(feature="sunfish-wrq-post-return-callers",track_caller)]
    fn gate(&mut self, generation: u64) -> Result<(), Error> {
        self.check(generation)?;
        self.quarantined = true; // Remains set on IO failure or unwinding IO.
        self.ring.sample()?;
        self.quarantined = false;
        Ok(())
    }

    pub fn register(&mut self, generation: u64, service: Service, port: u32) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.register(generation, service, port)?;
        Ok(())
    }
    pub fn start_control(&mut self, generation: u64) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.start_control(generation)?;
        Ok(())
    }
    pub fn enable_wlfw_client(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.enable_wlfw_client(generation, local_port)?;
        Ok(())
    }
    /// Reserve WLFW while holding indication registration behind the live
    /// ICNSS power-sequencer permit.
    pub fn enable_wlfw_client_power_gated(
        &mut self,
        generation: u64,
        local_port: u32,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .enable_wlfw_client_power_gated(generation, local_port)?;
        Ok(())
    }
    pub fn authorize_wlfw_power(
        &mut self,
        generation: u64,
        permit: crate::qcom_wlan_power::PowerReadyPermit,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.authorize_wlfw_power(generation, permit)?;
        Ok(())
    }
    /// Reserve the one-shot IPA client endpoint. This does not send QMI or
    /// initialize the AP-side IPA provider.
    pub fn enable_ipa_client(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.enable_ipa_client(generation, local_port)?;
        Ok(())
    }
    pub fn enable_servreg_notif_client(
        &mut self,
        generation: u64,
        local_port: u32,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .enable_servreg_notif_client(generation, local_port)?;
        Ok(())
    }
    pub fn servreg_notif_state(&self) -> Option<crate::qcom_servreg_notif_client::State> {
        self.session.servreg_notif_state()
    }
    pub fn servreg_notif_outcome(&self) -> Option<crate::qcom_servreg_notif_client::Outcome> {
        self.session.servreg_notif_outcome()
    }
    pub fn ipa_state(&self) -> Option<crate::qcom_ipa_client::State> {
        self.session.ipa_state()
    }
    pub fn ipa_outcome(&self) -> Option<crate::qcom_ipa_client::ResponseOutcome> {
        self.session.ipa_outcome()
    }
    /// Revalidate the live transport and queue one source-profiled IPA
    /// request. The caller must supply runtime fields from a trusted provider.
    pub fn begin_ipa_init_modem_driver(
        &mut self,
        generation: u64,
        fields: &crate::qcom_ipa::InitRequestFields,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .begin_ipa_init_modem_driver(generation, fields)?;
        Ok(())
    }
    pub fn begin_servreg_notif_query(&mut self, generation: u64) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.begin_servreg_notif_query(generation)?;
        Ok(())
    }
    pub fn begin_servreg_notif_listener(&mut self, generation: u64) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.begin_servreg_notif_listener(generation)?;
        Ok(())
    }
    pub fn begin_servreg_notif_restart_pd(&mut self, generation: u64) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.begin_servreg_notif_restart_pd(generation)?;
        Ok(())
    }
    pub fn wlfw_state(&self) -> Option<crate::qcom_wlfw_client::State> {
        self.session.wlfw_state()
    }
    pub fn wlfw_outcome(&self) -> Option<crate::qcom_wlfw_client::RegistrationOutcome> {
        self.session.wlfw_outcome()
    }
    pub fn wlfw_msa_info_outcome(&self) -> Option<crate::qcom_wlfw_client::MsaInfoOutcome> {
        self.session.wlfw_msa_info_outcome()
    }
    pub fn wlfw_capability_summary(&self) -> Option<crate::qcom_wlfw_client::CapabilitySummary> {
        self.session.wlfw_capability_summary()
    }
    pub fn wlfw_dynamic_feature_mask_done(&self) -> bool {
        self.session.wlfw_dynamic_feature_mask_done()
    }
    pub fn wlfw_last_indication(&self) -> Option<crate::qcom_wlfw_client::IndicationOutcome> {
        self.session.wlfw_last_indication()
    }
    pub fn wlfw_firmware_ready_seen(&self) -> bool {
        self.session.wlfw_firmware_ready_seen()
    }
    pub fn begin_wlfw_msa_info(
        &mut self,
        generation: u64,
        reservation: MsaReservation,
    ) -> Result<(), Error> {
        if reservation.generation() != generation {
            return Err(Error::StaleGeneration);
        }
        if reservation.identity() != self.ring.identity {
            return Err(Error::IdentityChanged);
        }
        self.gate(generation)?;
        self.session.begin_wlfw_msa_info(generation, reservation)?;
        Ok(())
    }
    pub fn begin_wlfw_msa_ready(
        &mut self,
        generation: u64,
        permit: &mut crate::qcom_msa::MsaReadyPermit,
    ) -> Result<(), Error> {
        if permit.generation() != generation {
            return Err(Error::StaleGeneration);
        }
        if permit.identity() != self.ring.identity {
            return Err(Error::IdentityChanged);
        }
        self.gate(generation)?;
        self.session.begin_wlfw_msa_ready(generation, permit)?;
        Ok(())
    }
    pub fn begin_wlfw_capability_request(&mut self, generation: u64) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.begin_wlfw_capability_request(generation)?;
        Ok(())
    }
    pub fn complete_wlfw_dynamic_feature_mask_timeout(
        &mut self,
        generation: u64,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .complete_wlfw_dynamic_feature_mask_timeout(generation)?;
        Ok(())
    }
    pub fn begin_wlfw_bdf_download(
        &mut self,
        generation: u64,
        image: &crate::qcom_wlfw_bdf::VerifiedBdfImage<'_>,
    ) -> Result<(), Error> {
        if image.generation() != generation {
            return Err(Error::StaleGeneration);
        }
        if image.identity() != self.ring.identity {
            return Err(Error::IdentityChanged);
        }
        self.gate(generation)?;
        self.session.begin_wlfw_bdf_download(generation, image)?;
        Ok(())
    }
    pub fn continue_wlfw_bdf_download(
        &mut self,
        generation: u64,
        image: &crate::qcom_wlfw_bdf::VerifiedBdfImage<'_>,
    ) -> Result<(), Error> {
        if image.generation() != generation {
            return Err(Error::StaleGeneration);
        }
        if image.identity() != self.ring.identity {
            return Err(Error::IdentityChanged);
        }
        self.gate(generation)?;
        self.session.continue_wlfw_bdf_download(generation, image)?;
        Ok(())
    }
    pub fn begin_wlfw_empty_cal_report(&mut self, generation: u64) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.begin_wlfw_empty_cal_report(generation)?;
        Ok(())
    }
    pub fn set_service_available(
        &mut self,
        generation: u64,
        service: Service,
        ready: bool,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .set_service_available(generation, service, ready)?;
        Ok(())
    }
    pub fn prepare_service_available(
        &mut self,
        generation: u64,
        service: Service,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .prepare_service_available(generation, service)?;
        Ok(())
    }
    pub fn prepare_tftp_observer(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.prepare_tftp_observer(generation, local_port)?;
        Ok(())
    }
    pub fn prepare_tftp_server(&mut self, generation: u64, local_port: u32) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.prepare_tftp_server(generation, local_port)?;
        Ok(())
    }
    pub fn tftp_server_prepared(&self, generation: u64, local_port: u32) -> bool {
        self.session.tftp_server_prepared(generation, local_port)
    }
    pub fn control_status(&self) -> crate::qcom_qrtr_control::Status {
        self.session.control_status()
    }
    pub fn last_completion_error(&self) -> Option<core::CompletionDiagnostic> {
        self.session.last_completion_error()
    }
    pub fn last_credit_diagnostic(&self) -> Option<core::CreditDiagnostic> {
        self.session.last_credit_diagnostic()
    }
    pub fn client_gone_diagnostic(&self) -> Option<core::ClientGoneDiagnostic> {
        self.session.client_gone_diagnostic()
    }
    #[cfg(feature = "sunfish-qrtr-deferred-requests")]
    pub fn deferred_close_diagnostic(&self) -> Option<core::DeferredCloseDiagnostic> {
        self.session.deferred_close_diagnostic()
    }
    #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]
    pub(crate) fn commit_tftp_close(&mut self, generation: u64, peer: crate::qcom_qmi_router::Address) -> Result<crate::qcom_qrtr_control::Event, Error> {
        if generation != self.ring.generation { return Err(Error::StaleGeneration); }
        if self.quarantined { return Err(Error::Quarantined); }
        self.quarantined = true;
        self.ring.sample()?;
        #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
        let occupied_before = self.ring.registration_reply.as_ref()
            .map_or(0, |_| self.session.tftp_tx_occupied());
        let event = self.session.commit_tftp_close(generation, peer)?;
        #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
        self.observe_tftp_slot_release(occupied_before);
        self.quarantined = false;
        Ok(event)
    }
    #[cfg(all(test, feature = "sunfish-tftp-close-handoff-prototype"))]
    pub(crate) fn test_fail_close_stage(&mut self, stage: u8) { self.session.test_fail_close_stage(stage); }
    #[cfg(feature = "sunfish-tftp-deferred-close-experiment")]
    pub fn set_tftp_owner_busy(&mut self, generation: u64, busy: bool) -> Result<(), Error> {
        self.check(generation)?;
        self.session.set_tftp_owner_busy(generation, busy).map_err(Into::into)
    }
    #[cfg(feature = "sunfish-tftp-peer-close-experiment")]
    pub fn set_tftp_owner_peers(&mut self, generation: u64, peers: [Option<crate::qcom_qmi_router::Address>; core::TFTP_OWNER_PEER_SLOTS]) -> Result<(), Error> {
        self.check(generation)?;
        self.session.set_tftp_owner_peers(generation, peers).map_err(Into::into)
    }
    /// One fresh live gate for callback-free bookkeeping on host-owned packets.
    /// No successful sample is carried into the next FIFO operation.
    #[cfg(feature = "sunfish-tftp-peer-metadata-batch")]
    pub(crate) fn refresh_tftp_peer_snapshot(
        &mut self,
        generation: u64,
        held: &[Option<TftpHandle>],
        retained: &[(Option<crate::qcom_qmi_router::Address>, u8, bool); core::TFTP_OWNER_PEER_SLOTS],
    ) -> Result<[(Option<crate::qcom_qmi_router::Address>, u8); core::TFTP_OWNER_PEER_SLOTS], Error> {
        self.gate(generation)?;
        let mut peers = [None; core::TFTP_OWNER_PEER_SLOTS];
        let mut firmware = [None; core::TFTP_OWNER_PEER_SLOTS];
        let mut snapshot = [(None, 0); core::TFTP_OWNER_PEER_SLOTS];
        let mut count = 0;
        let mut firmware_count = 0;
        for handle in held.iter().flatten() {
            // Each opaque handle is still checked against the current Session
            // owner, generation and held slot. Bytes were copied at ingress.
            let (route, packet) = self.session.tftp_datagram(handle)?;
            *peers.get_mut(count).ok_or(core::Error::NoCapacity)? = Some(route.destination);
            snapshot[count] = (Some(route.destination), 1);
            count += 1;
            if (route.source.port == crate::qcom_tftp_server_check::SERVICE_PORT
                    && crate::qcom_tftp::decode_read_request(packet).is_ok())
                || (0x5100..=0x51ff).contains(&route.source.port) {
                *firmware.get_mut(firmware_count).ok_or(core::Error::NoCapacity)? = Some(route.destination);
                firmware_count += 1;
            }
        }
        for &(peer, kind, is_firmware) in retained {
            if let Some(peer) = peer {
                *peers.get_mut(count).ok_or(core::Error::NoCapacity)? = Some(peer);
                snapshot[count] = (Some(peer), kind);
                count += 1;
                if is_firmware {
                    *firmware.get_mut(firmware_count).ok_or(core::Error::NoCapacity)? = Some(peer);
                    firmware_count += 1;
                }
            }
        }
        // No callbacks, transport I/O or external state changes occur between
        // the gate and these infallible-for-the-checked-generation setters.
        self.session.set_tftp_owner_peers(generation, peers)?;
        self.session.set_tftp_firmware_peers(generation, firmware)?;
        Ok(snapshot)
    }
    #[cfg(all(test, feature = "sunfish-tftp-peer-metadata-batch"))]
    pub(crate) fn test_peer_metadata(&self) -> ([Option<crate::qcom_qmi_router::Address>; core::TFTP_OWNER_PEER_SLOTS], [Option<crate::qcom_qmi_router::Address>; core::TFTP_OWNER_PEER_SLOTS]) {
        self.session.test_peer_metadata()
    }
    #[cfg(feature = "sunfish-tftp-close-owner-fence")]
    pub fn set_tftp_firmware_peers(&mut self, generation: u64, peers: [Option<crate::qcom_qmi_router::Address>; core::TFTP_OWNER_PEER_SLOTS]) -> Result<(), Error> {
        self.revalidate(generation)?;
        self.session.set_tftp_firmware_peers(generation, peers).map_err(Into::into)
    }
    #[cfg(feature = "sunfish-tftp-wlan-close-handoff")]
    pub(crate) fn commit_firmware_close(&mut self, generation: u64, peer: crate::qcom_qmi_router::Address) -> Result<(), Error> {
        if generation != self.ring.generation { return Err(Error::StaleGeneration); }
        if self.quarantined { return Err(Error::Quarantined); }
        self.quarantined = true;
        self.ring.sample()?;
        self.session.commit_firmware_close(generation, peer)?;
        self.quarantined = false;
        Ok(())
    }
    pub fn retired_rmtfs_peer_count(&self) -> usize {
        self.session.retired_rmtfs_peer_count()
    }
    pub fn remote_server(&self, index: usize) -> Option<crate::qcom_qrtr_control::Server> {
        self.session.remote_server(index)
    }
    pub fn wlfw_endpoint(
        &mut self,
        generation: u64,
    ) -> Result<Option<crate::qcom_qmi_router::Address>, Error> {
        self.gate(generation)?;
        Ok(self.session.wlfw_endpoint())
    }
    pub fn offer_receive(&mut self, generation: u64) -> Result<u32, Error> {
        self.gate(generation)?;
        Ok(self.session.offer_receive(generation)?)
    }
    pub fn request(
        &mut self,
        generation: u64,
        handle: &RequestHandle,
    ) -> Result<(Context, Request<'_>), Error> {
        self.gate(generation)?;
        Ok(self.session.request(handle)?)
    }
    pub fn request_qmi(
        &mut self,
        generation: u64,
        handle: &RequestHandle,
    ) -> Result<(Context, &[u8]), Error> {
        self.gate(generation)?;
        Ok(self.session.request_qmi(handle)?)
    }
    pub fn tftp_datagram(
        &mut self,
        generation: u64,
        handle: &TftpHandle,
    ) -> Result<(crate::qcom_glink_qmi::TftpReplyRoute, &[u8]), Error> {
        self.gate(generation)?;
        Ok(self.session.tftp_datagram(handle)?)
    }
    pub fn finish_tftp_request(
        &mut self,
        generation: u64,
        handle: &TftpHandle,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.finish_tftp_request(handle)?;
        Ok(())
    }
    pub fn queue_reply(
        &mut self,
        generation: u64,
        handle: &RequestHandle,
        reply: Reply<'_>,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.queue_reply(handle, reply)?;
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        if self.ring.prearm_io.is_none() && self.session.registry_prearm_queue_matches(handle) {
            self.ring.io.registry_prearm_timing(true);
            self.ring.prearm_io = Some(RegistryPrearmIo { queued: self.ring.io.registry_prearm_totals(),
                queued_at: self.ring.first_server_check_clock.map(|now|now()), ..RegistryPrearmIo::default() });
        }
        Ok(())
    }
    #[cfg(feature="sunfish-elf-third-reply")]
    pub(crate) fn queue_elf_third_reply(
        &mut self,
        generation: u64,
        handle: &RequestHandle,
        reply: Reply<'_>,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session.queue_elf_third_reply(handle, reply)?;
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        if self.ring.prearm_io.is_none() && self.session.registry_prearm_queue_matches(handle) {
            self.ring.io.registry_prearm_timing(true);
            self.ring.prearm_io = Some(RegistryPrearmIo { queued: self.ring.io.registry_prearm_totals(),
                queued_at: self.ring.first_server_check_clock.map(|now|now()), ..RegistryPrearmIo::default() });
        }
        Ok(())
    }
    pub fn queue_ipa_service_reply(
        &mut self,
        generation: u64,
        handle: &RequestHandle,
        response: &crate::qcom_ipa_service::PreparedResponse,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .queue_ipa_service_reply(generation, handle, response)?;
        Ok(())
    }
    pub fn take_ipa_service_response_completion(
        &mut self,
        generation: u64,
    ) -> Result<Option<u64>, Error> {
        self.gate(generation)?;
        Ok(self
            .session
            .take_ipa_service_response_completion(generation)?)
    }
    pub fn queue_ipa_master_init_indication(
        &mut self,
        generation: u64,
        indication: &crate::qcom_ipa_service::PreparedIndication,
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .queue_ipa_master_init_indication(generation, indication)?;
        Ok(())
    }
    pub fn take_ipa_indication_completion(
        &mut self,
        generation: u64,
    ) -> Result<Option<u64>, Error> {
        self.gate(generation)?;
        Ok(self.session.take_ipa_indication_completion(generation)?)
    }
    pub fn queue_tftp_response(
        &mut self,
        generation: u64,
        route: crate::qcom_glink_qmi::TftpReplyRoute,
        packet: &[u8],
    ) -> Result<(), Error> {
        self.gate(generation)?;
        self.session
            .queue_tftp_response(generation, route, packet)?;
        Ok(())
    }
    pub fn in_flight(&self) -> usize {
        self.session.in_flight()
    }
    #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
    pub(crate) fn observe_elf_reply(&mut self, handle: &RequestHandle) {
        self.session.observe_elf_reply(handle);
    }

    #[cfg(feature = "sunfish-wlan-elf-exchange-trace")]
    pub(crate) fn elf_exchange(&self) -> Option<core::ElfExchangeTrace> {
        self.session.elf_exchange()
    }

    #[cfg(feature = "sunfish-wlan-first-server-check-trace")]
    pub(crate) fn first_server_check(&self) -> Option<core::ElfServerCheckTrace> {
        self.session.first_server_check()
    }

    /// Read-only publication evidence; does not poll the transport.
    #[cfg(feature = "sunfish-linux-rmtfs")]
    #[cfg(feature="sunfish-elf-queued-handoff")]
    pub(crate) fn unpublished_registry_reply(&self,h:&core::RequestHandle)->bool{self.session.unpublished_registry_reply(h)}
    pub fn reply_fully_published(&self, handle: &RequestHandle) -> bool {
        self.session.reply_fully_published(handle)
    }
    pub fn configuration(&self) -> crate::qcom_glink_qmi::Config {
        self.session.configuration()
    }
    pub fn endpoint(&self, service: Service) -> Option<crate::qcom_qmi_router::Endpoint> {
        self.session.endpoint(service)
    }

    /// Read-only lifecycle/cursor check for the serialized IRQ owner.
    #[cfg_attr(feature="sunfish-wrq-post-return-callers",track_caller)]
    pub fn revalidate(&mut self, generation: u64) -> Result<(), Error> {
        self.gate(generation)
    }

    /// Latch a local lifecycle fault without publishing or releasing anything.
    pub fn suspend(&mut self) {
        self.quarantined = true;
    }

    /// At most one complete record per call. Partial records retain RX tail.
    /// Rejected records halt this adapter with RX tail held, even if the core
    /// already copied a final fragment. No queued RX_DONE can then be published.
    pub fn poll_receive(&mut self, generation: u64) -> Result<Receive, Error> {
        self.check(generation)?;
        #[cfg(feature = "sunfish-ipa-registration-reply-path")]
        if let Some(trace) = self.ring.registration_reply.as_mut().filter(|trace| !trace.finished) {
            trace.polls = trace.polls.saturating_add(1);
            if let Some(now) = self.ring.first_server_check_clock.map(|clock| clock()) {
                if let Some(gap) = trace.last_poll_at.and_then(|last| now.checked_sub(last)) {
                    trace.max_poll_gap = trace.max_poll_gap.max(gap);
                }
                trace.last_poll_at = Some(now);
            }
        }
        self.quarantined = true;
        #[cfg(feature = "sunfish-qrtr-deferred-requests")]
        if self.session.deferred_ready() {
            self.ring.sample()?;
            match self.session.retry_deferred(generation) {
                Ok(Some(event)) => { self.quarantined = false; return Ok(Receive::Session(event)); },
                Ok(None) => {},
                Err(error) => {
                    self.session_failure = Some(SessionFailureDiagnostic { stage: 1, command: u32::MAX, parameter: u32::MAX });
                    return Err(error.into());
                },
            }
        }
        #[cfg(feature = "sunfish-linux-mpss-glink")]
        if let Some(first) = self.ring.control.as_ref().and_then(|c| c.early[0]) {
            self.ring.sample()?;
            let event = match self.session.ingest(generation, &first.bytes[..first.len]) {
                Ok(event) => event,
                Err(error) => {
                    self.session_failure = Some(SessionFailureDiagnostic { stage: 2,
                        command: u32::from(u16::from_le_bytes([first.bytes[0], first.bytes[1]])),
                        parameter: u32::from_le_bytes(first.bytes[4..8].try_into().unwrap()) });
                    if error == core::Error::NoCapacity { self.session.record_capacity(1, Some(&first.bytes[..first.len])); }
                    return Err(error.into());
                }
            };
            #[cfg(feature = "sunfish-grant-buffer-reads")]
            if first.bytes[0] == 9 { self.ring.grant_reads.release(word(&first.bytes, 4)); }
            let queue = &mut self.ring.control.as_mut().unwrap().early;
            queue.rotate_left(1);
            queue[7] = None;
            self.quarantined = false;
            return Ok(Receive::Session(event));
        }
        #[cfg(feature = "sunfish-spare-wrq-ingress")]
        { self.ring.wrq_work.rx_start = self.ring.wrq_work_clock(); }
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        let ingress_start = self.ring.ingress_clock();
        let size = match self.ring.peek()? {
            Ok(size) => size,
            Err(state) => {
                #[cfg(feature = "sunfish-ipa-ingress-work")]
                if let (Some(start), Some(end)) = (ingress_start, self.ring.ingress_clock()) {
                    let (kind, size, detail) = match state {
                        Receive::Pending { available, needed } => (9, available as u16, needed as u32),
                        _ => (8, 0, 0),
                    };
                    self.ring.ipa_ingress_work.push(IpaIngressRecord {
                        start, end, kind, cmd: 0xff, detail, size, position: None,
            #[cfg(feature = "sunfish-wrq-stage-costs")] protocol: None,
        });
                }
                self.quarantined = false;
                return Ok(state);
            }
        };
        #[cfg(feature = "sunfish-wrq-return-pending")]
        if let Some(trace) = self.ring.wrq_return_pending.as_mut()
            .filter(|trace| trace.follows(self.ring.words[2]) && trace.first.is_none())
        {
            trace.first_at = self.ring.first_server_check_clock.map(|clock| clock());
            trace.first = Some(WrqPrefixHeader::read(&self.ring.record[..size]));
        }
        #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
        if let (Some(length), Some(window), Some(capture)) =
            (self.ring.rx_bytes, self.ring.reserved_oack_rx.as_ref(), self.ring.reserved_oack_ahead.as_mut())
            && window.done.is_none()
        {
            let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
            if let Some(position) = window.head.checked_sub(unread as u64) {
                let header = RegistrationHeader::read(&self.ring.record[..size], false,
                    self.ring.first_server_check_clock.map(|clock| clock()));
                capture.record(position, header, &self.ring.record[..size]);
            }
        }
        #[cfg(feature = "sunfish-linux-mpss-glink")]
        if self.ring.record[0] == 2 && self.ring.control.is_some() {
            let channels = &mut self.ring.control.as_mut().unwrap().channels;
            if let Err(error) = channels.open(&self.ring.record[..size], true) {
                self.ring.open_diagnostic = Some(channels.open_diagnostic(&self.ring.record[..size], 2));
                return Err(error);
            }
            #[cfg(feature = "sunfish-grant-rx-open")]
            if size == 16 && word(&self.ring.record, 4) & 0xffff == 3
                && self.ring.grant_receive.open.is_none() && self.ring.first_wrq_rx.is_some()
                && self.session.original_1f_close_count() == Some(0)
                && let (Some(length), Some(history)) = (self.ring.rx_bytes, self.ring.registration_rx.as_ref())
            {
                let bytes = &self.ring.record;
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                self.ring.grant_receive.byte_bounds = history.bounds(unread, 1);
                self.ring.grant_receive.open_bounds = history.bounds(unread, size);
                self.ring.grant_receive.open = Some((u16::from_le_bytes([bytes[2], bytes[3]]),
                    u16::from_be_bytes([bytes[8], bytes[9]])));
            }
            #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
            self.ring.capture_registration_rx(size, false, &Receive::Control);
            #[cfg(feature="sunfish-registry-tx3-wire-headers")]
            self.ring.capture_registry_wire_rx(size,false);
            #[cfg(feature = "sunfish-tftp-first-oack-headers")]
            self.ring.capture_first_oack_rx(size, false);
            self.consume_received(size, false)?;
            self.quarantined = false;
            return Ok(Receive::Control);
        }
        #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
        let reserved_before = self.session.reserved_oack().map(|(oack, trace)| (oack, trace))
            .filter(|(oack, _)| oack.peer.port == 0x49 && oack.published_at.is_some());
        let notification = self.ring.record[0] == 13;
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        let before = self.ring.server_check_rx.as_ref()
            .filter(|trace| trace.done.is_none())
            .and_then(|_| self.session.first_server_check());
        #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
        let awaiting_registration = self.session.ipa_registration_trace().is_none();
        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
        let completing_oack = (size == 8 && matches!(self.ring.record[0], 6 | 14))
            .then(|| self.session.published_oack_request(word(&self.ring.record, 4))).flatten();
        #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
        let occupied_before = self.ring.registration_reply.as_ref()
            .map_or(0, |_| self.session.tftp_tx_occupied());
        #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
        let awaiting_close = self.ring.reg_close.as_ref().is_some_and(|t| !t.already_closed && t.end.is_none())
            && self.session.original_1f_close_count() == Some(0);
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        let first_1f_open = self.ring.init_close.close_end.is_none()
            && self.session.original_1f_close_count() == Some(0);
        #[cfg(feature = "sunfish-ipa-announcement-fifo")]
        let awaiting_announcement = self.ring.track_ipa_announcement
            && self.session.first_ipa_announcement().is_none();
        let event = if notification {
            Receive::ReadNotification
        } else {
            match self.session.ingest(generation, &self.ring.record[..size]) {
                Ok(event) => Receive::Session(event),
                Err(error) => {
                    self.session_failure = Some(SessionFailureDiagnostic { stage: 3,
                        command: u32::from(u16::from_le_bytes([self.ring.record[0], self.ring.record[1]])),
                        parameter: u32::from_le_bytes(self.ring.record[4..8].try_into().unwrap()) });
                    if error == core::Error::NoCapacity { self.session.record_capacity(1, Some(&self.ring.record[..size])); }
                    return Err(error.into());
                }
            }
        };
        #[cfg(feature = "sunfish-first-wrq-spare-oack")]
        if matches!(event, Receive::Session(core::Event::ReceiveIntentRequest { .. }))
            && word(&self.ring.record, 4) == 108 && self.first_grant.request_108_at.is_none()
            && self.session.original_1f_close_count() == Some(0)
        {
            self.first_grant.request_108_at = self.ring.first_server_check_clock.map(|now| now());
        }
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        if self.ring.record[0] == 9 {
            self.ring.grant_reads.release(word(&self.ring.record, 4));
        } else if matches!(event, Receive::Session(core::Event::ReceiveIntentRequest { .. }))
            && self.ring.grant_reads.clock_armed && self.ring.init_close.close_end.is_none()
            && !self.ring.grant_reads.request.matched {
            let bounds = self.ring.registration_rx.as_ref().and_then(|history|
                history.bounds(ring_distance(self.ring.words[2], self.ring.words[3], self.ring.rx_bytes?), size));
            self.ring.grant_reads.capture(word(&self.ring.record, 4), self.session.grant_backing.intent,
                self.session.grant_available_ids(), bounds.and_then(|b| b.last_absent),
                bounds.and_then(|b| b.first_visible), self.ring.first_server_check_clock.map(|now| now()));
        }
        #[cfg(feature = "sunfish-oack-grant-outcome")]
        if let Receive::Session(core::Event::ReceiveIntentRequest { granted }) = event
            && word(&self.ring.record, 4) == 108
            && let Some(window) = self.ring.wrq_return_pending.as_ref().filter(|w| w.follows(self.ring.words[2]))
        {
            let backing = self.session.grant_backing;
            self.ring.grant_reads.capture_108(window.bytes - window.remaining, granted,
                backing.intent, backing.reserved_at, self.ring.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
        self.observe_tftp_slot_release(occupied_before);
        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
        if let Some(handle) = completing_oack
            && matches!(event, Receive::Session(core::Event::TransmitDone))
        {
            self.observe_registration_tftp_oack_progress(handle, 4);
        }
        #[cfg(feature = "sunfish-ipa-announcement-fifo")]
        if self.ring.track_ipa_announcement && self.session.first_ipa_announcement().is_some() {
            self.ring.track_ipa_announcement = false;
            // The normal decoder latched the first accepted modem IPA NEW_SERVER
            // during this ingest. Correlate its final record before moving the tail.
            if awaiting_announcement && matches!(event, Receive::Session(core::Event::QrtrControl(_)))
                && matches!(self.ring.record[0], 9 | 12) && word(&self.ring.record, 12) == 0
                && let Some(length) = self.ring.rx_bytes
                && let Some(history) = self.ring.registration_rx.as_ref()
            {
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                self.ring.ipa_announcement_rx = history.bounds(unread, size);
            }
        }
        #[cfg(feature = "sunfish-tftp-first-oack-headers")]
        self.ring.capture_first_oack_rx(size, before.is_some_and(|trace| trace.oack.phase == 3)
            && self.session.first_server_check().is_some_and(|trace| trace.oack.phase == 4));
        #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
        if awaiting_registration && let Some(length) = self.ring.rx_bytes {
            let matched = self.session.ipa_registration_trace().is_some();
            let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
            if self.ring.registration_rx.is_none()
                && (matched || matches!(event, Receive::Session(Event::TftpRequest(_))))
            {
                // Start with the validated head at the first TFTP request.
                // This includes records already queued behind that request.
                self.ring.registration_rx = Some(ServerCheckRxWindow::new(
                    unread, self.ring.first_server_check_clock.map(|clock| clock())));
            }
            if matched && let Some(trace) = self.ring.registration_rx.as_mut() {
                // The session latched the validated registration while the
                // RX tail still pointed at the record being ingested.
                trace.finish(unread, size);
            }
        }
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        let mut closes_prefix = false;
        #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
        if self.ring.track_first_wrq && let Some(first) = self.session.original_server_check() {
            self.ring.track_first_wrq = false;
            #[cfg(feature = "sunfish-wrq-prefix-close")]
            {
                closes_prefix = self.ring.wrq_prefix.active
                    && matches!(event, Receive::Session(Event::TftpRequest(handle)) if first == (handle, true))
                    && self.session.original_1f_close_count() == Some(0);
                self.ring.wrq_prefix.active = false;
            }
            if let Receive::Session(Event::TftpRequest(handle)) = event
                && first == (handle, true)
                && matches!(self.ring.record[0], 9 | 12) && word(&self.ring.record, 12) == 0
                && let Some(length) = self.ring.rx_bytes
                && let Some(window) = self.ring.registration_rx.as_ref()
            {
                // The normal decoder accepted this datagram. Locate the end
                // of its final GLINK record before advancing the host RX tail.
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                self.ring.first_wrq_rx = window.bounds(unread, size);
            }
        }
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        if let Some(before) = before
            && let Some(after) = self.session.first_server_check()
            && let Some(trace) = self.ring.server_check_rx.as_mut()
        {
            if before.timing.closes == 0 && after.timing.closes != 0 {
                trace.close_head = Some(trace.head);
            }
            if before.oack.phase == 3 && after.oack.phase == 4
                && let Some(length) = self.ring.rx_bytes
            {
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                trace.finish(unread, size);
            }
        }
        #[cfg(feature="sunfish-registry-tx3-wire-headers")]
        {
            let matching=matches!(&event,Receive::Session(e) if self.session.registry_predecessor_completed_by(e));
            self.ring.capture_registry_wire_rx(size,matching);
        }
        #[cfg(feature = "sunfish-registry-post-reply-trace")]
        if let Receive::Session(ref completed) = event
            && self.session.registry_predecessor_completed_by(completed)
            && let Some(window) = self.ring.registry_reply.as_mut()
            && let Some(length) = self.ring.rx_bytes
        {
            // Only the normal decoder's exact opaque request receipt freezes
            // the window, before advancing the RX tail through that record.
            window.finish(ring_distance(self.ring.words[2], self.ring.words[3], length), size);
        }
        #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
        self.ring.capture_registration_rx(size,
            self.session.ipa_registration_trace().is_some_and(|reg| reg.completed_at.is_some()), &event);
        #[cfg(feature = "sunfish-ipa-registration-reply-path")]
        if let Some(trace) = self.ring.registration_reply.as_mut().filter(|trace| !trace.finished)
            && self.session.ipa_registration_trace().is_some_and(|reg| reg.completed_at.is_some())
        {
            #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
            if let Some(mut window) = self.ring.registration_reply_rx.take()
                && let Some(length) = self.ring.rx_bytes
            {
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                window.finish(unread, size);
                trace.completion_visibility = window.done;
            }
            // The peeked record is the matching completion. Stop before
            // sampling the FIFO after advancing past it.
            trace.finished = true;
        }
        #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
        if let Some((before, before_peer)) = reserved_before
            && let Some((after, after_peer)) = self.session.reserved_oack()
            && let Some(window) = self.ring.reserved_oack_rx.as_mut()
        {
            if before_peer.is_some_and(|peer| peer.first_close_at.is_none())
                && after_peer.is_some_and(|peer| peer.first_close_at.is_some())
            {
                window.close_head = Some(window.head);
            }
            if before.done_at.is_none() && after.done_at.is_some()
                && let Some(length) = self.ring.rx_bytes
            {
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
                if let Some(capture) = self.ring.reserved_oack_ahead.as_mut() {
                    capture.completion_id = Some(word(&self.ring.record[..size], 4));
                    capture.finish(window, unread, size);
                }
                window.finish(unread, size);
            }
        }
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        if self.session.server_check_49().is_some_and(|trace| trace.first_close_at.is_some())
            && let Some(trace) = self.ring.oack_peer_tail.as_mut()
        {
            trace.close();
        }
        #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
        if self.session.server_check_49().is_some_and(|trace| trace.first_close_at.is_some())
            && let Some(length) = self.ring.rx_bytes
            && let Some(trace) = self.ring.oack_close_fifo.as_mut().filter(|trace| trace.done.is_none())
        {
            let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
            trace.finish(unread, size);
        }
        // Only normal ingestion can bind this record to a FIFO offset. The
        // session increments the original WRQ close count on validated DEL_CLIENT.
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        let init_close_match = if first_1f_open && self.session.original_1f_close_count() == Some(1) {
            self.ring.rx_bytes.zip(self.ring.registration_rx.as_ref()).and_then(|(length, history)| {
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                let end = history.head.checked_sub(unread as u64)?.checked_add(size as u64)?;
                Some((end, history.bounds(unread, size)?))
            })
        } else { None };
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        let ingress = ingress_start.map(|start| {
            let position = self.ring.rx_bytes.zip(self.ring.registration_rx.as_ref())
                .and_then(|(length, history)| history.head.checked_sub(
                    ring_distance(self.ring.words[2], self.ring.words[3], length) as u64));
            let (kind, detail) = match event {
                Receive::Session(core::Event::QrtrControl(crate::qcom_qrtr_control::Event::Server { server, added, .. })) =>
                    (if added { 4 } else { 5 }, server.service),
                Receive::Session(core::Event::QrtrControl(crate::qcom_qrtr_control::Event::ClientRemoved(port))) =>
                    (0xa, port),
                _ => (0, word(&self.ring.record, 4)),
            };
            IpaIngressRecord { start, end: start, kind, detail, size: size as u16,
                cmd: self.ring.record[0], position,
            #[cfg(feature = "sunfish-wrq-stage-costs")] protocol: None,
        }
        });
        self.consume_received(size, notification)?;
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        if closes_prefix {
            self.ring.wrq_prefix.complete = true;
        } else if awaiting_announcement && !self.ring.track_ipa_announcement && self.ring.track_first_wrq {
            self.ring.wrq_prefix.active = true;
        }
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        if let Some(mut record) = ingress && let Some(clock) = self.ring.first_server_check_clock {
            record.end = clock();
            if awaiting_announcement && self.ring.ipa_announcement_rx.is_some() {
                self.ring.ipa_ingress_work.target = record.position;
            }
            self.ring.ipa_ingress_work.push(record);
        }
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        if let Some((end, bounds)) = init_close_match {
            self.ring.init_close.close_end = Some(end);
            self.ring.init_close.bounds = Some(bounds);
        }
        #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
        if awaiting_close && self.session.original_1f_close_count() == Some(1)
            && let Some(trace) = self.ring.reg_close.as_mut()
        {
            trace.end = Some(trace.consumed);
            trace.record_bytes = Some(size as u32);
        }
        #[cfg(feature = "sunfish-grant-rx-open")]
        if let Receive::Session(core::Event::ReceiveIntentRequest { granted }) = event {
            self.ring.grant_receive.awaiting_ack = granted && word(&self.ring.record, 4) == 52;
        }
        self.quarantined = false;
        Ok(event)
    }

    fn consume_received(&mut self, size: usize, notification: bool) -> Result<(), Error> {
        #[cfg(feature = "sunfish-oack-grant-ack-turn")]
        if self.ring.grant_reads.pending_active(self.ring.words[2]) {
            self.ring.grant_reads.check_pending(self.ring.words[2], &self.ring.record[..size],
                self.ring.first_server_check_clock.map(|clock| clock()));
            #[cfg(feature = "sunfish-oack-retry-close")]
            self.ring.grant_reads.pending_repeat_identity(self.ring.words[2],
                self.session.first_tftp_repeat_on_intent(word(&self.ring.record, 4)));
        }
        #[cfg(feature = "sunfish-wrq-return-headers")]
        if let Some(trace) = self.ring.wrq_return_pending.as_mut().filter(|t| t.follows(self.ring.words[2])) {
            trace.record_header(&self.ring.record[..size], self.ring.first_server_check_clock.map(|clock| clock()));
        }
        #[cfg(feature = "sunfish-first-oack-yield-rx")]
        let start = self.ring.words[2];
        self.ring.consume(size, notification)?;
        #[cfg(feature = "sunfish-spare-wrq-ingress")]
        if let (Some(start), Some(clock)) = (self.ring.wrq_work.rx_start.take(), self.ring.first_server_check_clock) {
            self.ring.wrq_work.record(start, clock(), 0, &self.ring.record[..size]);
        }
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        if self.ring.wrq_prefix.active {
            self.ring.wrq_prefix.consumed(&self.ring.record[..size]);
        }
        #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
        if let Some(trace) = self.ring.reg_close.as_mut().filter(|t| t.end.is_none() && !t.already_closed) {
            trace.consumed = trace.consumed.saturating_add(size as u32);
        }
        #[cfg(feature = "sunfish-first-oack-yield-rx")]
        if self.first_grant.window.remaining != 0 {
            let peer = self.session.server_check_oacks()[0].map(|t| [t.peer.node, t.peer.port]);
            self.first_grant.window.consumed(start, self.ring.words[2], &self.ring.record[..size], peer);
        }
        Ok(())
    }

    #[cfg(feature = "sunfish-paired-oack-init")]
    pub(crate) fn try_init_after_oack(&mut self, generation: u64) -> Result<(), Error> {
        if self.first_grant.outcome == 1 && self.ready_for_init_at_registration() {
            self.publish_inner(generation, Some(core::OackFollowup::FirstGrantInit))?;
        }
        Ok(())
    }

    /// Publish one session record, or use the reserved eight bytes for a single
    /// READ_NOTIF while blocked. A READ_NOTIF never commits the waiting session
    /// record; successful retry publishes that record once after peer progress.
    pub fn publish_next(&mut self, generation: u64) -> Result<Transmit, Error> {
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        let before = self.ring.ingress_clock().map(|at| (at, self.ring.ipa_ingress_work.next));
        let result = self.publish_inner(generation, #[cfg(feature = "sunfish-tftp-oack-followup")] None);
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        if let Some((start, previous)) = before && self.ring.ipa_ingress_work.next == previous
            && let Some(end) = self.ring.ingress_clock()
        {
            let detail = match result {
                Ok(Transmit::Idle) => 0, Ok(Transmit::AwaitingIntent) => 1,
                Ok(Transmit::Blocked { .. }) => 2, Ok(Transmit::Session(_)) => 3, Err(_) => 4,
            };
            self.ring.ipa_ingress_work.push(IpaIngressRecord {
                start, end, kind: 7, cmd: 0xff, detail, size: 0, position: None,
            #[cfg(feature = "sunfish-wrq-stage-costs")] protocol: None,
        });
        }
        result
    }

    #[cfg(feature = "sunfish-first-oack-grant-window")]
    pub(crate) fn first_grant_trace(&self) -> FirstGrantTrace { self.first_grant }

    #[cfg(feature = "sunfish-grant-rx-open")]
    pub(crate) fn grant_receive_trace(&self) -> &GrantReceiveTrace { &self.ring.grant_receive }

    #[cfg(feature = "sunfish-wrq-grant-turns")]
    pub(crate) fn mark_primary_grant(&mut self) {
        self.ring.grant_receive.primary_checked = self.ring.grant_receive.ack_count;
    }

    #[cfg(feature = "sunfish-grant-rx-open")]
    pub(crate) fn take_grant_receive_turn(&mut self) -> bool {
        #[cfg(feature = "sunfish-wrq-grant-turns")]
        {
            let grant = &mut self.ring.grant_receive;
            if grant.ack_count == grant.turn_ack { return false; }
            grant.turn_ack = grant.ack_count;
            grant.turn_count += 1;
        }
        #[cfg(not(feature = "sunfish-wrq-grant-turns"))]
        if self.ring.grant_receive.taken { return false; }
        self.ring.grant_receive.taken = true;
        self.ring.grant_receive.turn_at = self.ring.first_server_check_clock.map(|clock| clock());
        true
    }

    #[cfg(feature = "sunfish-wrq-return-pending")]
    pub(crate) fn wrq_return_pending(&self) -> Option<&WrqReturnPending> { self.ring.wrq_return_pending.as_ref() }

    #[cfg(feature = "sunfish-grant-buffer-reads")]
    pub(crate) fn grant_buffer_reads(&self) -> &GrantBufferReads { &self.ring.grant_reads.request }
    #[cfg(feature = "sunfish-oack-grant-outcome")]
    pub(crate) fn oack_grant_outcome(&self) -> &OackGrantTrace { &self.ring.grant_reads.outcome }


    /// The ordinary writer must commit the exact grant's advertisement or ACK.
    /// Only its remaining positive ACK and original OACK may borrow turns.
    #[cfg(feature = "sunfish-first-oack-grant-window")]
    pub(crate) fn publish_first_grant_turn(&mut self, generation: u64) -> Result<(Transmit, u32), Error> {
        use core::OackFollowup::{FirstGrantAck, FirstGrantReply};
        let expected = if self.first_grant.trigger == 0 {
            self.session.first_oack_grant_trigger()
        } else { None };
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        if expected.is_some() {
            self.first_grant.backing = self.session.grant_backing;
        }
        #[cfg(feature = "sunfish-paired-oack-init")]
        let awaiting_registration = self.registration_close_fifo().is_none();
        #[cfg(feature = "sunfish-first-wrq-return-oack")]
        let wrq_done_before = self.session.server_check_oacks()[0].and_then(|first| first.wrq_done_at);
        let ordinary = self.publish_next(generation)?;
        #[cfg(feature = "sunfish-first-oack-spare-buffer")]
        if let Transmit::Session(Published::ReceiveIntent(id)) = ordinary {
            if self.session.oack_resources(0x5001).is_some_and(|resources| resources.primed_intent == Some(id)) {
                // Only this successful fresh advertisement opens one ordinary
                // checked writer call. A blocked or uncertain write cannot do so.
                self.ring.grant_reads.capture_primed(id);
                if self.session.original_1f_close_count() == Some(0) {
                    let followup = self.publish_next(generation)?;
                    let progress = matches!(followup, Transmit::Session(_)
                        | Transmit::Blocked { read_notification_sent: true });
                    return Ok((ordinary, u32::from(progress)));
                }
            }
        }
        #[cfg(feature = "sunfish-oack-grant-ack-turn")]
        if let Transmit::Session(Published::ReceiveIntent(id)) = ordinary {
            let trace = &self.ring.grant_reads.outcome;
            if trace.oack_captured && trace.intent == Some(id)
                && trace.advertisement.is_some_and(|offer| offer.id == id)
                && trace.ack.is_none() && self.session.grant_backing.requested == 108
                && self.session.grant_backing.intent == Some(id)
                && self.session.original_1f_close_count() == Some(0)
            {
                // The committed exact advertisement opens one post-OACK turn.
                // Use the ordinary selector and checked writer so an earlier
                // protocol obligation or blocked write keeps its normal fallback.
                let followup = self.publish_next(generation)?;
                let progress = matches!(followup, Transmit::Session(_)
                    | Transmit::Blocked { read_notification_sent: true });
                return Ok((ordinary, u32::from(progress)));
            }
        }
        #[cfg(feature = "sunfish-first-wrq-return-oack")]
        if self.first_grant.wrq_return_outcome == 0 && wrq_done_before.is_none()
            && self.session.server_check_oacks()[0].is_some_and(|first| first.wrq_done_at.is_some())
        {
            #[cfg(feature = "sunfish-wrq-return-pending")]
            if self.session.server_check_oacks()[0].is_some_and(|first| first.peer.port == 0x1f)
                && let Some((bytes, early)) = self.tftp_rx_snapshot()
            {
                #[cfg(feature = "sunfish-grant-rx-open")]
                {
                    let remaining = distance(self.ring.words[0], self.ring.words[1]);
                    self.ring.grant_receive.return_tail = Some(if remaining == 0 {
                        ServerCheckTail::Consumed(self.ring.first_server_check_clock.map(|clock| clock()))
                    } else { ServerCheckTail::Pending(remaining) });
                }
                self.ring.wrq_return_pending = Some(WrqReturnPending::new(self.ring.words[2], bytes, early));
            }
            self.first_grant.wrq_return_outcome = 1;
            if let Some((mask, fitting)) = self.session.wrq_return_oack_candidate() {
                self.first_grant.wrq_return_mask = Some(mask);
                self.first_grant.wrq_return_fitting = Some(fitting as u8);
                // Cached RX can veto the opportunity. Admission and publication still
                // require the ordinary fresh FIFO samples, readback and head commit.
                if self.tftp_rx_snapshot() != Some((0, 0)) {
                    self.first_grant.wrq_return_outcome = 2;
                } else if mask == 0x02 && fitting >= 2 {
                    self.session.wrq_return_writer = true;
                    let result = self.publish_inner(generation, Some(FirstGrantReply));
                    self.session.wrq_return_writer = false;
                    self.first_grant.wrq_return_outcome = 3;
                    if matches!(result?, Transmit::Session(Published::Data { last_fragment: true, .. })) {
                        self.first_grant.wrq_return_outcome = 4;
                        return Ok((ordinary, 1));
                    }
                }
            }
        }
        let mut extra = 0;
        #[cfg(feature = "sunfish-paired-oack-init")]
        let expected = if awaiting_registration && self.registration_close_fifo().is_some()
            && self.first_grant.trigger == 0 && self.ready_for_init_at_registration()
        {
            let candidate = self.session.first_oack_grant_trigger();
            #[cfg(feature = "sunfish-first-wrq-spare-oack")]
            if candidate.is_none() && self.session.first_oack_ready_without_grant() {
                // The registration head transition opens this path only once.
                // Keep receive priority and the exact-transfer checks in publish_inner.
                let tx = self.publish_inner(generation, Some(FirstGrantReply))?;
                let sent = matches!(tx, Transmit::Session(Published::Data { last_fragment: true, .. }));
                self.first_grant.direct_turn = if sent { 1 } else { 2 };
                return Ok((ordinary, u32::from(sent)));
            }
            if let Some(ad @ Published::ReceiveIntent(_)) = candidate {
                #[cfg(feature = "sunfish-grant-ad-writer-trace")]
                { self.first_grant.backing = self.session.grant_backing; }
                if self.publish_inner(generation, Some(core::OackFollowup::FirstGrantAd))?
                    != Transmit::Session(ad)
                {
                    return Ok((ordinary, 0));
                }
                extra += 1;
            }
            candidate
        } else { expected };
        let Some(event) = expected else { return Ok((ordinary, 0)); };
        if extra == 0 && ordinary != Transmit::Session(event) { return Ok((ordinary, 0)); }
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        { self.first_grant.backing = self.session.grant_backing; }
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        self.ring.grant_reads.match_backing(self.session.grant_backing.requested, self.session.grant_backing.intent);
        let clock = self.ring.first_server_check_clock;
        if matches!(event, Published::ReceiveIntent(_)) {
            self.first_grant.trigger = 5;
            self.first_grant.advertisement_at = clock.map(|clock| clock());
            let ack = self.publish_inner(generation, Some(FirstGrantAck))?;
            if ack != Transmit::Session(Published::IntentRequestAck { granted: true }) {
                self.first_grant_stop(ack);
                return Ok((ordinary, extra));
            }
            extra += 1;
        } else {
            self.first_grant.trigger = 8;
        }
        self.first_grant.ack_at = clock.map(|clock| clock());
        let reply = self.publish_inner(generation, Some(FirstGrantReply))?;
        if matches!(reply, Transmit::Session(Published::Data { last_fragment: true, .. })) {
            self.first_grant.outcome = 1;
            extra += 1;
        } else {
            self.first_grant_stop(reply);
        }
        Ok((ordinary, extra))
    }

    #[cfg(feature = "sunfish-first-oack-grant-window")]
    fn record_grant_yield(&mut self, bytes: u32) {
        self.first_grant.unread_bytes = bytes;
        #[cfg(feature = "sunfish-first-oack-yield-rx")]
        {
            self.first_grant.window.next_tail = self.ring.words[2];
            self.first_grant.window.remaining = bytes;
        }
    }

    #[cfg(feature = "sunfish-first-oack-grant-window")]
    fn first_grant_stop(&mut self, result: Transmit) {
        self.first_grant.outcome = match result {
            _ if self.first_grant.unread_bytes != 0 || self.first_grant.outcome == 2 => 2,
            Transmit::Blocked { .. } => 3,
            _ => 4,
        };
    }

    #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
    pub(crate) fn early_reserved_oack_pending(&self) -> bool {
        self.session.early_reserved_oack_pending()
    }

    #[cfg(feature = "sunfish-tftp-oack-after-rxdone")]
    pub(crate) fn take_oack_after_done(&mut self) -> bool {
        self.session.take_oack_after_done()
    }

    #[cfg(feature = "sunfish-tftp-oack-credit-early")]
    pub(crate) fn take_oack_credit_turn(&mut self) -> bool {
        self.session.take_oack_credit_turn()
    }

    #[cfg(feature = "sunfish-tftp-oack-credit-retry")]
    pub(crate) fn take_oack_credit_retry(&mut self) -> bool {
        self.session.take_oack_credit_retry()
    }

    #[cfg(feature = "sunfish-tftp-oack-grant-followup")]
    pub(crate) fn oack_followup_grant_pending(&self) -> bool {
        self.session.oack_followup_grant_pending()
    }

    #[cfg(feature = "sunfish-tftp-oack-followup")]
    pub(crate) fn publish_oack_followup(&mut self, generation: u64, stage: core::OackFollowup)
        -> Result<Transmit, Error>
    {
        self.publish_inner(generation, Some(stage))
    }

    fn publish_inner(&mut self, generation: u64,
        #[cfg(feature = "sunfish-tftp-oack-followup")] followup: Option<core::OackFollowup>,
    ) -> Result<Transmit, Error> {
        self.check(generation)?;
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        let first_prearm_io = self.ring.prearm_io.is_some_and(|t| !t.entered);
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        if first_prearm_io {
            let totals=self.ring.io.registry_prearm_totals();let at=self.ring.first_server_check_clock.map(|now|now());
            let trace=self.ring.prearm_io.as_mut().unwrap();trace.entered=true;trace.first_enter=totals;trace.first_enter_at=at;
        }
        self.quarantined = true;
        self.ring.sample()?;
        #[cfg(feature = "sunfish-first-oack-grant-window")]
        let first_grant = matches!(followup, Some(core::OackFollowup::FirstGrantAd | core::OackFollowup::FirstGrantInit | core::OackFollowup::FirstGrantAck | core::OackFollowup::FirstGrantReply));
        #[cfg(not(feature = "sunfish-first-oack-grant-window"))]
        let first_grant = false;
        #[cfg(feature = "sunfish-first-oack-grant-window")]
        if first_grant {
            let (bytes, early) = self.tftp_rx_snapshot().ok_or(Error::NotReady)?;
            if bytes != 0 || early != 0 {
                self.record_grant_yield(bytes);
                self.first_grant.outcome = 2;
                self.quarantined = false;
                return Ok(Transmit::Idle);
            }
        }
        #[cfg(feature = "sunfish-tftp-tail-observer")]
        let first_oack_before = self.session.tftp_endpoint_trace().map(|t| t.flags);
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        let first_server_check_queued = self.ring.first_server_check_tail == ServerCheckTail::Untracked
            && self.session.first_server_check().is_some_and(|trace| trace.oack.phase == 2);
        #[cfg(feature = "sunfish-ipa-registration-reply-path")]
        let registration_pending = self.ring.registration_reply.is_none()
            && self.session.ipa_registration_trace().is_some_and(|reg|
                reg.queued_at.is_some() && reg.published_at.is_none());
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        let watch_oack = self.ring.oack_peer_tail.is_none()
            && self.session.server_check_49().is_some_and(|trace| trace.oack_at.is_none());
        #[cfg(feature = "sunfish-tftp-oack-early-reserve")]
        if matches!(followup, Some(core::OackFollowup::EarlyReserve))
            && self.ring.oack_close_fifo.is_none()
            && let Some(length) = self.ring.rx_bytes
        {
            let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
            self.ring.oack_close_fifo = Some(ServerCheckRxWindow::new(unread,
                self.ring.first_server_check_clock.map(|clock| clock())));
        }
        let mut failure = None;
        #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
        let mut opening_header = None;
        #[cfg(feature="sunfish-registry-tx3-wire-headers")]
        let watch_registry_wire=self.ring.registry_wire.is_none()
            && self.session.registry_window_trace().predecessor.is_some_and(|p|!p.publication_frozen);
        #[cfg(feature="sunfish-registry-tx3-wire-headers")]
        let mut opening_registry_wire=None;
        #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
        let watch_reserved = self.session.reserved_oack().is_some_and(|(oack, peer)|
            oack.peer.port == 0x49 && oack.published_at.is_none()
                && peer.is_some_and(|p| p.first_close_at.is_none()));
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        let init_pending = self.ring.init_close.fresh_unread.is_none()
            && self.session.ipa_init_transport_trace().is_some_and(|t| t.published_at.is_none());
        let mut receive_arrived = false;
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        let oack_pending = self.ring.wrq_prefix.oack_head.is_none()
            && self.session.first_server_check().is_some_and(|t| t.exact_wrq && t.peer.node == 0
                && t.peer.port == 0x1f && t.oack.phase == 2);
        #[cfg(feature = "sunfish-oack-grant-outcome")]
        {
            if self.session.grant_backing.requested == 108 {
                self.ring.grant_reads.sync_108(self.session.grant_backing.intent, self.session.grant_backing.reserved_at);
            }
            self.ring.grant_reads.oack_pool = (oack_pending && !self.ring.grant_reads.outcome.oack_captured)
                .then(|| (self.session.grant_available_ids(), self.session.receive_snapshot().states));
        }
        #[cfg(feature = "sunfish-glink-announcement-reuse")]
        let reusable_ack = self.session.reusable_intent_ack_pending();
        let writer = |record: &[u8]| {
            #[cfg(feature = "sunfish-glink-announcement-reuse")]
            let yield_to_rx = first_grant || (reusable_ack && record.len() == 8
                && u16::from_le_bytes([record[0],record[1]]) == 8 && word(record,4) == 1);
            #[cfg(not(feature = "sunfish-glink-announcement-reuse"))]
            let yield_to_rx = first_grant;
            #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
            let before = watch_reserved.then(|| self.ring.io.reserved_io_totals()).flatten();
            #[cfg(feature = "sunfish-wrq-prefix-close")]
            { self.ring.wrq_prefix.watch_head = oack_pending || init_pending; }
            #[cfg(feature = "sunfish-grant-buffer-reads")]
            { self.ring.grant_session_write = true; }
            let result = self.ring.write_checked(record, TX_RESERVE, yield_to_rx);
            #[cfg(feature = "sunfish-grant-buffer-reads")]
            { self.ring.grant_session_write = false; }
            #[cfg(feature = "sunfish-wrq-prefix-close")]
            { self.ring.wrq_prefix.watch_head = false; }
            #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
            if watch_reserved {
                let after = self.ring.io.reserved_io_totals();
                self.ring.reserved_writer_io.record(before, after, true);
            }
            match result {
                Ok(false) => {
                    receive_arrived = true;
                    Err(WriteError::WouldBlock)
                }
                Ok(true) => {
                    #[cfg(feature="sunfish-registry-tx3-wire-headers")]
                    if watch_registry_wire{opening_registry_wire=RegistryWireHeader::read(record,true,
                        self.ring.first_server_check_clock.map(|clock|clock()));}
                    #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
                    if registration_pending {
                        opening_header = Some(RegistrationHeader::read(record, true,
                            self.ring.first_server_check_clock.map(|clock| clock())));
                    }
                    Ok(())
                },
                Err(Error::WouldBlock) => Err(WriteError::WouldBlock),
                Err(error) => {
                    failure = Some(error);
                    Err(WriteError::Uncertain)
                }
            }
        };
        #[cfg(feature = "sunfish-tftp-oack-followup")]
        let result = match followup {
            Some(stage) => self.session.publish_oack_followup(generation, stage, writer),
            None => self.session.publish_next(generation, writer),
        };
        #[cfg(not(feature = "sunfish-tftp-oack-followup"))]
        let result = self.session.publish_next(generation, writer);
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        {
            let arm = self.session.registry_window_trace().target.is_some_and(|p| p.conversion.arms != 0);
            if self.ring.prearm_io.is_some_and(|t| (first_prearm_io && !t.ended) || (arm && !t.armed)) {
                let totals=self.ring.io.registry_prearm_totals();let at=self.ring.first_server_check_clock.map(|now|now());
                let trace=self.ring.prearm_io.as_mut().unwrap();
                if first_prearm_io && !trace.ended {trace.ended=true;trace.first_end=totals;trace.first_end_at=at;}
                if arm && !trace.armed {trace.armed=true;trace.arm=totals;trace.arm_at=at;}
                if arm {self.ring.io.registry_prearm_timing(false);}
            }
        }
        #[cfg(feature = "sunfish-oack-grant-outcome")]
        { self.ring.grant_reads.oack_pool = None; }
        if let Some(error) = failure {
            return Err(error);
        }
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        if matches!(result, Ok(Some(core::Published::Data { last_fragment: true, .. }))) {
            // Bind the head-store clock only after normal publication identifies this transaction.
            let head = self.ring.wrq_prefix.last_data_head;
            if oack_pending && self.session.first_server_check().is_some_and(|t| t.oack.phase == 3) {
                self.ring.wrq_prefix.oack_head = head;
            }
            if init_pending && self.session.ipa_init_transport_trace().is_some_and(|t| t.published_at.is_some()) {
                self.ring.wrq_prefix.init_head = head;
            }
        }
        // A receive-priority yield can move this exact advertisement to a later writer pass.
        #[cfg(feature = "sunfish-grant-ad-writer-trace")]
        if let Ok(Some(core::Published::ReceiveIntent(id))) = result
            && self.first_grant.backing.intent == Some(id)
            && self.session.grant_backing.intent == Some(id)
        {
            self.first_grant.backing = self.session.grant_backing;
            #[cfg(feature = "sunfish-grant-buffer-reads")]
            self.ring.grant_reads.match_backing(self.session.grant_backing.requested, Some(id));
        }
        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
        if let Ok(Some(core::Published::Data { remote_intent, last_fragment: true })) = result
            && let Some(handle) = self.session.published_oack_request(remote_intent)
        {
            self.observe_registration_tftp_oack_progress(handle, 2);
        }
        let result = match result {
            Ok(Some(published)) => Transmit::Session(published),
            Ok(None) => Transmit::Idle,
            Err(core::Error::NoRemoteIntent) => Transmit::AwaitingIntent,
            Err(core::Error::WouldBlock) if receive_arrived => {
                #[cfg(feature = "sunfish-first-oack-grant-window")]
                if first_grant { self.record_grant_yield(self.tftp_rx_snapshot().ok_or(Error::NotReady)?.0); }
                Transmit::Idle
            }
            Err(core::Error::WouldBlock) => Transmit::Blocked {
                read_notification_sent: self.ring.blocked_notice()?,
            },
            Err(error) => {
                self.session_failure = Some(SessionFailureDiagnostic { stage: 4, command: u32::MAX, parameter: u32::MAX });
                if error == core::Error::NoCapacity { self.session.record_capacity(2, None); }
                return Err(error.into());
            }
        };
        #[cfg(feature = "sunfish-wrq-stage-costs")]
        if let Some(first) = self.session.server_check_oacks()[0]
            && first.peer.port == 0x1f && let Some(returned) = first.wrq_done_at
        { self.ring.wrq_work.finish(returned); }
        #[cfg(feature = "sunfish-wrq-validation-path")]
        if self.session.server_check_oacks()[0].is_some_and(|first|first.peer.port==0x1f && first.oack_at.is_some())
            && let Some(end)=self.ring.wrq_work_clock()
        {self.ring.wrq_work.validation.finish(end);}
        #[cfg(feature = "sunfish-grant-rx-open")]
        if let Transmit::Session(Published::IntentRequestAck { granted }) = result {
            let matched = ::core::mem::take(&mut self.ring.grant_receive.awaiting_ack);
            #[cfg(not(feature = "sunfish-wrq-grant-turns"))]
            let track = !self.ring.grant_receive.taken;
            #[cfg(feature = "sunfish-wrq-grant-turns")]
            let track = self.session.original_server_check().is_none();
            if granted && matched && track {
                self.ring.grant_receive.ack_count = self.ring.grant_receive.ack_count.saturating_add(1);
                self.ring.grant_receive.ack_at = self.ring.first_server_check_clock.map(|clock| clock());
            }
        }
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        if init_pending && matches!(result, Transmit::Session(core::Published::Data { last_fragment: true, .. }))
            && self.session.ipa_init_transport_trace().is_some_and(|t| t.published_at.is_some())
        {
            // Wire publication and the doorbell succeeded. Read a fresh,
            // validated cursor pair before returning control to the scheduler.
            let clock = self.ring.first_server_check_clock;
            let start = clock.map(|now| now());
            self.ring.sample()?;
            self.ring.init_close.sample_start = start;
            self.ring.init_close.sample_end = clock.map(|now| now());
            self.ring.init_close.fresh_unread = self.ring.rx_bytes.map(|length|
                ring_distance(self.ring.words[2], self.ring.words[3], length) as u32);
            self.ring.init_close.fresh_head = self.ring.registration_rx.as_ref().map(|h| h.head);
        }
        #[cfg(feature = "sunfish-registry-post-reply-trace")]
        if self.ring.registry_reply.is_none()
            && self.session.registry_window_trace().predecessor.is_some_and(|p| p.publication_frozen)
            && let Some(length) = self.ring.rx_bytes
        {
            // Successful checked publication sampled the cursor pair on both
            // sides of its head store. Reuse that pair; add no hardware reads.
            self.ring.registry_reply = Some(RegistryReplyWindow::new(
                distance(self.ring.words[0], self.ring.words[1]),
                ring_distance(self.ring.words[2], self.ring.words[3], length),
                self.ring.first_server_check_clock.map(|clock| clock())));
        }
        #[cfg(feature="sunfish-registry-tx3-wire-headers")]
        if watch_registry_wire && self.session.registry_window_trace().predecessor.is_some_and(|p|p.publication_frozen){
            let mut trace=RegistryWireTrace::default();trace.record(opening_registry_wire,false);self.ring.registry_wire=Some(trace);
        }
        #[cfg(feature = "sunfish-ipa-registration-reply-path")]
        if registration_pending
            && self.session.ipa_registration_trace().is_some_and(|reg| reg.published_at.is_some())
        {
            let remaining = distance(self.ring.words[0], self.ring.words[1]);
            let now = self.ring.first_server_check_clock.map(|clock| clock());
            let unread = self.ring.rx_bytes.map(|length|
                ring_distance(self.ring.words[2], self.ring.words[3], length));
            #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
            {
                self.ring.registration_reply_rx = unread.map(|bytes| ServerCheckRxWindow::new(bytes, now));
            }
            self.ring.registration_reply = Some(RegistrationReplyFifo {
                remaining,
                peer_tail_at: now.filter(|_| remaining == 0),
                last_empty_at: now.filter(|_| unread == Some(0)),
                max_unread: unread.unwrap_or(0),
                last_poll_at: now,
                ..RegistrationReplyFifo::default()
            });
            #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
            {
                // write_checked sampled on each side of the successful TX-head
                // store. The host has not advanced the RX tail since either.
                let clock = self.ring.first_server_check_clock;
                let fresh_start = clock.map(|now| now());
                self.ring.sample()?;
                self.ring.reg_close = Some(RegistrationCloseFifo {
                    pre: self.ring.reg_pre_unread,
                    post: unread.map(|bytes| bytes as u32),
                    fresh: self.ring.rx_bytes.map(|length| ring_distance(self.ring.words[2], self.ring.words[3], length) as u32),
                    fresh_start,
                    fresh_end: clock.map(|now| now()),
                    already_closed: self.session.original_1f_close_count().unwrap_or(0) != 0,
                    ..RegistrationCloseFifo::default()
                });
            }
            #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
            if let (Some(trace), Some(header)) = (self.ring.registration_reply.as_mut(), opening_header) {
                trace.push_header(header);
            }
        }
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        if watch_oack && self.session.server_check_49().is_some_and(|trace| trace.oack_at.is_some()) {
            let remaining = distance(self.ring.words[0], self.ring.words[1]);
            self.ring.oack_peer_tail = Some(OackPeerTail::new(remaining,
                self.ring.first_server_check_clock.map(|clock| clock())));
            #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
            if let Some(length) = self.ring.rx_bytes {
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                self.ring.reserved_oack_rx = Some(ServerCheckRxWindow::new(
                    unread, self.ring.first_server_check_clock.map(|clock| clock())));
                #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
                { self.ring.reserved_oack_ahead = Some(OackAheadCapture::new(
                    self.ring.first_server_check_clock.map(|clock| clock()), unread)); }
            }
        }
        // Arm after the exact repeat's RX_DONE and before the owner can
        // attempt the advertisement, grant ACK and OACK follow-up.
        #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
        if self.ring.oack_close_fifo.is_none()
            && self.session.oack_slot_follow().is_some_and(|trace| trace.exact_repeat_done_at.is_some())
            && self.session.server_check_49().is_some_and(|trace| trace.first_close_at.is_none())
            && let Some(length) = self.ring.rx_bytes
        {
            let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
            self.ring.oack_close_fifo = Some(ServerCheckRxWindow::new(unread,
                self.ring.first_server_check_clock.map(|clock| clock())));
        }
        #[cfg(feature = "sunfish-tftp-tail-observer")]
        if first_oack_before.is_some_and(|flags| flags & 4 == 0)
            && self.session.tftp_endpoint_trace().is_some_and(|trace| trace.flags & 4 != 0) {
            // The successful writer just published exactly one GLINK record.
            // Its current head is the first OACK's final boundary. No new I/O.
            let remaining = distance(self.ring.words[0], self.ring.words[1]);
            if remaining == 0 { self.ring.tftp_tail_seen = true; }
            else { self.ring.tftp_tail_remaining = Some(remaining); }
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        if first_server_check_queued && self.session.first_server_check().is_some_and(|trace| trace.oack.phase == 3) {
            // The checked writer just published the final record of this OACK.
            // Retain the distance to its head across subsequent cursor samples.
            let remaining = distance(self.ring.words[0], self.ring.words[1]);
            self.ring.first_server_check_tail = if remaining == 0 {
                ServerCheckTail::Consumed(self.ring.first_server_check_clock.map(|clock| clock()))
            } else {
                ServerCheckTail::Pending(remaining)
            };
            #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
            if let Some(length) = self.ring.rx_bytes {
                let unread = ring_distance(self.ring.words[2], self.ring.words[3], length);
                self.ring.server_check_rx = Some(ServerCheckRxWindow::new(
                    unread, self.ring.first_server_check_clock.map(|clock| clock())));
            }
        }
        self.quarantined = false;
        Ok(result)
    }

    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
    pub(crate) fn observe_registration_tftp_dispatch(
        &mut self, handle: core::TftpHandle, result: u8, occupied: u8,
    ) {
        if let Some(request) = self.registration_tftp_request_mut(handle) {
            request.server_result = result;
            request.occupied = occupied;
        }
        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
        self.update_registration_tftp_originals(handle, |original| {
            original.result = result;
            original.occupied = occupied;
        });
    }

    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
    pub(crate) fn observe_registration_tftp_oack_queued(&mut self, handle: core::TftpHandle) {
        if let Some(request) = self.registration_tftp_request_mut(handle) {
            request.oack_queued = true;
        }
        #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
        self.observe_registration_tftp_oack_progress(handle, 1);
    }

    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
    pub(crate) fn observe_registration_tftp_duplicate(
        &mut self, duplicate: core::TftpHandle, source: Option<(core::TftpHandle, u8)>,
    ) {
        let Some((handle, source)) = source else { return; };
        let mut original = OriginalTftpRequest {
            handle, id: handle.diagnostic_id(), source, result: u8::MAX, occupied: u8::MAX, phases: 0,
            #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
            queue_block: core::TftpQueueBlock::default(),
            #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
            full_queue: None,
        };
        if let Some(request) = self.registration_tftp_request_mut(handle) {
            original.result = request.server_result;
            original.occupied = request.occupied;
            original.phases = request.oack_progress;
            #[cfg(feature = "sunfish-ipa-tftp-queue-block")]
            { original.queue_block = request.queue_block; }
            #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
            { original.full_queue = request.full_queue; }
        }
        if let Some(request) = self.registration_tftp_request_mut(duplicate) {
            request.original = Some(original);
        }
    }

    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
    fn observe_registration_tftp_oack_progress(&mut self, handle: core::TftpHandle, phase: u8) {
        if let Some(request) = self.registration_tftp_request_mut(handle) {
            request.oack_progress |= phase;
        }
        self.update_registration_tftp_originals(handle, |original| original.phases |= phase);
    }

    #[cfg(feature = "sunfish-ipa-tftp-slot-snapshot")]
    fn observe_tftp_slot_release(&mut self, occupied_before: u8) {
        if occupied_before == 0 { return; }
        let freed = occupied_before & !self.session.tftp_tx_occupied();
        if freed == 0 { return; }
        let at = self.ring.first_server_check_clock.map(|clock| clock());
        let Some(trace) = self.ring.registration_reply.as_mut() else { return; };
        let record = |snapshot: &mut Option<TftpFullQueue>| {
            if let Some(snapshot) = snapshot.as_mut() {
                snapshot.record_release(freed, at);
            }
        };
        for request in trace.tftp_requests.iter_mut().flatten()
            .chain(trace.headers.iter_mut().flatten().filter_map(|header| header.tftp_request.as_mut()))
        {
            record(&mut request.full_queue);
            if let Some(original) = request.original.as_mut() {
                record(&mut original.full_queue);
            }
        }
    }

    #[cfg(feature = "sunfish-ipa-tftp-original-trace")]
    fn update_registration_tftp_originals(
        &mut self, handle: core::TftpHandle, update: impl Fn(&mut OriginalTftpRequest),
    ) {
        let Some(trace) = self.ring.registration_reply.as_mut().filter(|trace| !trace.finished) else { return; };
        for request in trace.tftp_requests.iter_mut().flatten()
            .chain(trace.headers.iter_mut().flatten().filter_map(|header| header.tftp_request.as_mut()))
        {
            if let Some(original) = request.original.as_mut().filter(|original| original.handle == handle) {
                update(original);
            }
        }
    }

    #[cfg(feature = "sunfish-ipa-tftp-dispatch-trace")]
    fn registration_tftp_request_mut(&mut self, handle: core::TftpHandle) -> Option<&mut RegistrationTftpRequest> {
        self.ring.registration_reply.as_mut().filter(|trace| !trace.finished)?
            .tftp_requests.iter_mut().rev().flatten().find(|request| request.handle == Some(handle))
    }

    #[cfg(feature = "sunfish-tftp-oack-reserve")]
    pub(crate) fn reserved_oack(&self) -> Option<(core::ReservedOack, Option<core::ServerCheckOackTrace>)> {
        self.session.reserved_oack()
    }

    #[cfg(feature = "sunfish-spare-reg-admission")]
    pub(crate) fn original_oack_awaits_registration(&self) -> bool {
        self.session.original_1f_close_count() == Some(0)
            && self.session.first_oack_eligibility().queued_at.is_some()
            && self.session.server_check_oacks()[0].is_some_and(|first| first.oack_at.is_none())
    }

    #[cfg(feature = "sunfish-ipa-init-empty-rx")]
    pub(crate) fn ready_for_init_at_registration(&self) -> bool {
        self.ring.reg_close.as_ref().is_some_and(|trace| trace.fresh == Some(0))
            && self.tftp_rx_snapshot() == Some((0, 0))
            && !self.session.deferred_ready()
            && self.session.original_1f_close_count() == Some(0)
    }

    #[cfg(feature = "sunfish-ipa-init-close-visibility")]
    pub(crate) fn init_close_fifo(&self) -> InitCloseFifo { self.ring.init_close }

    #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
    pub(crate) fn registration_close_fifo(&self) -> Option<RegistrationCloseFifo> { self.ring.reg_close }

    #[cfg(feature = "sunfish-ipa-registration-reply-path")]
    pub(crate) fn registration_reply_fifo(&self) -> Option<RegistrationReplyFifo> {
        self.ring.registration_reply
    }

    #[cfg(feature = "sunfish-spare-oack-completions")]
    pub(crate) fn registration_completion_window(&self) -> (Option<u64>, Option<ServerCheckRxDone>) {
        self.ring.registration_reply.as_ref()
            .map(|trace| (trace.peer_tail_at, trace.completion_visibility)).unwrap_or_default()
    }

    /// Existing cursor samples observed the peer advance its TX tail through
    /// the first OACK boundary. This is not application-level receipt.
    #[cfg(feature = "sunfish-tftp-tail-observer")]
    pub fn tftp_tail_seen(&self) -> bool { self.ring.tftp_tail_seen }

    #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
    pub(crate) fn first_server_check_tail(&self) -> ServerCheckTail {
        self.ring.first_server_check_tail
    }

    #[cfg(feature = "sunfish-tftp-first-oack-headers")]
    pub(crate) fn first_oack_headers(&self) -> Option<FirstOackHeaders> {
        self.ring.server_check_rx.as_ref().map(|trace| trace.headers)
    }
    #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
    pub(crate) fn registration_rx_visibility(&self) -> Option<ServerCheckRxDone> {
        self.ring.registration_rx.as_ref().and_then(|trace| trace.done)
    }
    #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
    pub(crate) fn first_oack_rx_done(&self) -> Option<ServerCheckRxDone> {
        self.ring.server_check_rx.as_ref().and_then(|trace| trace.done)
    }

    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    pub(crate) fn track_first_wrq(&mut self) {
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        { self.ring.grant_reads.clock_armed = true; }
        self.ring.track_first_wrq = self.session.first_server_check().is_none();
        #[cfg(feature = "sunfish-ipa-announcement-fifo")]
        { self.ring.track_ipa_announcement = self.session.first_ipa_announcement().is_none(); }
    }

    #[cfg(feature = "sunfish-ipa-ingress-work")]
    pub(crate) fn ipa_ingress_work(&self) -> &IpaIngressWork { &self.ring.ipa_ingress_work }
    #[cfg(feature = "sunfish-spare-wrq-ingress")]
    pub(crate) fn wrq_receive_work(&self) -> &WrqReceiveWork { &self.ring.wrq_work }
    #[cfg(feature = "sunfish-wrq-prefix-close")]
    pub(crate) fn wrq_prefix(&self) -> &WrqPrefix { &self.ring.wrq_prefix }

    #[cfg(feature = "sunfish-ipa-announcement-fifo")]
    pub(crate) fn ipa_announcement_rx(&self) -> Option<ServerCheckRxDone> {
        self.ring.ipa_announcement_rx
    }

    #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
    pub(crate) fn ipa_wrq_timing(&self) -> (Option<u64>, Option<ServerCheckRxDone>) {
        (self.session.ipa_endpoint_ready_at(), self.ring.first_wrq_rx)
    }

    /// Last validated RX cursor sample and any startup records still queued
    /// in the adapter. No additional device or shared-memory reads.
    #[cfg(feature = "sunfish-tftp-rx-backlog-observer")]
    pub fn tftp_rx_snapshot(&self) -> Option<(u32, u32)> {
        let length = self.ring.rx_bytes?;
        let bytes = ring_distance(self.ring.words[2], self.ring.words[3], length);
        #[cfg(feature = "sunfish-linux-mpss-glink")]
        let early = self.ring.control.as_ref().map_or(0, |c| c.early.iter().flatten().count());
        #[cfg(not(feature = "sunfish-linux-mpss-glink"))]
        let early = 0;
        Some((bytes as u32, early as u32))
    }

    /// Logical recovery ONLY after the native owner has stopped/drained the old
    /// peer and serialized all reset/IRQ/writer activity. Requires a newer local
    /// generation, unchanged mapping identity, and both rings empty. This does
    /// not stop hardware, rewrite cursors, renegotiate, or revoke DMA/SCM access.
    pub fn reset_after_transport_reset(&mut self, generation: u64) -> Result<(), Error> {
        #[cfg(feature = "sunfish-linux-mpss-glink")]
        if self.ring.control.is_some() {
            self.quarantined = true;
            return Err(Error::Negotiation);
        }
        if generation <= self.ring.generation {
            return Err(Error::StaleGeneration);
        }
        self.quarantined = true;
        let observation = self.ring.io.observe()?;
        ready(observation, generation)?;
        if observation.identity != self.ring.identity {
            return Err(Error::IdentityChanged);
        }
        if self.ring.io.rx_length()? != self.ring.rx_bytes {
            return Err(Error::IdentityChanged);
        }
        let words = descriptors(&mut self.ring.io, self.ring.rx_bytes)?;
        if words[0] != words[1] || words[2] != words[3] {
            return Err(Error::ResetNotEmpty);
        }
        self.ring.io.barrier(Barrier::Acquire)?;
        let after = self.ring.io.observe()?;
        ready(after, generation)?;
        if after != observation || descriptors(&mut self.ring.io, self.ring.rx_bytes)? != words {
            return Err(Error::IdentityChanged);
        }
        self.session.reset_after_transport_reset(generation)?;
        #[cfg(feature = "sunfish-first-oack-grant-window")]
        { self.first_grant = FirstGrantTrace::default(); }
        #[cfg(feature = "sunfish-grant-buffer-reads")]
        { self.ring.grant_reads = Default::default(); }
        #[cfg(feature = "sunfish-grant-rx-open")]
        { self.ring.grant_receive = Default::default(); }
        #[cfg(feature = "sunfish-wrq-return-pending")]
        { self.ring.wrq_return_pending = None; }
        self.ring.generation = generation;
        self.ring.words = words;
        self.ring.read_notification_end = None;
        #[cfg(feature = "sunfish-tftp-tail-observer")]
        {
            self.ring.tftp_tail_remaining = None;
            self.ring.tftp_tail_seen = false;
        }
        #[cfg(feature = "sunfish-wlan-first-server-check-tail")]
        { self.ring.first_server_check_tail = ServerCheckTail::Untracked; }
        #[cfg(feature = "sunfish-tftp-oack-peer-tail")]
        { self.ring.oack_peer_tail = None; }
        #[cfg(feature = "sunfish-tftp-oack-close-fifo")]
        { self.ring.oack_close_fifo = None; }
        #[cfg(feature = "sunfish-tftp-oack-fifo-outcome")]
        { self.ring.reserved_oack_rx = None; }
        #[cfg(feature = "sunfish-tftp-oack-ahead-headers")]
        { self.ring.reserved_oack_ahead = None; }
        #[cfg(feature = "sunfish-tftp-reserved-writer-io")]
        { self.ring.reserved_writer_io = ReservedWriterIoTrace::default(); }
        #[cfg(feature = "sunfish-wlan-first-oack-rx-fifo")]
        { self.ring.server_check_rx = None; }
        #[cfg(feature = "sunfish-ipa-registration-rx-visibility")]
        { self.ring.registration_rx = None; }
        #[cfg(feature = "sunfish-ipa-wrq-ready-fifo")]
        { self.ring.first_wrq_rx = None; self.ring.track_first_wrq = false; }
        #[cfg(feature = "sunfish-ipa-announcement-fifo")]
        { self.ring.ipa_announcement_rx = None; self.ring.track_ipa_announcement = false; }
        #[cfg(feature = "sunfish-ipa-ingress-work")]
        { self.ring.ipa_ingress_work = Default::default(); }
        #[cfg(feature = "sunfish-spare-wrq-ingress")]
        { self.ring.wrq_work = Default::default(); }
        #[cfg(feature = "sunfish-wrq-prefix-close")]
        { self.ring.wrq_prefix = Default::default(); }
        #[cfg(feature = "sunfish-ipa-registration-reply-path")]
        { self.ring.registration_reply = None; }
        #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
        { self.ring.registration_reply_rx = None; }
        #[cfg(feature = "sunfish-registry-post-reply-trace")]
        { self.ring.registry_reply = None; }
        #[cfg(feature="sunfish-registry-tx3-wire-headers")]
        { self.ring.registry_wire=None; }
        #[cfg(feature = "sunfish-registry-prearm-io-trace")]
        { self.ring.io.registry_prearm_timing(false); self.ring.prearm_io = None; }
        #[cfg(feature = "sunfish-ipa-init-close-visibility")]
        { self.ring.init_close = InitCloseFifo::default(); }
        #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
        { self.ring.reg_close = None; self.ring.reg_pre_unread = None; }
        self.ring.record.fill(0);
        self.session_failure = None;
        self.quarantined = false;
        Ok(())
    }
}

#[cfg(all(test, feature = "sunfish-wrq-return-headers"))]
mod wrq_window_tests {
    use super::*;
    fn open() -> [u8; 16] {
        [2, 0, 7, 0, 3, 0, 0, 0, b'D', b'S', 0, 0, 0, 0, 0, 0]
    }
    #[test]
    fn original_window_keeps_every_header_and_excludes_later_append() {
        let mut trace = WrqReturnPending::new(0, 104, 0);
        trace.record_header(&open(), Some(10));
        trace.advanced(16, 16, Some(11));
        let mut data = [0u8; 72];
        data[0] = 9;
        data[2] = 2;
        data[8] = 52;
        trace.record_header(&data, Some(12));
        trace.advanced(88, 72, Some(13));
        let done = [14, 0, 2, 0, 42, 0, 0, 0];
        trace.record_header(&done, Some(14));
        trace.advanced(96, 8, Some(15));
        trace.record_header(&done, Some(16));
        trace.advanced(104, 8, Some(17));
        trace.record_header(&open(), Some(18));
        assert_eq!((trace.header_count, trace.header_omitted), (4, 0));
        assert_eq!(
            trace
                .headers()
                .map(|header| header.offset)
                .collect::<std::vec::Vec<_>>(),
            [0, 16, 88, 96]
        );
        let header = trace.headers().next().unwrap();
        assert_eq!(
            (header.channel, header.name, header.checked, header.clear),
            (7, 0x44530000, Some(10), Some(11))
        );
        assert_eq!(trace.backlog_clear, Some(17));
    }
    #[test]
    fn window_overlap_keeps_full_length_and_uncertain_tail_unconfirmed() {
        let mut trace = WrqReturnPending::new(4088, 8, 0);
        assert!(trace.follows(4088));
        trace.record_header(&open(), Some(20));
        trace.record_header(&open(), Some(21));
        assert_eq!(trace.header_count, 1);
        assert_eq!(trace.headers().next().unwrap().clear, None);
        assert_eq!(trace.backlog_clear, None);
        trace.advanced(8, 16, Some(22));
        assert_eq!(trace.headers().next().unwrap().header.bytes, 16);
        assert_eq!(trace.backlog_clear, Some(22));
        assert!(!trace.follows(8));
        let empty = WrqReturnPending::new(0, 0, 0);
        assert_eq!(empty.headers().count(), 0);
    }
    #[test]
    fn full_capture_reports_overflow_and_does_not_overwrite_opening() {
        let mut trace = WrqReturnPending::new(0, 128, 0);
        let notice = [13, 0, 0, 0, 0, 0, 0, 0];
        for i in 0..16 {
            trace.record_header(&notice, Some(i));
            trace.advanced((i as u32 + 1) * 8, 8, Some(i + 1));
        }
        assert_eq!((trace.header_count, trace.header_omitted), (16, 3));
        assert_eq!(trace.headers().count(), 13);
        assert_eq!(trace.headers().next().unwrap().offset, 0);
    }
}

#[cfg(all(test, feature = "sunfish-oack-grant-outcome"))]
mod oack_grant_fifo_tests {
    use super::*;

    fn clock() -> u64 { 100 }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    fn after_oack() -> (Adapter<crate::Fake>, crate::Fake) {
        let peer = crate::Fake::new(0, 0);
        let mut session = crate::session();
        session.prepare_tftp_server(1, 0x5000).unwrap();
        let mut adapter = Adapter::attach(peer.clone(), session, 1).unwrap();
        adapter.start_control(1).unwrap();
        let mut first_id = None;
        let mut wrq_id = None;
        for _ in 0..6 {
            let id = adapter.offer_receive(1).unwrap();
            if first_id.is_some() && wrq_id.is_none() { wrq_id = Some(id); }
            first_id = first_id.or(Some(id));
            adapter.publish_next(1).unwrap();
            peer.take_tx();
        }
        let mut intents = crate::control(5, 2, 4);
        for id in 50u32..54 {
            intents.extend(4096u32.to_le_bytes());
            intents.extend(id.to_le_bytes());
        }
        peer.push(&intents);
        adapter.poll_receive(1).unwrap();
        let mut hello = std::vec::Vec::new();
        for value in [1u32, 2, 0, 0xffff_fffe, 0, 20, 1, 0xffff_fffe, 2, 0, 0, 0, 0] {
            hello.extend(value.to_le_bytes());
        }
        peer.push(&crate::data(first_id.unwrap(), &hello));
        adapter.poll_receive(1).unwrap();
        for _ in 0..8 {
            let tx = adapter.publish_next(1).unwrap();
            peer.take_tx();
            if let Transmit::Session(Published::Data { remote_intent, .. }) = tx {
                peer.push(&crate::control(6, 2, remote_intent));
                adapter.poll_receive(1).unwrap();
            }
        }
        let request = crate::qcom_tftp_server_check::REQUEST;
        let mut packet = std::vec::Vec::new();
        for value in [1u32, 1, 0, 0x1f, 0, request.len() as u32, 1, 0x5000] {
            packet.extend(value.to_le_bytes());
        }
        packet.extend(request);
        packet.resize((packet.len() + 3) & !3, 0);
        peer.push(&crate::data(wrq_id.unwrap(), &packet));
        assert!(matches!(adapter.poll_receive(1).unwrap(), Receive::Session(Event::TftpRequest { .. })));
        assert_eq!(adapter.session.original_1f_close_count(), Some(0));
        // The full publication observer is tested separately. Here it arms the
        // post-OACK guard so the real grant writer and FIFO faults can be replayed.
        adapter.ring.first_server_check_clock = Some(clock);
        adapter.ring.grant_reads.clock_armed = true;
        adapter.ring.grant_reads.capture_oack([None; core::RX_SLOTS], 0, 0, Some(100));
        adapter.ring.wrq_return_pending = Some(WrqReturnPending::new(peer.0.borrow().words[2], 8, 0));
        peer.push(&[7, 0, 2, 0, 108, 0, 0, 0]);
        assert_eq!(adapter.poll_receive(1).unwrap(), Receive::Session(Event::ReceiveIntentRequest { granted: true }));
        (adapter, peer)
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn post_oack_advertisement_gets_one_normal_ack_turn() {
        let (mut adapter, peer) = after_oack();
        let id = adapter.oack_grant_outcome().intent.unwrap();
        assert_eq!(adapter.publish_first_grant_turn(1).unwrap(),
            (Transmit::Session(Published::ReceiveIntent(id)), 1));
        let trace = adapter.oack_grant_outcome();
        assert_eq!(trace.advertisement.unwrap().id, id);
        assert_eq!(trace.ack_granted, Some(true));
        let bytes = peer.take_tx();
        assert_eq!((bytes[0], bytes[16]), (5, 8));
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn blocked_extra_ack_keeps_pending_grant_for_normal_retry() {
        let (mut adapter, peer) = after_oack();
        for _ in 0..(FIFO_BYTES - 32) / 8 {
            adapter.ring.write(&[13, 0, 0, 0, 0, 0, 0, 0], FULL_RESERVE).unwrap();
        }
        let id = adapter.oack_grant_outcome().intent.unwrap();
        let (ordinary, extra) = adapter.publish_first_grant_turn(1).unwrap();
        assert_eq!(ordinary, Transmit::Session(Published::ReceiveIntent(id)));
        assert_eq!(extra, 1, "the bounded blocked-write notification counts as progress");
        assert!(adapter.oack_grant_outcome().ack.is_none());
        peer.take_tx();
        assert_eq!(adapter.publish_next(1).unwrap(),
            Transmit::Session(Published::IntentRequestAck { granted: true }));
        assert_eq!(adapter.oack_grant_outcome().ack_granted, Some(true));
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn extra_ack_head_faults_preserve_quarantine_without_claiming_commit() {
        for failure in ["ack_head_before", "ack_head_after"] {
            let (mut adapter, peer) = after_oack();
            peer.0.borrow_mut().fail = Some(failure);
            assert!(adapter.publish_first_grant_turn(1).is_err(), "{failure}");
            assert!(adapter.oack_grant_outcome().advertisement.is_some());
            assert!(adapter.oack_grant_outcome().ack.is_none());
            peer.0.borrow_mut().fail = None;
            assert!(adapter.publish_next(1).is_err(), "{failure}");
        }
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn unrelated_or_pre_oack_grant_does_not_borrow_a_turn() {
        for post_oack in [true, false] {
            let (mut adapter, _) = after_oack();
            if post_oack { adapter.ring.grant_reads.outcome.intent = Some(999); }
            else { adapter.ring.grant_reads.outcome.oack_captured = false; }
            let (ordinary, extra) = adapter.publish_first_grant_turn(1).unwrap();
            assert!(matches!(ordinary, Transmit::Session(Published::ReceiveIntent(_))));
            assert_eq!(extra, 0);
            assert!(adapter.oack_grant_outcome().ack.is_none());
        }
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn processed_original_close_prevents_extra_ack_turn() {
        let (mut adapter, peer) = after_oack();
        let mut close = std::vec::Vec::new();
        for value in [1u32, 6, 0, 0xffff_fffe, 0, 20, 1, 0xffff_fffe, 6, 0, 0x1f, 0, 0] {
            close.extend(value.to_le_bytes());
        }
        let available = adapter.session.grant_available_ids().into_iter().flatten().next().unwrap();
        peer.push(&crate::data(available, &close));
        adapter.poll_receive(1).unwrap();
        assert_eq!(adapter.session.original_1f_close_count(), Some(1));
        let (_, extra) = adapter.publish_first_grant_turn(1).unwrap();
        assert_eq!(extra, 0);
        assert!(adapter.oack_grant_outcome().ack.is_none());
    }

    #[cfg(feature = "sunfish-oack-grant-ack-turn")]
    #[test]
    fn pending_use_tail_fault_does_not_claim_consumption() {
        let (mut adapter, peer) = prepared(7, false);
        adapter.poll_receive(1).unwrap();
        let record = crate::query(1);
        let mut ids = [None; core::RX_SLOTS];
        ids[0] = Some(1);
        adapter.ring.grant_reads.capture_oack(ids, 0, record.len() as u32, Some(100));
        adapter.ring.grant_reads.start_pending(peer.0.borrow().words[2]);
        peer.push(&record);
        peer.0.borrow_mut().fail = Some("tail");
        assert!(adapter.poll_receive(1).is_err());
        let trace = adapter.oack_grant_outcome();
        assert!(trace.used[0].unwrap().clear.is_none());
        assert!(trace.pending_clear.is_none());
    }

    #[cfg(feature = "sunfish-oack-retry-close")]
    #[test]
    fn pending_metadata_matches_only_the_first_repeat_datagram() {
        let (mut adapter, peer) = after_oack();
        for first_repeat in [true, false] {
            let id = adapter.session.grant_available_ids().into_iter().flatten().next().unwrap();
            let body = crate::qcom_tftp_server_check::REQUEST;
            let mut packet = std::vec::Vec::new();
            for value in [1u32, 1, 0, 0x1f, 0, body.len() as u32, 1, 0x5000] { packet.extend(value.to_le_bytes()); }
            packet.extend(body);
            packet.resize((packet.len() + 3) & !3, 0);
            let record = crate::data(id, &packet);
            let mut ids = [None; core::RX_SLOTS]; ids[0] = Some(id);
            adapter.ring.grant_reads = grant_reads::GrantReadLedger::default();
            adapter.ring.grant_reads.capture_oack(ids, 0, record.len() as u32, Some(100));
            adapter.ring.grant_reads.start_pending(peer.0.borrow().words[2]);
            peer.push(&record);
            assert!(matches!(adapter.poll_receive(1).unwrap(), Receive::Session(Event::TftpRequest(_))));
            let trace = adapter.oack_grant_outcome();
            let meta = trace.pending_tftp[0].unwrap();
            assert!(meta.exact_wrq);
            assert_eq!(meta.first_repeat, first_repeat);
            assert!(trace.used[0].unwrap().clear.is_some());
        }
    }

    fn prepared(offers: usize, full: bool) -> (Adapter<crate::Fake>, crate::Fake) {
        let peer = crate::Fake::new(0, 0);
        let mut adapter = Adapter::attach(peer.clone(), crate::session(), 1).unwrap();
        for _ in 0..offers {
            adapter.offer_receive(1).unwrap();
            adapter.publish_next(1).unwrap();
            peer.take_tx();
        }
        if full {
            for _ in 0..(FIFO_BYTES - FULL_RESERVE) / 8 {
                adapter.ring.write(&[13, 0, 0, 0, 0, 0, 0, 0], FULL_RESERVE).unwrap();
            }
        }
        adapter.ring.first_server_check_clock = Some(clock);
        adapter.ring.grant_reads.clock_armed = true;
        adapter.ring.wrq_return_pending = Some(WrqReturnPending::new(0, 8, 0));
        peer.push(&[7, 0, 2, 0, 108, 0, 0, 0]);
        (adapter, peer)
    }

    #[test]
    fn negative_ack_is_matched_without_inventing_an_advertisement() {
        let (mut adapter, _) = prepared(8, false);
        assert_eq!(adapter.poll_receive(1).unwrap(),
            Receive::Session(Event::ReceiveIntentRequest { granted: false }));
        assert_eq!(adapter.publish_next(1).unwrap(),
            Transmit::Session(Published::IntentRequestAck { granted: false }));
        let trace = adapter.oack_grant_outcome();
        assert_eq!(trace.initial_granted, Some(false));
        assert_eq!(trace.ack_granted, Some(false));
        assert!(trace.ack.unwrap().published.is_some());
        assert!(trace.intent.is_none() && trace.advertisement.is_none());
    }

    #[test]
    fn blocked_advertisement_and_uncertain_ack_head_keep_capture_honest() {
        let (mut adapter, peer) = prepared(7, true);
        assert_eq!(adapter.poll_receive(1).unwrap(),
            Receive::Session(Event::ReceiveIntentRequest { granted: true }));
        assert!(matches!(adapter.publish_next(1).unwrap(), Transmit::Blocked { .. }));
        assert!(adapter.oack_grant_outcome().advertisement.is_none());
        {
            let mut memory = peer.0.borrow_mut();
            memory.words[0] = memory.words[1];
        }
        let intent = adapter.oack_grant_outcome().intent.unwrap();
        assert_eq!(adapter.publish_next(1).unwrap(),
            Transmit::Session(Published::ReceiveIntent(intent)));
        peer.0.borrow_mut().fail_after_store = true;
        assert!(adapter.publish_next(1).is_err());
        let trace = adapter.oack_grant_outcome();
        assert_eq!(trace.advertisement.unwrap().id, intent);
        assert!(trace.ack.is_none() && trace.ack_granted.is_none());
        assert!(adapter.publish_next(1).is_err());
    }
}

#[cfg(all(test, feature = "sunfish-first-oack-spare-buffer"))]
mod first_spare_fifo_tests {
    use super::*;
    #[test]
    fn blocked_fresh_advertisement_keeps_ordinary_fallback() {
        let (peer, owner, _, _) = crate::first_oack_spare_tests::prepared(false);
        let mut adapter = owner.into_spare_test_adapter();
        // Publish the WRQ return, then fill using actual checked writes so
        // cached and peer cursors describe the same legitimate history.
        for _ in 0..4 {
            if adapter.session.server_check_oacks()[0].unwrap().wrq_done_at.is_some() { break; }
            adapter.publish_next(1).unwrap();
        }
        peer.take_tx();
        while FIFO_BYTES - distance(adapter.ring.words[0], adapter.ring.words[1]) > 24 {
            adapter.ring.write(&[13,0,0,0,0,0,0,0], FULL_RESERVE).unwrap();
        }
        let (ordinary, extra) = adapter.publish_first_grant_turn(1).unwrap();
        assert!(matches!(ordinary, Transmit::Blocked { .. }));
        assert_eq!(extra, 0);
        assert!(adapter.oack_grant_outcome().primed_offer.is_none());
        assert_eq!(adapter.session.oack_resources(0x5001).unwrap().primed_intent, None);
        assert!(adapter.server_check_oacks()[0].unwrap().oack_at.is_none());
        peer.take_tx();
        let (ordinary, extra) = adapter.publish_first_grant_turn(1).unwrap();
        assert!(matches!(ordinary, Transmit::Session(Published::ReceiveIntent(_))));
        assert_eq!(extra, 1);
        assert!(adapter.oack_grant_outcome().primed_offer.is_some());
        assert!(adapter.server_check_oacks()[0].unwrap().oack_at.is_some());
    }
    #[test]
    fn processed_client_close_prevents_fresh_advertisement_and_extra_turn() {
        let (peer, owner, _, _) = crate::first_oack_spare_tests::prepared(false);
        let mut adapter = owner.into_spare_test_adapter();
        let id = adapter.session.grant_available_ids().into_iter().flatten().next().unwrap();
        peer.push(&crate::data(id, &crate::owner_control_frame(6, [0, 0x1f, 0, 0])));
        adapter.poll_receive(1).unwrap();
        let (_, extra) = adapter.publish_first_grant_turn(1).unwrap();
        assert_eq!(extra, 0);
        assert_eq!(adapter.session.oack_resources(0x5001).unwrap().primed_intent, None);
    }
    #[test]
    fn later_grant_cannot_rearm_priming_after_108_was_decoded() {
        let (_, owner, _, _) = crate::first_oack_spare_tests::prepared(false);
        let mut adapter = owner.into_spare_test_adapter();
        for size in [108, 52] {
            adapter.session.ingest(1, &crate::control(7, 2, size)).unwrap();
            let mut ack = false;
            for _ in 0..8 {
                let tx = adapter.publish_next(1).unwrap();
                if matches!(tx, Transmit::Session(Published::IntentRequestAck { .. })) { ack = true; break; }
            }
            assert!(ack);
            assert!(adapter.server_check_oacks()[0].unwrap().oack_at.is_none());
        }
        for _ in 0..8 { adapter.publish_first_grant_turn(1).unwrap(); }
        assert_eq!(adapter.session.oack_resources(0x5001).unwrap().primed_intent, None);
    }
}

#[cfg(all(test, feature = "sunfish-wrq-stage-costs"))]
mod wrq_stage_cost_tests {
    use super::*;
    #[cfg(feature="sunfish-wrq-post-return-callers")]
    #[test]
    fn actual_adapter_gate_propagates_its_callers_sample_identity() {
        use ::core::sync::atomic::{AtomicU64,Ordering};
        static CLOCK:AtomicU64=AtomicU64::new(1000);
        let (_,owner,_,_)=crate::first_oack_spare_tests::prepared(false);
        let mut a=owner.into_spare_test_adapter();
        a.ring.wrq_work.validation=WrqValidationWork::default();
        a.ring.first_server_check_clock=Some(||CLOCK.fetch_add(1,Ordering::Relaxed));
        let line=line!()+1;
        a.revalidate(1).unwrap();
        let c=a.ring.wrq_work.validation.callsite_costs([Some((0,u64::MAX)),None,None]);
        assert_eq!(c.len,1);assert_eq!(c.entries[0].site,SampleSite{file:SampleSite::file_id(file!()),line});
        assert_eq!(c.entries[0].count,[1,0,0]);assert_eq!(c.unknown_site,[0;3]);
    }
    #[test]
    fn wrq_checks_continue_after_registration_and_close_histories_finish() {
        use ::core::sync::atomic::{AtomicU64,Ordering};
        static CLOCK:AtomicU64=AtomicU64::new(1000);
        CLOCK.store(1000,Ordering::Relaxed);
        let (_,owner,_,_)=crate::first_oack_spare_tests::prepared(false);
        let mut a=owner.into_spare_test_adapter();
        a.ring.track_first_wrq=false;a.ring.track_ipa_announcement=false;
        a.ring.registration_rx.as_mut().unwrap().done=Some(ServerCheckRxDone {
            last_absent:Some(1),first_visible:Some(2),visible_at_close:Some(true),bytes_ahead:Some(0)});
        a.ring.init_close.close_end=Some(3);
        a.ring.first_server_check_clock=Some(||CLOCK.fetch_add(1,Ordering::Relaxed));
        let before=a.ring.wrq_work.checks(0,u64::MAX).0;
        a.ring.sample().unwrap();
        assert_eq!(a.ring.wrq_work.checks(0,u64::MAX).0,before+1);
    }
    #[test]
    fn stages_clip_nested_checks_and_record_spans_and_freeze_at_return() {
        let mut w = WrqReceiveWork::default();
        w.sample(5,15); w.sample(18,22); w.sample(25,35);
        w.record(4,16,0,&crate::control(7,2,52));
        w.record(17,24,1,&crate::control(8,1,1));
        w.record(24,34,1,&crate::control(14,1,0x20));
        w.finish(30);
        w.sample(31,40); w.record(31,40,1,&crate::control(5,1,1));
        let a=w.cost(10,20); let b=w.cost(20,30); let c=w.cost(30,40);
        assert_eq!((a.total,a.check_count,a.check_ticks,a.rx,a.tx,a.lost),(10,2,7,6,3,0));
        assert_eq!((b.total,b.check_count,b.check_ticks,b.rx,b.tx,b.lost),(10,2,7,0,10,0));
        assert_eq!((c.total,c.check_count,c.rx,c.tx),(0,0,0,0));
        assert_eq!(w.records.records(0,100).count(),3);
        w.finish(40); assert_eq!(w.finished,Some(30));
    }
    #[test]
    fn overflow_reports_history_loss_and_qrtr_prefix_identity() {
        let mut w=WrqReceiveWork::default();
        for n in 0..100 {w.sample(n*2,n*2+1);}
        for n in 0..30 {w.record(n*2,n*2+1,0,&crate::control(7,2,n as u32));}
        assert_eq!(w.cost(0,200).lost,3);
        let mut p=crate::data(0x20,&crate::owner_control_frame(4,[0x31,0x201,0,0x1d]));
        w.record(201,202,0,&p);
        let r=w.records.records(201,202).last().unwrap();
        assert_eq!(r.protocol,Some([4,0xffff_fffe,0xffff_fffe,0x31]));
        p[8..12].copy_from_slice(&8u32.to_le_bytes());p.truncate(24);
        w.record(203,204,0,&p);
        assert_eq!(w.records.records(203,204).last().unwrap().protocol,None);
    }
}

#[cfg(feature = "sunfish-wrq-stage-costs")]
#[derive(Default)]
pub struct WrqStageCost {
    pub total: u64,
    pub check_count: u32,
    pub check_ticks: u64,
    pub rx: u64,
    pub tx: u64,
    pub lost: u32,
}

#[cfg(feature = "sunfish-wrq-stage-costs")]
impl WrqReceiveWork {
    fn finish(&mut self, at: u64) { self.finished.get_or_insert(at); }
    pub fn cost(&self, from: u64, to: u64) -> WrqStageCost {
        let to = to.min(self.finished.unwrap_or(to));
        if to <= from { return WrqStageCost::default(); }
        let (check_count, check_ticks, lost) = self.checks(from, to);
        let mut cost = WrqStageCost { total: to-from, check_count, check_ticks,
            lost: u32::from(self.records.lost(from)) | (u32::from(lost)<<1), ..Default::default() };
        for r in self.records.records(from,to) {
            let overlap=r.end.min(to).saturating_sub(r.start.max(from));
            match r.kind {0=>cost.rx+=overlap, 1|2|3|6=>cost.tx+=overlap, _=>{}}
        }
        cost
    }
}

#[cfg(feature = "sunfish-wrq-validation-path")]
#[path = "qcom_glink_wrq_cost.rs"]
mod wrq_validation_cost;
#[cfg(feature = "sunfish-wrq-validation-path")]
pub use wrq_validation_cost::*;

#[cfg(all(test, feature = "sunfish-registry-post-reply-trace"))]
#[test]
fn registry_post_reply_window_retains_tail_visibility_and_reports_history_loss() {
    for clocked in [false, true] {
        let time = |n| clocked.then_some(n);
        let mut w = RegistryReplyWindow::new(40, 0, time(10));
        w.sample(16, 0, 0, time(20), time(21));
        assert_eq!(w.trace.peer_tail_at, None);
        w.sample(24, 80, 80, time(30), time(31));
        assert_eq!(w.trace.peer_tail_at, time(31));
        w.sample(0, 0, 8, time(40), time(41));
        w.finish(8, 8);
        let bounds = w.trace.completion.unwrap();
        assert_eq!((bounds.last_absent, bounds.first_visible, bounds.bytes_ahead), (time(20), time(31), Some(72)));
        assert!(w.trace.finished);
        let frozen = w.trace;
        w.sample(1, 8, 8, time(50), time(51)); w.finish(8,8);
        assert_eq!(w.trace, frozen);
    }
    let mut w = RegistryReplyWindow::new(8, 0, Some(1));
    w.sample(8, 8, 8, Some(2), Some(3));
    for i in 0..RX_HEAD_HISTORY { w.sample(0, 8, 8, Some(4+i as u64), Some(5+i as u64)); }
    assert_eq!(w.trace.history_evictions, 1);
    // Original completion was not parsed until after its arrival span expired.
    // Missing visibility is explicit; never substitute the latest observation.
    w.finish(8*(RX_HEAD_HISTORY+1),8);
    assert_eq!(w.trace.completion.unwrap().first_visible, None);
    assert_eq!(w.trace.peer_tail_at, Some(3));
}

#[cfg(all(test,feature="sunfish-registry-prearm-io-trace"))]
#[test]
fn registry_prearm_io_delta_rejects_missing_saturated_regressed_or_overflowed_counters(){
    let mut t=RegistryPrearmIo::default();assert_eq!(t.total(true),None);
    t.queued=Some([10;12]);let mut b=[u64::MAX;12];b[..4].copy_from_slice(&[11,12,13,14]);t.arm=Some(b);
    assert_eq!(t.delta(true),Some([1,2,3,4]));assert_eq!(t.total(true),Some(10)); // nested categories are not added
    t.arm.as_mut().unwrap()[1]=9;assert_eq!(t.delta(true),None);
    t.arm=Some([u64::MAX;12]);assert_eq!(t.delta(true),None);
    t.queued=Some([0;12]);t.arm=Some([u64::MAX-1;12]);assert_eq!(t.total(true),None);
}

#[cfg(all(test,feature="sunfish-registry-tx3-wire-headers"))]
#[test]
fn registry_wire_header_projects_checked_ids_and_leaves_short_or_unknown_details_unavailable(){
    let mut r=[0u8;56];
    for (at,v) in [(0,0x0001_0009u32),(4,7),(8,40),(16,1),(20,1),(24,1),(28,0x4003),(36,7),(44,9)]{r[at..at+4].copy_from_slice(&v.to_le_bytes());}
    r[48..55].copy_from_slice(&[2,3,0,0x21,0,0,0]);let h=RegistryWireHeader::read(&r,true,Some(100)).unwrap();
    assert_eq!((h.command,h.channel,h.parameter,h.bytes),(9,1,7,56));assert_eq!(h.qrtr,Some([1,0x4003,9]));assert_eq!(h.qmi,Some((2,3,0x21)));assert_eq!(h.at,Some(100));assert!(h.tx);
    r[8..12].copy_from_slice(&u32::MAX.to_le_bytes());assert_eq!(RegistryWireHeader::read(&r,false,None).unwrap().qrtr,None);
    let mut close=[0u8;68];for(at,v)in[(0,0x0002_0009u32),(4,20),(8,52),(16,1),(20,6),(28,u32::MAX),(36,20),(44,u32::MAX),(48,6),(52,0),(56,0x49)]{close[at..at+4].copy_from_slice(&v.to_le_bytes());}
    assert_eq!(RegistryWireHeader::read(&close,false,None).unwrap().del_client,Some([0,0x49]));
    for len in 0..8{assert!(RegistryWireHeader::read(&r[..len],false,None).is_none());}
    assert!(RegistryWireHeader::read(&std::vec![0;65536],false,None).is_none());
}
#[cfg(all(test,feature="sunfish-registry-tx3-wire-headers"))]
#[test]
fn registry_wire_trace_keeps_opening_tail_and_clockless_completion_without_late_overwrite(){
    let mut t=RegistryWireTrace::default();let h=RegistryWireHeader::read(&[6,0,2,0,0,0,0,0],false,None).unwrap();
    for parameter in 0..100{t.record(Some(RegistryWireHeader{parameter,..h}),false);}
    let kept:std::vec::Vec<_>=t.headers.iter().flatten().map(|h|h.parameter).collect();assert_eq!(kept,(0..40).chain(61..100).collect::<std::vec::Vec<_>>());assert_eq!(t.omitted,21);
    t.record(Some(RegistryWireHeader{parameter:100,..h}),true);assert!(t.finished);assert_eq!(t.completion.unwrap().parameter,100);assert_eq!(t.completion.unwrap().at,None);
    t.record(Some(h),false);t.record(None,true);assert_eq!(t.omitted,21);assert_eq!(t.unparsed,0);assert_eq!(t.completion.unwrap().parameter,100);
    let mut t=RegistryWireTrace::default();t.record(None,true);assert!(t.finished);assert_eq!(t.unparsed,1);assert_eq!(t.completion,None);t.record(Some(h),false);assert!(t.headers.iter().all(Option::is_none));
    let mut t=RegistryWireTrace::default();t.unparsed=u32::MAX;t.record(None,false);assert_eq!(t.unparsed,u32::MAX);
}

#[cfg(all(test,mpss_glink_transport_tests,feature="sunfish-glink-announcement-reuse"))]
mod announcement_reuse_fifo_tests {
use super::*;
use crate::{Fake,Op};
#[test]
fn announcement_reuse_fifo_ack_keeps_head_unpublished_for_visible_or_late_rx(){
    for wrap in [0,0x3ff8]{for mode in 0..3{
        let(mut s,id,_original)=crate::announcement_fixture::ready();
        let mut packet=Vec::new();for value in [1u32,4,0,0xfffffffe,0,20,1,0xfffffffe,4,0x777,1,0,0x123]{packet.extend(value.to_le_bytes());}
        assert!(matches!(s.ingest(1,&crate::data(id,&packet)),Ok(Event::QrtrControl(_))));
        let mut returned=false;for _ in 0..32{let result=s.publish_next(1,|b|{if b[0]==14{assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()),id);returned=true;}Ok(())}).unwrap();if returned{break;}assert!(result.is_some());}assert!(returned);
        assert_eq!(s.ingest(1,&crate::control(7,2,52)),Ok(Event::ReceiveIntentRequest{granted:true}));
        let peer=Fake::new(wrap,wrap);let mut a=Adapter::attach(peer.clone(),crate::session(),1).unwrap();
        // The pure Session fixture supplies a validated logical prefix; this
        // test isolates the actual FIFO publication boundary, not boot setup.
        a.session=s;peer.clear();
        match mode{0=>peer.push(&crate::control(13,0,0)),1=>{let mut fragment=crate::data(id,&packet[..8]);fragment[12..16].copy_from_slice(&44u32.to_le_bytes());peer.push(&fragment);},_=>peer.0.borrow_mut().append_notification_on_write=true}
        assert_eq!(a.publish_next(1),Ok(Transmit::Idle));assert_eq!(peer.0.borrow().words[1],wrap);
        assert!(!peer.log().iter().any(|op|matches!(op,Op::Head(_))),"RX-visible ACK has not committed");
        if mode==1{assert_eq!(a.poll_receive(1),Ok(Receive::Session(Event::Fragment)));}else{assert_eq!(a.poll_receive(1),Ok(Receive::ReadNotification));}
        assert_eq!(a.publish_next(1),Ok(Transmit::Session(Published::IntentRequestAck{granted:mode!=1})));
        let bytes=peer.take_tx();assert_eq!((bytes[0],u32::from_le_bytes(bytes[4..8].try_into().unwrap())),(8,u32::from(mode!=1)));
    }}
}
}

#[cfg(all(test,feature="sunfish-registry-tftp-wire"))]
#[test]
fn registry_tftp_identity_projects_actual_server_replies_and_bounds_unknown_packets(){
    fn record(body:&[u8],tx:bool,local:u32,peer:u32)->std::vec::Vec<u8>{
        let mut packet=std::vec::Vec::new();
        for value in [1,1,if tx{1}else{0},if tx{local}else{peer},0,body.len()as u32,if tx{0}else{1},if tx{peer}else{local}]{packet.extend(value.to_le_bytes());}
        packet.extend(body);packet.resize((packet.len()+3)&!3,0);
        let mut r=std::vec::Vec::new();r.extend(9u16.to_le_bytes());r.extend((if tx{1u16}else{2u16}).to_le_bytes());r.extend(7u32.to_le_bytes());r.extend((packet.len()as u32).to_le_bytes());r.extend(0u32.to_le_bytes());r.extend(packet);r.resize((r.len()+7)&!7,0);r
    }
    use crate::qcom_tftp_server_check::{ServerCheck,Route,Address,REQUEST};
    let mut server=ServerCheck::new();let mut out=[0u8;512];let incoming=Route{source:Address{node:0,port:0x49},destination:Address{node:1,port:0x5000}};
    let request=record(REQUEST,false,0x5000,0x49);let h=RegistryWireHeader::read(&request,false,None).unwrap();assert_eq!(h.tftp,Some([0x0002ffff,0x01000049]));
    let reply=server.receive(incoming,REQUEST,&mut out).unwrap();assert_eq!(reply.len,39);
    let oack=record(&out[..reply.len],true,reply.route.source.port,0x49);assert_eq!(oack.len(),88);assert_eq!(RegistryWireHeader::read(&oack,true,None).unwrap().tftp,Some([0x0006ffff,39]));
    let missing=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";
    let mut second=incoming;second.source.port=0x4a;let reply=server.receive(second,missing,&mut out).unwrap();assert_eq!(reply.len,19);
    let error=record(&out[..reply.len],true,reply.route.source.port,0x4a);assert_eq!(error.len(),72);assert_eq!(RegistryWireHeader::read(&error,true,None).unwrap().tftp,Some([0x00050001,19]));
    assert_eq!(RegistryWireHeader::read(&record(missing,false,0x5000,0x4a),false,None).unwrap().tftp,Some([0x0001ffff,0x03000000|missing.len()as u32]));
    for(opcode,number,body)in [(3,7,&b"\0\x03\0\x07ab"[..]),(4,3,&b"\0\x04\0\x03"[..]),(5,1,&b"\0\x05\0\x01\0"[..])]{let h=RegistryWireHeader::read(&record(body,true,0x5100,0x49),true,None).unwrap();assert_eq!(h.tftp,Some([(opcode<<16)|number,body.len()as u32]));}
    assert_eq!(RegistryWireHeader::read(&record(REQUEST,false,0x4003,0x5005),false,None).unwrap().tftp,None,"peer port alone never identifies local TFTP");
    for body in [&b"\0"[..],&b"\0\x03\0"[..],&b"\xff\xff\0\0"[..]]{assert_eq!(RegistryWireHeader::read(&record(body,true,0x5005,0x49),true,None).unwrap().tftp,None);}
    let mut partial=oack.clone();partial[12..16].copy_from_slice(&8u32.to_le_bytes());assert_eq!(RegistryWireHeader::read(&partial,true,None).unwrap().tftp,None);
    let mut invalid=oack.clone();invalid[36..40].copy_from_slice(&u32::MAX.to_le_bytes());assert_eq!(RegistryWireHeader::read(&invalid,true,None).unwrap().tftp,None);
    println!("Actual server-check OACK:39payload/88wire; missing-file error:19payload/72wire. Scalar request categories/block/error IDs, direction, fragments and bounds passed.");
}
