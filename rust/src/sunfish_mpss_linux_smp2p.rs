//! Selected Linux cold modem SMP2P, retaining the actual prepared SMEM owner.
//! No PAS/reset operation, clock vote, modem recovery or GLINK publication.
#![forbid(unsafe_code)]
use crate::{
    qcom_modem_status::{self as dir, Item},
    sunfish_ipa_smem as ipa,
    sunfish_mpss_linux_smem::{self as memory, Extension, Payload, Prepared},
};
use core::mem::ManuallyDrop;
pub const BYTES: usize = 340;
pub const AP: Item = memory::ITEMS[0].1;
pub const PEER_ID: u16 = 435;
pub const DOORBELL: u32 = 1 << 14;
/// Read the complete bounded SMP2P item; callers retain the existing barriers,
/// lock and, for peer data, independent snapshot comparisons.
pub(crate) fn read_item(m: &impl dir::Read, offset: usize, out: &mut [u8; BYTES]) {
    for (n, chunk) in out.chunks_exact_mut(4).enumerate() {
        chunk.copy_from_slice(&m.read_le32(offset + n * 4).to_le_bytes());
    }
}
pub const INIT: [(usize, u32); 16] = [
    (0, 0x504d5324),
    (4, 0x100),
    (8, 0x10000),
    (12, 16),
    (16, 0),
    (4, 0x102),
    (20, 0x7473616d),
    (24, 0x6b2d7265),
    (28, 0x656e7265),
    (32, 0x6c),
    (12, 0x10010),
    (40, 0x617069),
    (44, 0),
    (48, 0),
    (52, 0),
    (12, 0x20010),
];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Consumed,
    Predecessor,
    Guard,
    LockBusy,
    LockLost,
    Directory,
    Layout,
    Outbound,
    Readback,
    Store,
    Notify,
    PeerHeader,
    PeerVersion,
    PeerNames,
    PeerFatal,
    PeerStopped,
    PeerRestart,
    PeerIdentity,
}

#[cfg(all(test, mpss_smp2p_tests))]
#[path = "../../tests/sunfish_mpss_linux_smp2p_scope.rs"]
mod native_scope;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Lifecycle {
    PreAuth,
    AuthSubmitted,
    AuthReturned,
    Running,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Power {
    Inactive,
    Active,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub generation: u64,
    pub lifecycle: Lifecycle,
    pub power: Power,
}
/// Actual platform lifecycle and power owner, not packet-supplied flags.
/// The cold boot must exclude all resets/remaps; runtime failures are terminal.
/// A sampled active IPA power reference must remain held through publication
/// and subsequent use. No concurrent host may withdraw or replace that lease.
pub trait Signals {
    fn observe(&mut self, generation: u64) -> Result<Observation, Error>;
    fn notify(&mut self) -> Result<(), Error>;
}
#[cfg(feature = "sunfish-linux-mpss-glink")]
pub trait GlinkSignals: Signals {
    /// Same retained APCS writer; fixed GLINK bit 12, never a second mapping.
    fn notify_glink(&mut self) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    Init,
    Version,
    Features,
    RestartAck,
    IpaEnabled,
    IpaValid,
}
pub struct Write {
    at: usize,
    value: u32,
    field: Field,
}
impl Write {
    pub fn at(&self) -> usize {
        self.at
    }
    pub fn value(&self) -> u32 {
        self.value
    }
    pub fn field(&self) -> Field {
        self.field
    }
}
/// Additional closed AP428 stores only. Claimed once after the native payload
/// writer completed the verified allocation/zero sequence. No raw extraction.
pub trait SmpIo: Payload {
    fn claim_smp2p(&mut self) -> Result<(), Error>;
    fn smp_store(&mut self, request: &Write) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Initialize,
    Active,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Poll {
    Waiting,
    Unstable,
    VersionChanged,
    Observed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Peer {
    pub version: u8,
    pub features: u32,
    pub flags: u32,
    pub valid: u16,
    pub slave_kernel: Option<u32>,
    pub ipa: Option<u32>,
    names: [[u8; 16]; 16],
}
fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn half(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
pub fn decode(b: &[u8; BYTES]) -> Result<Option<Peer>, Error> {
    let version = b[4];
    if version == 0 {
        return Ok(None);
    }
    if version > 2 {
        return Err(Error::PeerVersion);
    }
    let valid = half(b, 14);
    if word(b, 0) != 0x504d5324
        || half(b, 8) != 1
        || half(b, 10) != 0
        || half(b, 12) != 16
        || valid > 16
    {
        return Err(Error::PeerHeader);
    }
    let mut peer = Peer {
        version,
        features: word(b, 4) >> 8,
        flags: word(b, 16),
        valid,
        slave_kernel: None,
        ipa: None,
        names: [[0; 16]; 16],
    };
    for i in 0..valid as usize {
        let at = 20 + i * 20;
        let name: &[u8] = &b[at..at + 16];
        let n = name.iter().position(|b| *b == 0).unwrap_or(16);
        if n == 0 || !name[..n].iter().all(|b| (32..=126).contains(b)) {
            return Err(Error::PeerNames);
        }
        peer.names[i].copy_from_slice(name);
        if name[..n] == *b"slave-kernel" {
            if peer.slave_kernel.replace(word(b, at + 16)).is_some() {
                return Err(Error::PeerNames);
            }
        }
        if name[..n] == *b"ipa" {
            if peer.ipa.replace(word(b, at + 16)).is_some() {
                return Err(Error::PeerNames);
            }
        }
    }
    Ok(Some(peer))
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub error: Option<Error>,
    pub initialized: usize,
    pub submitted: u32,
    pub completed: u32,
    pub kicks_sent: u32,
    pub kicks_done: u32,
    pub samples: u32,
    pub unstable: u32,
    pub version: u8,
    pub features: u32,
    pub negotiated: bool,
    pub peer: Option<Peer>,
    pub peer_item: Option<Item>,
    pub ready_seen: bool,
    pub handover_seen: bool,
    pub setup_seen: bool,
    pub power_notified: bool,
    pub power_reported: Option<Power>,
    pub restart_acks: u32,
}
/// Stable peer ready value after authentication was submitted. This is not a
/// claim of physical IRQ delivery, firmware authentication or a WLAN permit.
pub struct ReadyObservation {
    generation: u64,
}
impl ReadyObservation {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
pub struct Controller<I: ipa::Io, P: SmpIo, S: Signals> {
    #[cfg(feature = "sunfish-linux-mpss-glink")]
    glink: Option<glink::State>,
    memory: ManuallyDrop<Extension<I, P>>,
    signals: ManuallyDrop<S>,
    generation: u64,
    report: Report,
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    crash_evidence: crate::qcom_modem_crash::Evidence,
    out: [u8; BYTES],
    busy: bool,
    started: bool,
    lifecycle: Lifecycle,
    publishing_power: Option<Power>,
    ready_taken: bool,
    free_uncached: usize,
    free_cached: usize,
}
impl<I: ipa::Io, P: SmpIo, S: Signals> Controller<I, P, S> {
    pub fn new(mut memory: Extension<I, P>, receipt: Prepared, signals: S) -> Self {
        let result = memory.claim_smp2p(receipt);
        let generation = result.unwrap_or(0);
        let error = if result.is_err() {
            Some(Error::Predecessor)
        } else {
            None
        };
        Self {
            #[cfg(feature = "sunfish-linux-mpss-glink")]
            glink: None,
            memory: ManuallyDrop::new(memory),
            signals: ManuallyDrop::new(signals),
            generation,
            report: Report {
                phase: if error.is_none() {
                    Phase::Initialize
                } else {
                    Phase::Failed
                },
                error,
                initialized: 0,
                submitted: 0,
                completed: 0,
                kicks_sent: 0,
                kicks_done: 0,
                samples: 0,
                unstable: 0,
                version: 2,
                features: 1,
                negotiated: false,
                peer: None,
                peer_item: None,
                ready_seen: false,
                handover_seen: false,
                setup_seen: false,
                power_notified: false,
                power_reported: None,
                restart_acks: 0,
            },
            #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
            crash_evidence: crate::qcom_modem_crash::Evidence::default(),
            out: [0; BYTES],
            busy: false,
            started: false,
            lifecycle: Lifecycle::PreAuth,
            publishing_power: None,
            ready_taken: false,
            free_uncached: memory::FINAL_FREE,
            free_cached: memory::PART_BYTES,
        }
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub fn crash_evidence(&self) -> crate::qcom_modem_crash::Evidence {
        self.crash_evidence
    }
    pub fn memory(&self) -> &Extension<I, P> {
        &self.memory
    }
    pub fn report(&self) -> Report {
        let mut r = self.report;
        if self.busy && r.error.is_none() {
            r.phase = Phase::Failed;
            r.error = Some(Error::Consumed);
        }
        r
    }
    fn fail(&mut self, e: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error.get_or_insert(e);
        e
    }
    fn parts(&mut self) -> Result<(&mut I, &mut P), Error> {
        self.memory.smp2p_parts().map_err(|_| Error::Predecessor)
    }
    fn observe(&mut self, initial: bool) -> Result<Observation, Error> {
        let o = self
            .signals
            .observe(self.generation)
            .map_err(|_| Error::Guard)?;
        if o.generation != self.generation || self.generation == 0 {
            return Err(Error::Guard);
        }
        if initial && o.lifecycle != Lifecycle::PreAuth {
            return Err(Error::Guard);
        }
        if o.lifecycle < self.lifecycle {
            return Err(Error::PeerRestart);
        }
        if self.publishing_power.is_some_and(|p| p != o.power)
            || (self.report.power_reported == Some(Power::Active) && o.power != Power::Active)
        {
            return Err(Error::Guard);
        }
        self.lifecycle = o.lifecycle;
        if o.lifecycle != Lifecycle::PreAuth {
            self.started = true;
        }
        Ok(o)
    }
    fn owned(&mut self) -> Result<bool, Error> {
        let (i, _) = self.parts()?;
        i.barrier();
        Ok(i.lock_read() == 1)
    }
    fn acquire(&mut self) -> Result<(), Error> {
        for _ in 0..ipa::LOCK_ATTEMPT_LIMIT {
            self.parts()?.0.lock_write(1);
            if self.owned()? {
                return Ok(());
            }
        }
        Err(Error::LockBusy)
    }
    fn release(&mut self) -> Result<(), Error> {
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        let (i, _) = self.parts()?;
        i.lock_write(0);
        i.barrier();
        Ok(())
    }
    fn directory(&mut self, initial: bool) -> Result<Option<Item>, Error> {
        self.directory_with_extra(initial, None).map(|(peer, _)| peer)
    }
    fn directory_with_extra(&mut self, initial: bool, extra: Option<u16>) -> Result<(Option<Item>, Option<dir::Directory>), Error> {
        let old_u = self.free_uncached;
        let old_c = self.free_cached;
        let (io, _) = self.parts()?;
        Extension::<I, P>::table(io).map_err(|_| Error::Layout)?;
        // Preserve a stability check across two independently validated scans,
        // while resolving the five fixed IDs together under the same SMEM lock.
        let ids = [497, memory::ITEMS[0].0, memory::ITEMS[1].0, memory::ITEMS[2].0, PEER_ID];
        let (all, extra_directory) = if let Some(id) = extra {
            let ids = [ids[0], ids[1], ids[2], ids[3], ids[4], id];
            let found = dir::allocation_directories(io, ids).map_err(|_| Error::Directory)?;
            if dir::allocation_directories(io, ids).map_err(|_| Error::Directory)? != found {
                return Err(Error::Directory);
            }
            ([found[0], found[1], found[2], found[3], found[4]], Some(found[5]))
        } else {
            let found = dir::allocation_directories(io, ids).map_err(|_| Error::Directory)?;
            if dir::allocation_directories(io, ids).map_err(|_| Error::Directory)? != found {
                return Err(Error::Directory);
            }
            (found, None)
        };
        let a = all[0];
        if a.route.index != 7
            || a.route.offset != memory::PART
            || a.route.size != memory::PART_BYTES
            || a.route.cacheline != 32
            || a.item
                != Some(Item {
                    offset: memory::PART + 48,
                    size: 8192,
                })
            || a.free_uncached < old_u
            || a.free_cached > old_c
        {
            return Err(Error::Layout);
        }
        for (index, (_, item)) in memory::ITEMS.into_iter().enumerate() {
            let d = all[index + 1];
            if d.route != a.route
                || d.free_uncached != a.free_uncached
                || d.free_cached != a.free_cached
                || d.item != Some(item)
            {
                return Err(Error::Layout);
            }
        }
        let d = all[4];
        if d.route != a.route
            || d.free_uncached != a.free_uncached
            || d.free_cached != a.free_cached
        {
            return Err(Error::Layout);
        }
        if initial
            && (a.free_uncached != memory::FINAL_FREE
                || a.free_cached != memory::PART_BYTES
                || d.item.is_some())
        {
            return Err(Error::Layout);
        }
        self.free_uncached = a.free_uncached;
        self.free_cached = a.free_cached;
        if let Some(bound) = self.report.peer_item {
            if d.item != Some(bound) {
                return Err(Error::PeerIdentity);
            }
        }
        if let Some(item) = d.item {
            if item.size < BYTES
                || item.size > 4096
                || item
                    .offset
                    .checked_add(item.size)
                    .is_none_or(|e| e > ipa::SMEM_BYTES - 4096)
            {
                return Err(Error::Layout);
            }
            for (_, owned) in memory::ITEMS {
                if item.offset < owned.offset + owned.size && owned.offset < item.offset + item.size
                {
                    return Err(Error::Layout);
                }
            }
            self.report.peer_item = Some(item);
        }
        Ok((d.item, extra_directory))
    }
    fn outbound(&mut self) -> Result<(), Error> {
        let expected = self.out;
        let (i, _) = self.parts()?;
        for (n, chunk) in expected.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes(chunk.try_into().unwrap());
            if i.read_le32(AP.offset + n * 4) != word {
                return Err(Error::Outbound);
            }
        }
        Ok(())
    }
    fn store(&mut self, at: usize, value: u32, field: Field, initial: bool) -> Result<(), Error> {
        self.observe(initial)?;
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        self.outbound()?;
        self.report.submitted += 1;
        self.parts()?
            .1
            .smp_store(&Write { at, value, field })
            .map_err(|_| Error::Store)?;
        let (i, _) = self.parts()?;
        i.barrier();
        let mut b = [0; 4];
        for (n, v) in b.iter_mut().enumerate() {
            *v = i.read8(AP.offset + at + n);
        }
        if u32::from_le_bytes(b) != value {
            return Err(Error::Readback);
        }
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        self.out[at..at + 4].copy_from_slice(&value.to_le_bytes());
        self.report.completed += 1;
        Ok(())
    }
    fn kick(&mut self, initial: bool) -> Result<(), Error> {
        self.observe(initial)?;
        self.outbound()?;
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        self.parts()?.0.barrier();
        self.report.kicks_sent += 1;
        self.signals.notify().map_err(|_| Error::Notify)?;
        self.report.kicks_done += 1;
        Ok(())
    }
    fn finish(&mut self, result: Result<(), Error>) -> Result<Phase, Error> {
        if let Err(e) = self.release() {
            return Err(self.fail(e));
        }
        match result {
            Ok(()) => {
                self.busy = false;
                Ok(self.report.phase)
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    pub fn initialize(&mut self) -> Result<Phase, Error> {
        if self.busy || self.report.error.is_some() || self.report.phase != Phase::Initialize {
            return Err(self.fail(Error::Consumed));
        }
        self.busy = true;
        if let Err(e) = self.observe(true).and_then(|_| self.acquire()) {
            return Err(self.fail(e));
        }
        let result = (|| {
            self.directory(true)?;
            self.outbound()?;
            self.parts()?.1.claim_smp2p()?;
            for (n, (at, value)) in INIT.into_iter().enumerate() {
                self.store(at, value, Field::Init, true)?;
                self.report.initialized = n + 1;
                // Original Linux publishes version, kicks, allocates both
                // entries, then kicks the outgoing edge again.
                if n == 5 || n == 15 {
                    self.kick(true)?;
                }
            }
            self.observe(true)?;
            self.outbound()?;
            self.report.phase = Phase::Active;
            Ok(())
        })();
        self.finish(result)
    }
    fn read_peer(&mut self, item: Item) -> Result<Option<[u8; BYTES]>, Error> {
        let mut a = [0; BYTES];
        let mut b = [0; BYTES];
        let (i, _) = self.parts()?;
        i.barrier();
        read_item(i, item.offset, &mut a);
        i.barrier();
        read_item(i, item.offset, &mut b);
        i.barrier();
        if a == b { Ok(Some(a)) } else { Ok(None) }
    }
    fn accept_peer(&mut self, p: Peer) -> Result<(), Error> {
        if p.slave_kernel.is_some_and(|v| v & 1 != 0) {
            return Err(Error::PeerFatal);
        }
        if p.slave_kernel.is_some_and(|v| v & (8 | 128) != 0) {
            return Err(Error::PeerStopped);
        }
        if let Some(old) = self.report.peer {
            if p.valid < old.valid
                || p.names[..old.valid as usize] != old.names[..old.valid as usize]
            {
                return Err(Error::PeerNames);
            }
            if self.report.negotiated && p.version != old.version {
                return Err(Error::PeerVersion);
            }
            if self.report.ready_seen && (p.flags ^ old.flags) & 1 != 0 {
                return Err(Error::PeerRestart);
            }
        }
        if self.report.ready_seen && p.slave_kernel.is_none_or(|v| v & 2 == 0) {
            return Err(Error::PeerRestart);
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Poll, Error> {
        if self.busy || self.report.error.is_some() || self.report.phase != Phase::Active {
            return Err(self.fail(Error::Consumed));
        }
        self.busy = true;
        if let Err(e) = self.observe(false).and_then(|_| self.acquire()) {
            return Err(self.fail(e));
        }
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        let captured_before = if self.crash_evidence.before.is_none() {
            self.crash_evidence.before = Some(match self.parts() {
                Ok((io, _)) => crate::qcom_modem_crash::snapshot(io),
                Err(_) => Err(crate::qcom_modem_status::Error::Unstable),
            });
            true
        } else { false };
        let result = (|| {
            self.outbound()?;
            let item = self.directory(false)?;
            if !self.started {
                if item.is_some() {
                    return Err(Error::PeerIdentity);
                }
                return Ok(Poll::Waiting);
            }
            let Some(item) = item else {
                return Ok(Poll::Waiting);
            };
            let Some(bytes) = self.read_peer(item)? else {
                self.report.unstable += 1;
                return Ok(Poll::Unstable);
            };
            let Some(p) = decode(&bytes)? else {
                if self.report.negotiated {
                    return Err(Error::PeerRestart);
                }
                return Ok(Poll::Waiting);
            };
            self.accept_peer(p)?;
            self.report.samples += 1;
            // Pin names/identity even if this sample only changes our version.
            self.report.peer = Some(p);
            if !self.report.negotiated {
                if p.version < self.report.version {
                    self.store(
                        4,
                        u32::from(p.version) | (self.report.features << 8),
                        Field::Version,
                        false,
                    )?;
                    self.report.version = p.version;
                    self.kick(false)?;
                    return Ok(Poll::VersionChanged);
                }
                if p.version != self.report.version {
                    return Err(Error::PeerVersion);
                }
                self.report.features &= p.features;
                self.store(
                    4,
                    u32::from(self.report.version) | (self.report.features << 8),
                    Field::Features,
                    false,
                )?;
                self.report.negotiated = true;
            }
            if p.ipa.is_some_and(|v| v & 1 != 0) && !self.report.power_notified {
                let power = self.observe(false)?.power;
                self.publishing_power = Some(power);
                let enabled = u32::from(power == Power::Active) * 2;
                self.store(56, enabled, Field::IpaEnabled, false)?;
                if enabled != 0 {
                    self.kick(false)?;
                }
                self.store(56, enabled | 1, Field::IpaValid, false)?;
                self.kick(false)?;
                self.report.power_notified = true;
                self.report.power_reported = Some(power);
                self.publishing_power = None;
            }
            if self.report.features & 1 != 0 {
                let ack = ((p.flags & 1) != 0) as u32 * 2;
                if word(&self.out, 16) != ack {
                    self.store(16, ack, Field::RestartAck, false)?;
                    self.kick(false)?;
                    self.report.restart_acks += 1;
                }
            }
            self.report.ready_seen |= p.slave_kernel.is_some_and(|v| v & 2 != 0);
            self.report.handover_seen |= p.slave_kernel.is_some_and(|v| v & 4 != 0);
            self.report.setup_seen |= p.ipa.is_some_and(|v| v & 2 != 0);
            self.report.peer = Some(p);
            self.observe(false)?;
            self.outbound()?;
            Ok(Poll::Observed)
        })();
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        if matches!(result, Err(Error::PeerFatal)) {
            self.crash_evidence.after = Some(match self.parts() {
                Ok((io, _)) => crate::qcom_modem_crash::snapshot(io),
                Err(_) => Err(crate::qcom_modem_status::Error::Unstable),
            });
        }
        let released = self.release();
        if let Err(e) = released {
            #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
            {
                if captured_before { self.crash_evidence.before = Some(Err(crate::qcom_modem_status::Error::LockLost)); }
                if self.crash_evidence.after.is_some() { self.crash_evidence.after = Some(Err(crate::qcom_modem_status::Error::LockLost)); }
            }
            return Err(self.fail(e));
        }
        match result {
            Ok(p) => {
                self.busy = false;
                Ok(p)
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    pub fn take_ready_observation(&mut self) -> Result<ReadyObservation, Error> {
        if self.busy || self.ready_taken || self.report.error.is_some() || !self.report.ready_seen {
            return Err(self.fail(Error::Consumed));
        }
        if let Err(e) = self.observe(false) {
            return Err(self.fail(e));
        }
        self.ready_taken = true;
        Ok(ReadyObservation {
            generation: self.generation,
        })
    }
}

#[cfg(feature = "sunfish-linux-mpss-glink")]
#[path = "sunfish_mpss_linux_glink.rs"]
mod glink;
#[cfg(feature = "sunfish-linux-mpss-glink")]
pub(crate) use glink::Kind as GlinkField;
#[cfg(feature = "sunfish-linux-mpss-glink")]
pub use glink::{GlinkError, GlinkPayload, GlinkPoll, GlinkReport, GlinkRx, GlinkWrite};
