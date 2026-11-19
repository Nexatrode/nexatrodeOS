//! Bounded passive NEW_SERVER inventory using fresh local intents3..34.
//! No peer/local intent reuse, outgoing QRTR payload or service/QMI/DMA action.
use crate::{
    pixel_glink_channel, pixel_glink_done, pixel_glink_open,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, Header},
    pixel_glink_rx::{self as rx, Buffer},
    pixel_glink_version,
    pixel_modem_status::{Clock, Window, sync},
    pixel_qrtr_hello,
    pixel_qrtr_next::{self as next, DiscoveryCheckpoint},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{self as smem, Error, Read};
pub const MAX_NEW: usize = 32;
pub const MAX_TX: usize = 168 + 32 * MAX_NEW;
pub const TARGET_SERVICE: u32 = 0x45;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Record {
    pub service: u32,
    pub instance: u32,
    pub node: u32,
    pub port: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub stage: u32,
    pub records: [Record; MAX_NEW + 1],
    pub count: usize,
    pub latest: Record,
    pub found: bool,
    pub target: Record,
    pub limit: bool,
    pub words: [u32; 8],
    pub available: u32,
    pub copied: u32,
    pub done_consumed: u32,
    pub requests: u32,
    pub advertised: u32,
    pub notifications: u32,
    pub byte_writes: u32,
    pub tail_stores: u32,
    pub head_stores: u32,
    pub old_guards: bool,
    pub pool_guards: bool,
    pub pending: Option<Header>,
    pub qrtr_type: u32,
    pub body_command: u32,
    pub samples: u32,
    pub data_samples: u32,
    pub elapsed_ms: u32,
    pub frequency: u32,
    pub slave_kernel: u32,
    pub status_samples: u32,
    pub status_changes: u32,
    pub first_status: u32,
    pub first_fatal_status: u32,
    pub first_fatal_ms: u32,
}
impl Report {
    fn new() -> Self {
        Self {
            error: None,
            stage: 0,
            records: [Record {
                service: 0,
                instance: 0,
                node: 0,
                port: 0,
            }; MAX_NEW + 1],
            count: 0,
            latest: Record::default(),
            found: false,
            target: Record::default(),
            limit: false,
            words: [0; 8],
            available: 0,
            copied: 0,
            done_consumed: 0,
            requests: 0,
            advertised: 0,
            notifications: 0,
            byte_writes: 0,
            tail_stores: 0,
            head_stores: 0,
            old_guards: false,
            pool_guards: false,
            pending: None,
            qrtr_type: 0,
            body_command: 0,
            samples: 0,
            data_samples: 0,
            elapsed_ms: 0,
            frequency: 0,
            slave_kernel: u32::MAX,
            status_samples: 0,
            status_changes: 0,
            first_status: u32::MAX,
            first_fatal_status: u32::MAX,
            first_fatal_ms: u32::MAX,
        }
    }
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.old_guards && self.pool_guards && (self.found || self.limit)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Packet {
    Done(usize),
    Advertise(usize),
    Planned(Plan),
}
// Only this module can construct plans. No wire value supplies a write address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Retire,
    Credit,
    RequestedCredit,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Plan {
    kind: Kind,
    cycle: usize,
    at: usize,
    tail: u32,
}
impl Plan {
    fn shape(self) -> (usize, usize, u32, u32) {
        assert!(self.cycle < MAX_NEW);
        let (n, id, min_at, min_tail, max_tail) = match self.kind {
            Kind::Retire => (
                8,
                2 + self.cycle as u32,
                168 + 24 * self.cycle,
                744 + 72 * self.cycle as u32,
                744 + 80 * self.cycle as u32,
            ),
            Kind::Credit => (
                16,
                3 + self.cycle as u32,
                176 + 24 * self.cycle,
                744 + 72 * self.cycle as u32,
                752 + 80 * self.cycle as u32,
            ),
            Kind::RequestedCredit => (
                24,
                3 + self.cycle as u32,
                176 + 24 * self.cycle,
                752 + 72 * self.cycle as u32,
                752 + 80 * self.cycle as u32,
            ),
        };
        let max_at = if self.kind == Kind::Retire {
            168 + 32 * self.cycle
        } else {
            176 + 32 * self.cycle
        };
        assert!(
            self.at >= min_at && self.at <= max_at && self.at % 8 == 0 && self.at + n <= MAX_TX
        );
        assert!(self.tail >= min_tail && self.tail <= max_tail && self.tail % 8 == 0);
        (self.at, n, self.tail, id)
    }
}
impl Packet {
    pub(crate) fn shape(self) -> (usize, usize, u32, u32) {
        match self {
            Self::Planned(p) => p.shape(),
            Self::Done(k) => {
                assert!(k <= MAX_NEW);
                (168 + 32 * k, 8, 744 + 80 * k as u32, 2 + k as u32)
            }
            Self::Advertise(k) => {
                assert!(k < MAX_NEW);
                (176 + 32 * k, 24, 752 + 80 * k as u32, 3 + k as u32)
            }
        }
    }
    pub(crate) fn has_request(self) -> bool {
        matches!(self, Self::Advertise(_))
            || matches!(self,Self::Planned(p) if p.kind==Kind::RequestedCredit)
    }
    pub(crate) fn writes_tail(self) -> bool {
        !matches!(self,Self::Planned(p) if p.kind==Kind::Credit)
    }
    pub(crate) fn is_done(self) -> bool {
        matches!(self, Self::Done(_)) || matches!(self,Self::Planned(p) if p.kind==Kind::Retire)
    }
    pub(crate) fn cycle(self) -> usize {
        match self {
            Self::Done(k) | Self::Advertise(k) => k,
            Self::Planned(p) => p.cycle,
        }
    }
    pub(crate) fn bytes(self) -> ([u8; 24], usize) {
        let (_, n, _, id) = self.shape();
        let mut b = [0; 24];
        match self {
            Self::Planned(p) => {
                if p.kind == Kind::Retire {
                    b[..4].copy_from_slice(&[6, 0, 1, 0]);
                    b[4..8].copy_from_slice(&id.to_le_bytes());
                } else {
                    b[..16].copy_from_slice(&[5, 0, 1, 0, 1, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
                    b[12..16].copy_from_slice(&id.to_le_bytes());
                    if p.kind == Kind::RequestedCredit {
                        b[16..24].copy_from_slice(&[8, 0, 1, 0, 1, 0, 0, 0]);
                    }
                }
            }
            Self::Done(_) => {
                b[..8].copy_from_slice(&[6, 0, 1, 0, 0, 0, 0, 0]);
                b[4..8].copy_from_slice(&id.to_le_bytes())
            }
            Self::Advertise(_) => {
                b.copy_from_slice(&[
                    5, 0, 1, 0, 1, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 8, 0, 1, 0, 1, 0, 0, 0,
                ]);
                b[12..16].copy_from_slice(&id.to_le_bytes())
            }
        }
        (b, n)
    }
}
pub(crate) trait Io {
    fn packet(&mut self, p: Packet);
    fn tail(&mut self, p: Packet);
    fn head(&mut self, p: Packet);
    fn notify(&mut self);
}
#[derive(Clone, Copy)]
struct Frame {
    start: u32,
    id: u32,
    wire: [u8; 72],
    payload: [u8; 52],
}
pub(crate) fn service(payload: &[u8; 52]) -> Result<Record, Error> {
    let mut w = [0; 13];
    for (i, v) in w.iter_mut().enumerate() {
        *v = u32::from_le_bytes(payload[4 * i..4 * i + 4].try_into().unwrap())
    }
    if w[..9] != [1, 4, 0, 0xfffffffe, 0, 20, 1, 0xfffffffe, 4]
        || w[11] != 0
        || w[12] == 0
        || w[12] >= 0xfffffffe
    {
        return Err(Error::Payload);
    }
    Ok(Record {
        service: w[9],
        instance: w[10],
        node: w[11],
        port: w[12],
    })
}
pub(crate) fn initial_tx() -> [u8; MAX_TX] {
    let mut b = [0; MAX_TX];
    b[..16].copy_from_slice(&pixel_glink_version::PACKETS);
    b[16..32].copy_from_slice(&pixel_glink_open::REQUEST);
    b[32..40].copy_from_slice(&pixel_glink_channel::ACK);
    b[40..64].copy_from_slice(&rx::PACKETS);
    b[64..72].copy_from_slice(&pixel_glink_done::DONE);
    b[72..144].copy_from_slice(&pixel_qrtr_hello::PACKET);
    b[144..168].copy_from_slice(&next::PACKETS);
    b
}
struct Engine<'a> {
    ready: &'a ReadyObservation,
    base: &'a receive::Report,
    first: &'a Buffer,
    first_payload: [u8; 52],
    second: &'a Buffer,
    second_payload: [u8; 52],
    prefix: &'a [u8],
    pool: &'a mut [Buffer; MAX_NEW],
    payloads: [[u8; 52]; MAX_NEW],
    used: usize,
    expected: [u8; MAX_TX],
    len: usize,
    head: u32,
    tail: u32,
    last_tx: u32,
    last_rx: u32,
    r: Report,
}
impl Engine<'_> {
    fn note_status(&mut self, value: u32) {
        if self.r.status_samples == 0 {
            self.r.first_status = value;
        } else if self.r.slave_kernel != value {
            self.r.status_changes = self.r.status_changes.saturating_add(1);
        }
        self.r.status_samples = self.r.status_samples.saturating_add(1);
        if self.r.first_fatal_status == u32::MAX && value & 1 != 0 {
            self.r.first_fatal_status = value;
            self.r.first_fatal_ms = self.r.elapsed_ms;
        }
        self.r.slave_kernel = value;
    }
    fn state(&mut self, m: &impl Read) -> Result<receive::Report, Error> {
        let s = match receive::inspect_ref(m, self.ready) {
            Ok(s) => s,
            Err(Error::Fatal) => {
                // The normal ready gate stops on the first fatal bit. Read the
                // same validated status item once more so the terminal raw
                // word and its bounded observation time are not lost with the
                // error return. This remains read-only under the caller lock.
                if let Some(value) = smem::directory(m)
                    .ok()
                    .and_then(|d| d.item)
                    .and_then(|item| smem::payload(m, item).ok())
                    .and_then(|status| status.slave_kernel)
                {
                    self.note_status(value);
                }
                return Err(Error::Fatal);
            }
            Err(e) => return Err(e),
        };
        if let Some(value) = s.inventory.status.slave_kernel {
            self.note_status(value);
        }
        self.r.words = s.words;
        self.r.available = s.available;
        self.r.pending = s.header;
        if s.inventory.items != self.base.inventory.items
            || s.inventory.directory.route != self.base.inventory.directory.route
        {
            return Err(Error::RouteChanged);
        }
        if s.words[1] != self.head
            || s.words[2] != self.tail
            || s.words[0] < self.last_tx
            || s.words[0] > self.head
            || s.words[3] < self.last_rx
            || s.words[3] > 752 + 80 * MAX_NEW as u32
            || s.words[4..] != [0; 4]
        {
            return Err(Error::Outbound);
        }
        self.last_tx = s.words[0];
        self.last_rx = s.words[3];
        Ok(s)
    }
    fn tx(&self, m: &impl Read) -> Result<(), Error> {
        for i in 0..TX.size {
            let want = if i < self.len { self.expected[i] } else { 0 };
            if m.read8(TX.offset + i) != want {
                return Err(Error::Publication);
            }
        }
        Ok(())
    }
    fn buffers(&mut self) -> Result<(), Error> {
        self.r.old_guards = false;
        self.r.pool_guards = false;
        if !self.first.verified(&self.first_payload) || !self.second.verified(&self.second_payload)
        {
            return Err(Error::Publication);
        }
        self.r.old_guards = true;
        for i in 0..MAX_NEW {
            if if i < self.used {
                !self.pool[i].verified(&self.payloads[i])
            } else {
                !self.pool[i].empty()
            } {
                return Err(Error::Publication);
            }
        }
        self.r.old_guards = true;
        self.r.pool_guards = true;
        Ok(())
    }
    fn span(
        &self,
        m: &impl Read,
        s: &receive::Report,
        start: u32,
        n: usize,
    ) -> Result<[u8; 88], Error> {
        if n > 88 || start < self.tail || start as usize + n > s.words[3] as usize {
            return Err(Error::Bounds);
        }
        let item = s.inventory.items[2].ok_or(Error::ItemSize)?;
        let mut a = [0; 88];
        let mut b = [0; 88];
        for i in 0..n {
            a[i] = m.read8(item.offset + start as usize + i)
        }
        sync();
        for i in 0..n {
            b[i] = m.read8(item.offset + start as usize + i)
        }
        if a != b {
            return Err(Error::Unstable);
        }
        Ok(a)
    }
    fn frame_span(&mut self, m: &impl Read, s: &receive::Report, f: &Frame) -> Result<(), Error> {
        let skip = (f.start - self.tail) as usize;
        if skip != 0 && !(f.id == 2 && skip == 16) {
            return Err(Error::Bounds);
        }
        if s.words[3] < f.start + 72 || s.words[3] > f.start + 80 {
            return Err(Error::Bounds);
        }
        let bytes = self.span(m, s, self.tail, skip + 72)?;
        if (skip == 16 && bytes[..16] != self.prefix[..16]) || bytes[skip..skip + 72] != f.wire {
            return Err(Error::Unstable);
        }
        Ok(())
    }
    fn request(&mut self, m: &impl Read, s: &receive::Report) -> Result<bool, Error> {
        if s.available == 0 {
            return Ok(false);
        }
        if s.available != 8 {
            return Err(Error::Bounds);
        }
        let b = self.span(m, s, self.tail, 8)?;
        if b[..8] != [7, 0, 2, 0, 52, 0, 0, 0] {
            return Err(Error::Payload);
        }
        Ok(true)
    }
    fn publish(
        &mut self,
        m: &impl Read,
        io: &mut impl Io,
        p: Packet,
        frame: Option<&Frame>,
    ) -> Result<(), Error> {
        let (at, n, new_tail, _) = p.shape();
        if self.len != at || self.head as usize != at || at + n > MAX_TX {
            return Err(Error::Outbound);
        }
        self.r.stage = 2;
        let before = self.state(m)?;
        self.tx(m)?;
        self.buffers()?;
        match frame {
            Some(f) => {
                self.frame_span(m, &before, f)?;
                if !p.is_done() || new_tail != f.start + 72 {
                    return Err(Error::Bounds);
                }
            }
            None => {
                if self.request(m, &before)? != p.has_request() {
                    return Err(Error::Unstable);
                }
                if new_tail != self.tail + if p.has_request() { 8 } else { 0 } {
                    return Err(Error::Bounds);
                }
            }
        }
        sync();
        self.tx(m)?;
        let checked = self.state(m)?;
        if checked.words != before.words {
            return Err(Error::Unstable);
        }
        let (bytes, size) = p.bytes();
        self.expected[at..at + n].copy_from_slice(&bytes[..size]);
        self.len += n;
        self.r.stage = 3;
        io.packet(p);
        self.r.byte_writes += n as u32;
        sync();
        self.tx(m)?;
        sync();
        self.tx(m)?;
        let staged = self.state(m)?;
        self.buffers()?;
        match frame {
            Some(f) => self.frame_span(m, &staged, f)?,
            None => {
                if self.request(m, &staged)? != p.has_request() || staged.words != before.words {
                    return Err(Error::Unstable);
                }
            }
        }
        let checked = self.state(m)?;
        if checked.words != staged.words {
            return Err(Error::Unstable);
        }
        // Once released, peer bytes may change immediately. Subsequent gates
        // inspect only live indices, TX and retained private payloads.
        self.r.stage = 4;
        if p.writes_tail() {
            io.tail(p);
            self.r.tail_stores += 1;
            self.tail = new_tail;
        } else if new_tail != self.tail {
            return Err(Error::Bounds);
        }
        sync();
        let released = self.state(m)?;
        if frame.is_none() && (released.words[3] != before.words[3] || released.available != 0) {
            return Err(Error::Unstable);
        }
        self.tx(m)?;
        self.buffers()?;
        let gate = self.state(m)?;
        if frame.is_none() && (gate.words[3] != before.words[3] || gate.available != 0) {
            return Err(Error::Unstable);
        }
        self.r.stage = 5;
        io.head(p);
        self.r.head_stores += 1;
        self.head = (at + n) as u32;
        sync();
        self.state(m)?;
        self.tx(m)?;
        self.buffers()?;
        self.state(m)?;
        Ok(())
    }
    fn observe(&mut self, m: &impl Read) -> Result<bool, Error> {
        let before = self.state(m)?;
        self.tx(m)?;
        self.buffers()?;
        sync();
        self.tx(m)?;
        let after = self.state(m)?;
        if before.words != after.words {
            return Err(Error::Unstable);
        }
        Ok(after.words[0] == self.head)
    }
    fn receive(&mut self, m: &impl Read, k: usize) -> Result<Option<Frame>, Error> {
        self.r.stage = 8;
        let before = self.state(m)?;
        self.tx(m)?;
        self.buffers()?;
        if before.available > 80 {
            return Err(Error::Bounds);
        }
        if before.available == 0 {
            return Ok(None);
        }
        let n = before.available as usize;
        let bytes = self.span(m, &before, self.tail, n)?;
        if n >= 8 {
            let h = Header {
                command: u16::from_le_bytes(bytes[..2].try_into().unwrap()),
                param1: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
                param2: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            };
            self.r.pending = Some(h);
            if h != (Header {
                command: 9,
                param1: 2,
                param2: 3 + k as u32,
            }) {
                return Err(Error::Payload);
            }
        }
        if n < 16 {
            return Ok(None);
        }
        if bytes[8..16] != [52, 0, 0, 0, 0, 0, 0, 0] {
            return Err(Error::Payload);
        }
        if n < 72 {
            return Ok(None);
        }
        self.tx(m)?;
        self.buffers()?;
        let after = self.state(m)?;
        if before.words != after.words {
            return Err(Error::Unstable);
        }
        let payload: [u8; 52] = bytes[16..68].try_into().unwrap();
        self.note(&payload);
        service(&payload)?;
        let checked = self.state(m)?;
        if checked.words != after.words || checked.words[0] != self.head || !self.pool[k].empty() {
            return Err(Error::Unstable);
        }
        self.r.stage = 9;
        self.pool[k].copy52(&payload);
        self.payloads[k] = payload;
        self.used = k + 1;
        sync();
        self.buffers()?;
        let checked = self.state(m)?;
        if checked.words != after.words {
            return Err(Error::Unstable);
        }
        self.tx(m)?;
        sync();
        self.buffers()?;
        let checked = self.state(m)?;
        if checked.words != after.words {
            return Err(Error::Unstable);
        }
        self.r.copied += 1;
        Ok(Some(Frame {
            start: self.tail,
            id: 3 + k as u32,
            wire: bytes[..72].try_into().unwrap(),
            payload,
        }))
    }
    fn note(&mut self, p: &[u8; 52]) {
        let word = |i: usize| u32::from_le_bytes(p[4 * i..4 * i + 4].try_into().unwrap());
        self.r.qrtr_type = word(1);
        self.r.body_command = word(8);
        self.r.latest = Record {
            service: word(9),
            instance: word(10),
            node: word(11),
            port: word(12),
        }
    }
    fn record(&mut self, f: &Frame) -> Result<(), Error> {
        self.note(&f.payload);
        let rec = service(&f.payload)?;
        if self.r.count > MAX_NEW {
            return Err(Error::Bounds);
        }
        self.r.records[self.r.count] = rec;
        self.r.count += 1;
        if rec.service == TARGET_SERVICE {
            self.r.found = true;
            self.r.target = rec
        }
        Ok(())
    }
}
fn budget(
    clock: &mut impl Clock,
    start: u64,
    freq: u64,
    extended: bool,
    r: &mut Report,
) -> Result<(), Error> {
    let ticks = clock.now().wrapping_sub(start);
    r.elapsed_ms = (ticks.saturating_mul(1000) / freq).min(u32::MAX as u64) as u32;
    if ticks >= freq * if extended { 65 } else { 20 } {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}
fn pause(clock: &mut impl Clock, freq: u64) -> Result<(), Error> {
    let start = clock.now();
    for _ in 0..1_000_000 {
        if clock.now().wrapping_sub(start) >= freq / 100 {
            return Ok(());
        }
        core::hint::spin_loop()
    }
    Err(Error::Clock)
}
fn consumed(
    e: &mut Engine<'_>,
    w: &mut Window,
    clock: &mut impl Clock,
    start: u64,
    freq: u64,
    extended: bool,
    progress: &mut impl FnMut(&Report),
) -> Result<(), Error> {
    for poll in 0..512 {
        budget(clock, start, freq, extended, &mut e.r)?;
        e.r.samples += 1;
        if w.locked(|m| e.observe(m))? {
            return Ok(());
        }
        if poll % 25 == 0 {
            progress(&e.r)
        }
        pause(clock, freq)?
    }
    Err(Error::Timeout)
}
pub(crate) fn run_policy(
    w: &mut Window,
    checkpoint: &mut DiscoveryCheckpoint<'_, '_>,
    pool: &mut [Buffer; MAX_NEW],
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
    proactive: bool,
    extended: bool,
) -> Report {
    let (hello, second, prior, wire) = checkpoint.parts();
    let (send, _, control) = hello.parts();
    let (completion, _) = send.parts();
    let (ready, base, _, _, _, first) = completion.parts();
    let mut e = Engine {
        ready,
        base,
        first,
        first_payload: control.first_payload,
        second,
        second_payload: wire.bytes[16..68].try_into().unwrap(),
        prefix: &control.bytes[..16],
        pool,
        payloads: [[0; 52]; MAX_NEW],
        used: 0,
        expected: initial_tx(),
        len: 168,
        head: 168,
        tail: 656,
        last_tx: 168,
        last_rx: prior.words[3],
        r: Report::new(),
    };
    let result = (|| -> Result<(), Error> {
        e.r.stage = 1;
        if !prior.passed()
            || prior.words[0..3] != [168, 168, 656]
            || !(88..=96).contains(&prior.available)
            || !(72..=80).contains(&wire.size)
        {
            return Err(Error::Payload);
        }
        e.buffers()?;
        let mut frame = Frame {
            start: 672,
            id: 2,
            wire: wire.bytes[..72].try_into().unwrap(),
            payload: wire.bytes[16..68].try_into().unwrap(),
        };
        w.locked(|m| {
            let before = e.state(m)?;
            e.tx(m)?;
            e.frame_span(m, &before, &frame)?;
            e.buffers()?;
            let after = e.state(m)?;
            if before.words != after.words {
                return Err(Error::Unstable);
            }
            Ok(())
        })?;
        e.record(&frame)?;
        progress(&e.r);
        let freq = clock.frequency();
        if !(1_000_000..=1_000_000_000).contains(&freq) {
            return Err(Error::Clock);
        }
        e.r.frequency = freq as u32;
        let start = clock.now();
        let mut advanced = false;
        for _ in 0..10000 {
            if clock.now() != start {
                advanced = true;
                break;
            }
        }
        if !advanced {
            return Err(Error::Clock);
        }
        for k in 0..MAX_NEW {
            if e.r.found {
                return Ok(());
            }
            budget(clock, start, freq, extended, &mut e.r)?;
            let done = if proactive {
                Packet::Planned(Plan {
                    kind: Kind::Retire,
                    cycle: k,
                    at: e.len,
                    tail: frame.start + 72,
                })
            } else {
                Packet::Done(k)
            };
            w.locked(|m| e.publish(m, io, done, Some(&frame)))?;
            e.r.stage = 6;
            sync();
            io.notify();
            e.r.notifications += 1;
            sync();
            consumed(&mut e, w, clock, start, freq, extended, &mut progress)?;
            e.r.done_consumed += 1;
            let mut pending = false;
            for poll in 0..if proactive { 1 } else { 512 } {
                budget(clock, start, freq, extended, &mut e.r)?;
                e.r.stage = 7;
                e.r.samples += 1;
                pending = w.locked(|m| {
                    let before = e.state(m)?;
                    e.tx(m)?;
                    e.buffers()?;
                    let found = e.request(m, &before)?;
                    let after = e.state(m)?;
                    if before.words != after.words {
                        return Err(Error::Unstable);
                    }
                    Ok(found)
                })?;
                if pending || proactive {
                    break;
                }
                if poll % 25 == 0 {
                    progress(&e.r)
                }
                pause(clock, freq)?
            }
            if !pending && !proactive {
                return Err(Error::Timeout);
            }
            budget(clock, start, freq, extended, &mut e.r)?;
            let credit = if proactive {
                Packet::Planned(Plan {
                    kind: if pending {
                        Kind::RequestedCredit
                    } else {
                        Kind::Credit
                    },
                    cycle: k,
                    at: e.len,
                    tail: e.tail + if pending { 8 } else { 0 },
                })
            } else {
                Packet::Advertise(k)
            };
            w.locked(|m| e.publish(m, io, credit, None))?;
            e.r.requests += u32::from(pending);
            e.r.advertised += 1;
            sync();
            io.notify();
            e.r.notifications += 1;
            sync();
            consumed(&mut e, w, clock, start, freq, extended, &mut progress)?;
            let mut received = None;
            let data_start = if extended { clock.now() } else { 0 };
            for poll in 0..if extended { 8192 } else { 512 } {
                budget(clock, start, freq, extended, &mut e.r)?;
                if extended && clock.now().wrapping_sub(data_start) >= freq * 60 {
                    return Err(Error::Timeout);
                }
                e.r.samples += 1;
                e.r.data_samples += 1;
                received = w.locked(|m| e.receive(m, k))?;
                if received.is_some() {
                    break;
                }
                if poll % 25 == 0 {
                    progress(&e.r)
                }
                pause(clock, freq)?
            }
            frame = received.ok_or(Error::Timeout)?;
            e.record(&frame)?;
            progress(&e.r);
        }
        e.r.limit = !e.r.found;
        Ok(())
    })();
    if let Err(err) = result {
        e.r.error = Some(err)
    }
    e.r
}
pub(crate) fn run(
    w: &mut Window,
    checkpoint: &mut DiscoveryCheckpoint<'_, '_>,
    pool: &mut [Buffer; MAX_NEW],
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Report {
    run_policy(w, checkpoint, pool, io, clock, progress, false, false)
}
pub(crate) fn run_credit(
    w: &mut Window,
    checkpoint: &mut DiscoveryCheckpoint<'_, '_>,
    pool: &mut [Buffer; MAX_NEW],
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Report {
    run_policy(w, checkpoint, pool, io, clock, progress, true, false)
}
pub(crate) fn run_wait(
    w: &mut Window,
    checkpoint: &mut DiscoveryCheckpoint<'_, '_>,
    pool: &mut [Buffer; MAX_NEW],
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Report {
    run_policy(w, checkpoint, pool, io, clock, progress, true, true)
}
struct NativeIo {
    ap: *mut u8,
    doorbell: *mut u32,
}
impl Io for NativeIo {
    fn packet(&mut self, p: Packet) {
        let (at, n, _, _) = p.shape();
        let (b, size) = p.bytes();
        assert_eq!(n, size);
        for i in 0..n {
            unsafe { self.ap.add(TX.offset - PART + at + i).write_volatile(b[i]) }
        }
    }
    fn tail(&mut self, p: Packet) {
        assert!(p.writes_tail());
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 8)
                .cast::<u32>()
                .write_volatile(p.shape().2.to_le())
        }
    }
    fn head(&mut self, p: Packet) {
        let (at, n, _, _) = p.shape();
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(((at + n) as u32).to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct DiscoveryOnce<'a, 'b, 'c> {
    checkpoint: DiscoveryCheckpoint<'a, 'b>,
    pool: &'c mut [Buffer; MAX_NEW],
    spent: bool,
    proactive: bool,
    extended: bool,
    io: NativeIo,
}
impl<'a, 'b, 'c> DiscoveryOnce<'a, 'b, 'c> {
    /// # Safety
    /// Exclusive exact five-page/APCS mappings, no aliases/IRQ/other AP client.
    /// Pool32 private guarded buffers must be disjoint from both retained
    /// buffers. Keep this object and every lease through restart, all outcomes.
    pub unsafe fn new(
        checkpoint: DiscoveryCheckpoint<'a, 'b>,
        pool: &'c mut [Buffer; MAX_NEW],
        ap: *mut u8,
        doorbell: *mut u32,
    ) -> Self {
        Self {
            checkpoint,
            pool,
            spent: false,
            proactive: false,
            extended: false,
            io: NativeIo { ap, doorbell },
        }
    }
    /// Same exclusive mapping and retained/disjoint pool contract as `new`.
    pub unsafe fn credit(
        checkpoint: DiscoveryCheckpoint<'a, 'b>,
        pool: &'c mut [Buffer; MAX_NEW],
        ap: *mut u8,
        doorbell: *mut u32,
    ) -> Self {
        let mut this = unsafe { Self::new(checkpoint, pool, ap, doorbell) };
        this.proactive = true;
        this
    }
    /// Same exclusive mappings and retained buffer leases as `credit`.
    /// Only DATA observation is extended; no new packet or write authority.
    pub unsafe fn wait(
        checkpoint: DiscoveryCheckpoint<'a, 'b>,
        pool: &'c mut [Buffer; MAX_NEW],
        ap: *mut u8,
        doorbell: *mut u32,
    ) -> Self {
        let mut this = unsafe { Self::credit(checkpoint, pool, ap, doorbell) };
        this.extended = true;
        this
    }
    pub fn run(
        &mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Report {
        if self.spent {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return r;
        }
        self.spent = true;
        run_policy(
            w,
            &mut self.checkpoint,
            self.pool,
            &mut self.io,
            clock,
            progress,
            self.proactive,
            self.extended,
        )
    }
}
