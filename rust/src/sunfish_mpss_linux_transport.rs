//! Selected Linux VERSION/IPCRTR opening over the retained live FIFO.
//! One bounded operation per step; no cursor initialization or reset/retry.
#![forbid(unsafe_code)]
use super::*;
use ::core::mem::ManuallyDrop;
// Keep unhandled OPEN bookkeeping bounded while accommodating more modem
// channels than the previous eight-slot limit observed on the device.
const MAX_OTHER_CHANNELS: usize = 64;
pub trait BootIo: Io {
    /// Consume the retained native memory owner's single transport claim.
    fn claim_boot(&mut self) -> Result<(), Fault>;
    /// Permanently quarantine the enclosing native owner without hardware
    /// cleanup. Protocol failures must not reopen another owner entry point.
    fn fault_boot(&mut self);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootPhase {
    New,
    Negotiating,
    Ready,
    Failed,
}
/// A bounded pass waits for its peer, reaches ready, or exhausts its budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootDrive {
    Idle,
    Ready,
    BudgetExhausted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootReport {
    pub phase: BootPhase,
    pub features: u32,
    pub remote_channel: Option<u16>,
    pub other_channels: usize,
    pub tx_records: u32,
    pub rx_records: u32,
    pub deadline: Option<u64>,
    pub error: Option<Error>,
}
#[derive(Clone, Copy)]
struct Channel {
    id: u16,
    len: usize,
    name: [u8; 32],
}
#[derive(Clone, Copy)]
pub(super) struct Channels {
    others: [Option<Channel>; MAX_OTHER_CHANNELS],
    pub remote: Option<u16>,
    pub features: u32,
}
#[derive(Clone, Copy)]
pub(super) struct Early {
    pub bytes: [u8; core::MAX_TX_RECORD_BYTES],
    pub len: usize,
}
pub(super) struct Runtime {
    pub channels: Channels,
    pub early: [Option<Early>; 8],
}
pub(super) fn open_length(b: &[u8]) -> Result<usize, Error> {
    let len = (word(b, 4) & 0xffff) as usize;
    if !(2..=32).contains(&len) {
        return Err(Error::Negotiation);
    }
    Ok((8 + len + 7) & !7)
}
impl Channels {
    fn new() -> Self {
        Self {
            others: [None; MAX_OTHER_CHANNELS],
            remote: None,
            features: 1,
        }
    }
    pub(super) fn open_diagnostic(&self, b: &[u8], stage: u32) -> OpenDiagnostic {
        let advertised_length = word(b, 4) & 0xffff;
        let mut name = [0u8; 24];
        let name_len = b.len().saturating_sub(8).min(advertised_length as usize).min(name.len());
        name[..name_len].copy_from_slice(&b[8..8 + name_len]);
        let id = u16::from_le_bytes([b[2], b[3]]);
        let mut reason = u32::from(open_length(b).is_err());
        if id == 0 { reason |= 1 << 1; }
        if self.remote == Some(id) { reason |= 1 << 4; }
        if self.others.iter().flatten().any(|c| c.id == id) { reason |= 1 << 5; }
        if let Some(full_name) = b.get(8..8 + advertised_length as usize) {
            if full_name.last() != Some(&0) { reason |= 1 << 2; }
            if !full_name[..full_name.len().saturating_sub(1)].iter().all(|v| (32..=126).contains(v)) { reason |= 1 << 3; }
            if self.others.iter().flatten().any(|c| c.name[..c.len] == *full_name) { reason |= 1 << 6; }
            if full_name == b"IPCRTR\0" { reason |= 1 << 7; }
            else if self.others.iter().all(Option::is_some) { reason |= 1 << 8; }
        }
        OpenDiagnostic {
            stage,
            channel: id as u32,
            advertised_length,
            reason,
            other_channels: self.others.iter().flatten().count() as u32,
            remote_channel: self.remote.map_or(u32::MAX, u32::from),
            name,
            name_len,
        }
    }
    pub(super) fn open(&mut self, b: &[u8], late: bool) -> Result<(), Error> {
        if b.len() != open_length(b)? || b[..2] != [2, 0] {
            return Err(Error::Negotiation);
        }
        let id = u16::from_le_bytes([b[2], b[3]]);
        let len = (word(b, 4) & 0xffff) as usize;
        let name = &b[8..8 + len];
        if id == 0
            || name[len - 1] != 0
            || !name[..len - 1].iter().all(|v| (32..=126).contains(v))
            || self.remote == Some(id)
            || self
                .others
                .iter()
                .flatten()
                .any(|c| c.id == id || c.name[..c.len] == *name)
        {
            return Err(Error::Negotiation);
        }
        if name == b"IPCRTR\0" {
            if late || self.remote.is_some() {
                return Err(Error::Negotiation);
            }
            self.remote = Some(id);
        } else {
            let slot = self
                .others
                .iter_mut()
                .find(|c| c.is_none())
                .ok_or(Error::Negotiation)?;
            let mut copy = [0; 32];
            copy[..len].copy_from_slice(name);
            *slot = Some(Channel {
                id,
                len,
                name: copy,
            });
        }
        // Like Linux without an interested rpmsg consumer, unconfigured OPENs
        // are remembered but never ACKed or granted buffers/channels.
        Ok(())
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Sent {
    Version,
    VersionAck,
    Open,
    OpenAck,
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct Packet {
    bytes: [u8; 48],
    len: usize,
    kind: Sent,
}
impl Packet {
    fn new(command: u16, id: u16, value: u32, kind: Sent) -> Self {
        let mut p = Self {
            bytes: [0; 48],
            len: 8,
            kind,
        };
        p.bytes[..2].copy_from_slice(&command.to_le_bytes());
        p.bytes[2..4].copy_from_slice(&id.to_le_bytes());
        p.bytes[4..8].copy_from_slice(&value.to_le_bytes());
        p
    }
}
/// Owns the actual memory I/O and original prepared session until handoff.
/// Both remain retained on error/unwind/drop. No mutable I/O/session escape.
pub struct BootTransport<I: BootIo> {
    ring: ManuallyDrop<Ring<I>>,
    session: ManuallyDrop<Session>,
    channels: Channels,
    report: BootReport,
    claimed: bool,
    initialized: bool,
    busy: bool,
    pending: [Option<Packet>; 4],
    version_sent: bool,
    peer_version: bool,
    version_acked: bool,
    open_queued: bool,
    open_sent: bool,
    open_acked: bool,
    remote_acked: bool,
    ticks: u64,
    last_now: Option<u64>,
    early: [Option<Early>; 8],
}
impl<I: BootIo> BootTransport<I> {
    #[allow(dead_code)] // Consumed by the combined runtime when it is enabled.
    pub fn with_io<T>(&mut self, action: impl FnOnce(&mut I) -> T) -> T {
        action(&mut self.ring.io)
    }

    pub fn new(io: I, session: Session, generation: u64, hz: u64) -> Self {
        let mut s = Self {
            ring: ManuallyDrop::new(Ring {
                io,
                generation,
                identity: 0,
                words: [0; 8],
                rx_bytes: None,
                #[cfg(feature = "sunfish-ipa-init-close-visibility")]
                init_close: Default::default(),
                #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
                reg_close: None,
                #[cfg(feature = "sunfish-ipa-reg-close-fifo")]
                reg_pre_unread: None,
                #[cfg(feature = "sunfish-ipa-registration-reply-path")]
                registration_reply: None,
                #[cfg(feature = "sunfish-ipa-reply-fifo-visibility")]
                registration_reply_rx: None,
                #[cfg(feature = "sunfish-registry-post-reply-trace")]
                registry_reply: None,
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
                reserved_writer_io: super::ReservedWriterIoTrace::default(),
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
                #[cfg(feature = "sunfish-grant-rx-open")]
                grant_receive: Default::default(),
                #[cfg(feature = "sunfish-wrq-return-pending")]
                wrq_return_pending: None,
                read_notification_end: None,
                record: [0; MAX_RX_RECORD],
                control: None,
                open_diagnostic: None,
            }),
            session: ManuallyDrop::new(session),
            channels: Channels::new(),
            report: BootReport {
                phase: BootPhase::New,
                features: 1,
                remote_channel: None,
                other_channels: 0,
                tx_records: 0,
                rx_records: 0,
                deadline: None,
                error: None,
            },
            claimed: false,
            initialized: false,
            busy: false,
            pending: [None; 4],
            version_sent: false,
            peer_version: false,
            version_acked: false,
            open_queued: false,
            open_sent: false,
            open_acked: false,
            remote_acked: false,
            ticks: hz.saturating_mul(5),
            last_now: None,
            early: [None; 8],
        };
        if generation == 0
            || !(1_000_000..=1_000_000_000).contains(&hz)
            || s.session.check(generation).is_err()
            || !s.session.transport_is_pristine()
            || s.session.control_status().started
        {
            s.fail(Error::Negotiation);
        }
        s
    }
    fn fail(&mut self, e: Error) -> Error {
        self.ring.io.fault_boot();
        self.report.phase = BootPhase::Failed;
        self.report.error.get_or_insert(e);
        e
    }
    pub fn report(&self) -> BootReport {
        let mut r = self.report;
        r.features = self.channels.features;
        r.remote_channel = self.channels.remote;
        r.other_channels = self.channels.others.iter().flatten().count();
        if self.busy && r.error.is_none() {
            r.phase = BootPhase::Failed;
            r.error = Some(Error::Quarantined);
        }
        r
    }
    fn queue(&mut self, p: Packet) -> Result<(), Error> {
        *self
            .pending
            .iter_mut()
            .find(|v| v.is_none())
            .ok_or(Error::Negotiation)? = Some(p);
        Ok(())
    }
    fn plan_open(&mut self) -> Result<(), Error> {
        if !self.peer_version || !self.version_acked {
            return Ok(());
        }
        if !self.open_queued {
            self.open_queued = true;
            if let Some(id) = self.channels.remote {
                self.queue(Packet::new(4, id, 0, Sent::OpenAck))?;
            }
            let mut p = Packet::new(2, 1, 7, Sent::Open);
            p.bytes[8..15].copy_from_slice(b"IPCRTR\0");
            p.len = 16;
            self.queue(p)?;
        } else if self.open_sent && self.open_acked && !self.remote_acked {
            if let Some(id) = self.channels.remote {
                self.queue(Packet::new(4, id, 0, Sent::OpenAck))?;
            }
        }
        Ok(())
    }
    fn peek_control(&mut self) -> Result<Option<usize>, Error> {
        self.ring.sample()?;
        let Some(length) = self.ring.rx_bytes else {
            return Ok(None);
        };
        let available = ring_distance(self.ring.words[2], self.ring.words[3], length);
        if available < 8 {
            return Ok(None);
        }
        let mut first = [0; 8];
        self.ring.read_rx_at(0, &mut first)?;
        let cmd = u16::from_le_bytes([first[0], first[1]]);
        let size = match cmd {
            0 | 1 | 4 | 7 | 13 => 8,
            2 => open_length(&first)?,
            5 => {
                let count = word(&first, 4) as usize;
                if count == 0 || count > core::REMOTE_INTENTS {
                    return Err(Error::InvalidRecord);
                }
                8 + count * 8
            }
            _ => return Err(Error::Negotiation),
        };
        if available < size {
            return Ok(None);
        }
        // Two bounded private copies, then revalidate peer progress/identity.
        let mut a = [0; core::MAX_TX_RECORD_BYTES];
        let mut b = [0; core::MAX_TX_RECORD_BYTES];
        self.ring.read_rx_at(0, &mut a[..size])?;
        self.ring.read_rx_at(0, &mut b[..size])?;
        if a != b || a[..8] != first {
            return Err(Error::ChangedRecord);
        }
        self.ring.sample()?;
        self.ring.record[..size].copy_from_slice(&a[..size]);
        Ok(Some(size))
    }
    fn ingest(&mut self, n: usize) -> Result<bool, Error> {
        let b = &self.ring.record[..n];
        let cmd = u16::from_le_bytes([b[0], b[1]]);
        let id = u16::from_le_bytes([b[2], b[3]]);
        let value = word(b, 4);
        match cmd {
            0 => {
                if id == 0 {
                    return Err(Error::Negotiation);
                }
                if id == 1 {
                    self.channels.features &= value;
                    self.peer_version = true;
                }
                self.queue(Packet::new(1, 1, self.channels.features, Sent::VersionAck))?;
            }
            1 => {
                if id == 0 || !self.version_sent {
                    return Err(Error::Negotiation);
                }
                if id == 1 && value == self.channels.features {
                    self.version_acked = true;
                } else {
                    self.channels.features &= value;
                    self.version_acked = false;
                    self.queue(Packet::new(0, 1, self.channels.features, Sent::Version))?;
                }
            }
            2 => self.channels.open(b, false)?,
            4 => {
                if id != 1 || value != 0 || !self.open_sent || self.open_acked {
                    return Err(Error::Negotiation);
                }
                self.open_acked = true;
            }
            5 | 7 => {
                if self.channels.remote != Some(id) || !self.remote_acked {
                    return Err(Error::Negotiation);
                }
                let mut bytes = [0; core::MAX_TX_RECORD_BYTES];
                bytes[..n].copy_from_slice(b);
                *self
                    .early
                    .iter_mut()
                    .find(|s| s.is_none())
                    .ok_or(Error::Negotiation)? = Some(Early { bytes, len: n });
            }
            13 => {
                if id != 0 || value != 0 {
                    return Err(Error::InvalidRecord);
                }
                return Ok(true);
            }
            _ => return Err(Error::Negotiation),
        }
        Ok(false)
    }
    fn inner(&mut self, now: u64) -> Result<BootPhase, Error> {
        if self
            .last_now
            .is_some_and(|last| now.wrapping_sub(last) > u64::MAX / 2)
        {
            return Err(Error::Timeout);
        }
        self.last_now = Some(now);
        let deadline = *self
            .report
            .deadline
            .get_or_insert_with(|| now.wrapping_add(self.ticks));
        if now.wrapping_sub(deadline) < u64::MAX / 2 {
            return Err(Error::Timeout);
        }
        if !self.claimed {
            self.claimed = true;
            self.ring.io.claim_boot()?;
        }
        if !self.initialized {
            let o = self.ring.io.observe()?;
            if !o.ready && !o.fatal && o.generation == self.ring.generation {
                return Ok(BootPhase::New);
            }
            ready(o, self.ring.generation)?;
            if o.identity == 0 {
                return Err(Error::IdentityChanged);
            }
            let rx = self.ring.io.rx_length()?;
            let words = descriptors(&mut self.ring.io, rx)?;
            if words[0] != 0 || words[1] != 0 || words[2] != 0 || words[4..] != [0; 4] {
                return Err(Error::Negotiation);
            }
            self.ring.identity = o.identity;
            self.ring.words = words;
            self.ring.rx_bytes = rx;
            self.ring.sample()?;
            self.initialized = true;
            self.report.phase = BootPhase::Negotiating;
            self.queue(Packet::new(0, 1, self.channels.features, Sent::Version))?;
        }
        let o = self.ring.io.observe()?;
        if !o.ready
            && !o.fatal
            && o.generation == self.ring.generation
            && o.identity == self.ring.identity
        {
            return Ok(self.report.phase);
        }
        ready(o, self.ring.generation)?;
        if o.identity != self.ring.identity {
            return Err(Error::IdentityChanged);
        }
        if let Some(packet) = self.pending[0] {
            match self.ring.write(&packet.bytes[..packet.len], TX_RESERVE) {
                Ok(()) => {}
                Err(Error::WouldBlock) => {
                    self.ring.blocked_notice()?;
                    return Ok(self.report.phase);
                }
                Err(e) => return Err(e),
            }
            match packet.kind {
                Sent::Version => self.version_sent = true,
                Sent::Open => self.open_sent = true,
                Sent::OpenAck => self.remote_acked = true,
                Sent::VersionAck => {}
            }
            self.report.tx_records += 1;
            self.pending.rotate_left(1);
            self.pending[3] = None;
        } else if self.version_acked
            && self.peer_version
            && self.open_sent
            && self.open_acked
            && self.remote_acked
        {
            self.ring.sample()?;
            self.report.phase = BootPhase::Ready;
            self.report.deadline = None;
        } else {
            self.plan_open()?;
            if self.pending[0].is_none() {
                if let Some(size) = self.peek_control()? {
                    let notify = self.ingest(size)?;
                    self.ring.consume(size, notify)?;
                    self.report.rx_records += 1;
                }
            }
        }
        Ok(self.report.phase)
    }
    pub fn step(&mut self, now: u64) -> Result<BootPhase, Error> {
        if self.busy || self.report.error.is_some() {
            return Err(self.fail(Error::Quarantined));
        }
        if self.report.phase == BootPhase::Ready {
            return Ok(BootPhase::Ready);
        }
        self.busy = true;
        match self.inner(now) {
            Ok(p) => {
                self.busy = false;
                Ok(p)
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    /// Drain locally actionable opening work without needing a new IRQ for
    /// each queued reply. The caller must handle budget exhaustion safely.
    pub fn drive(&mut self, now: u64, budget: usize) -> Result<BootDrive, Error> {
        for _ in 0..budget {
            let before = (
                self.report(), self.claimed, self.initialized, self.pending,
                (self.version_sent, self.peer_version, self.version_acked,
                 self.open_queued, self.open_sent, self.open_acked, self.remote_acked),
                self.ring.words, self.ring.rx_bytes,
            );
            if self.step(now)? == BootPhase::Ready {
                return Ok(BootDrive::Ready);
            }
            let after = (
                self.report(), self.claimed, self.initialized, self.pending,
                (self.version_sent, self.peer_version, self.version_acked,
                 self.open_queued, self.open_sent, self.open_acked, self.remote_acked),
                self.ring.words, self.ring.rx_bytes,
            );
            if before == after {
                return Ok(BootDrive::Idle);
            }
        }
        Ok(BootDrive::BudgetExhausted)
    }
    /// Move the same rings and original unpublished session into runtime.
    /// An error returns this retained terminal owner; nothing is discarded.
    pub fn into_adapter(mut self) -> Result<Adapter<I>, Self> {
        if self.busy || self.report.error.is_some() || self.report.phase != BootPhase::Ready {
            self.fail(Error::Negotiation);
            return Err(self);
        }
        self.busy = true;
        if let Err(e) = self.ring.sample().and_then(|_| {
            self.session.bind_native_channel(
                self.ring.generation,
                1,
                self.channels.remote.ok_or(Error::Negotiation)?,
            )?;
            Ok(())
        }) {
            self.fail(e);
            return Err(self);
        }
        self.ring.control = Some(Runtime {
            channels: self.channels,
            early: self.early,
        });
        Ok(Adapter {
            #[cfg(feature = "sunfish-first-oack-grant-window")]
            first_grant: super::FirstGrantTrace::default(),
            ring: ManuallyDrop::into_inner(self.ring),
            session: ManuallyDrop::into_inner(self.session),
            quarantined: false,
            session_failure: None,
        })
    }
}
