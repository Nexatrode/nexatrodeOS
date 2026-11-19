//! One fixed QRTR v1 HELLO, using only current sealed peer intent0.
//! No new receive intent, RX-tail store, reuse, service request or DMA.
use crate::{
    pixel_glink_channel::{self as channel, Capture},
    pixel_glink_done::{self as done, SendCheckpoint},
    pixel_glink_intents, pixel_glink_open,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, Header},
    pixel_glink_rx::{self as rx, Buffer},
    pixel_glink_version,
    pixel_modem_status::{Clock, Window, sync},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};
pub const MAX_RX: usize = 512;
const fn packet() -> [u8; 72] {
    let mut b = [0; 72];
    b[0] = 9;
    b[2] = 1;
    b[8] = 52;
    let w: [u32; 13] = [1, 2, 1, 0xfffffffe, 0, 20, 0, 0xfffffffe, 2, 0, 0, 0, 0];
    let mut i = 0;
    while i < 13 {
        let v = w[i].to_le_bytes();
        let mut j = 0;
        while j < 4 {
            b[16 + 4 * i + j] = v[j];
            j += 1
        }
        i += 1
    }
    b
}
pub const PACKET: [u8; 72] = packet();
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub stage: u32,
    pub capacity: u32,
    pub reserved: bool,
    pub byte_writes: u32,
    pub head_store: bool,
    pub notifications: u32,
    pub tx_consumed: bool,
    pub words: [u32; 8],
    pub available: u32,
    pub captured: u32,
    pub parsed: u32,
    pub records: u32,
    pub done_seen: bool,
    pub done_command: u32,
    pub requests: u32,
    pub request_size: u32,
    pub advertised: u32,
    pub complete: bool,
    pub stop_offset: u32,
    pub header: Option<Header>,
    pub guards: bool,
    pub samples: u32,
}
impl Report {
    fn new() -> Self {
        Self {
            error: None,
            stage: 0,
            capacity: 0,
            reserved: false,
            byte_writes: 0,
            head_store: false,
            notifications: 0,
            tx_consumed: false,
            words: [0; 8],
            available: 0,
            captured: 0,
            parsed: 0,
            records: 0,
            done_seen: false,
            done_command: 0,
            requests: 0,
            request_size: 0,
            advertised: 0,
            complete: false,
            stop_offset: 0,
            header: None,
            guards: false,
            samples: 0,
        }
    }
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.reserved
            && self.head_store
            && self.notifications == 1
            && self.tx_consumed
            && self.done_seen
            && self.complete
            && self.guards
    }
}
pub(crate) trait Io {
    fn byte(&mut self, i: usize, v: u8);
    fn head(&mut self);
    fn notify(&mut self);
}
pub(crate) fn capacity(c: &Capture) -> Result<u32, Error> {
    if c.size != 488 {
        return Err(Error::Bounds);
    }
    let s = pixel_glink_intents::inspect(&c.bytes[..c.size]);
    if let Some(e) = s.error {
        return Err(e);
    }
    let inv = s.inventory;
    if s.parsed_bytes != c.size
        || inv.entries != 30
        || inv.intent_records != 30
        || inv.duplicate_ids != 0
        || inv.zero_sizes != 0
        || inv.min_id != 0
        || inv.max_id != 29
        || inv.intent_requests != 1
        || inv.last_request_size != 52
    {
        return Err(Error::Payload);
    }
    let (mut at, mut found) = (0, None);
    while at < c.size {
        let b = &c.bytes[at..];
        let command = u16::from_le_bytes(b[..2].try_into().unwrap());
        let count = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
        if command == 5 {
            for i in 0..count {
                let p = 8 + 8 * i;
                let size = u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
                let id = u32::from_le_bytes(b[p + 4..p + 8].try_into().unwrap());
                if id == 0 {
                    if found.is_some() || size < 52 {
                        return Err(Error::Payload);
                    }
                    found = Some(size)
                }
            }
            at += 8 + 8 * count
        } else {
            at += 8
        }
    }
    found.ok_or(Error::Payload)
}
/// Return the size of one exact peer-advertised remote intent after validating
/// the complete captured list. This is used only to select two distinct,
/// already advertised one-shot buffers for SERVREG announce/reply packets.
pub(crate) fn peer_capacity(c: &Capture, wanted: u32) -> Result<u32, Error> {
    if c.size != 488 {
        return Err(Error::Bounds);
    }
    let scan = pixel_glink_intents::inspect(&c.bytes[..c.size]);
    if let Some(e) = scan.error {
        return Err(e);
    }
    let inv = scan.inventory;
    if scan.parsed_bytes != c.size
        || inv.entries != 30
        || inv.intent_records != 30
        || inv.duplicate_ids != 0
        || inv.zero_sizes != 0
        || inv.min_id != 0
        || inv.max_id != 29
        || inv.intent_requests != 1
        || inv.last_request_size != 52
    {
        return Err(Error::Payload);
    }
    let (mut at, mut found) = (0, None);
    while at < c.size {
        let b = &c.bytes[at..];
        let command = u16::from_le_bytes(b[..2].try_into().unwrap());
        let count = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
        if command == 5 {
            for i in 0..count {
                let p = 8 + 8 * i;
                let size = u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
                let id = u32::from_le_bytes(b[p + 4..p + 8].try_into().unwrap());
                if id == wanted {
                    if found.is_some() {
                        return Err(Error::Payload);
                    }
                    found = Some(size)
                }
            }
            at += 8 + 8 * count;
        } else {
            at += 8
        }
    }
    found.filter(|size| *size >= 52).ok_or(Error::Payload)
}
fn plan(prior: &done::Report, received: &rx::Report, c: &Capture) -> Result<u32, Error> {
    if !prior.passed()
        || prior.words != [72, 72, 656, 656, 0, 0, 0, 0]
        || prior.available != 0
        || !received.passed()
        || received.qrtr_words != [1, 2, 0, 0xfffffffe, 0, 20, 0xffffffff, 0xfffffffe]
        || received.control_word != 2
    {
        return Err(Error::Payload);
    }
    capacity(c)
}
fn state(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    head: u32,
    last_tx: u32,
    last_rx: u32,
) -> Result<receive::Report, Error> {
    let s = receive::inspect_ref(m, ready)?;
    if s.inventory.items != base.inventory.items
        || s.inventory.directory.route != base.inventory.directory.route
    {
        return Err(Error::RouteChanged);
    }
    if s.words[1] != head
        || s.words[2] != 656
        || s.words[0] < last_tx
        || s.words[0] > head
        || s.words[3] < last_rx
        || s.words[4..] != [0; 4]
    {
        return Err(Error::Outbound);
    }
    Ok(s)
}
fn tx(m: &impl Read, staged: bool) -> Result<(), Error> {
    for i in 0..TX.size {
        let want = if i < 16 {
            pixel_glink_version::PACKETS[i]
        } else if i < 32 {
            pixel_glink_open::REQUEST[i - 16]
        } else if i < 40 {
            channel::ACK[i - 32]
        } else if i < 64 {
            rx::PACKETS[i - 40]
        } else if i < 72 {
            done::DONE[i - 64]
        } else if staged && i < 144 {
            PACKET[i - 72]
        } else {
            0
        };
        if m.read8(TX.offset + i) != want {
            return Err(Error::Publication);
        }
    }
    Ok(())
}
fn submit(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    prior: &done::Report,
    received: &rx::Report,
    c: &Capture,
    buffer: &Buffer,
    saved: &[u8; 52],
    io: &mut impl Io,
    r: &mut Report,
) -> Result<receive::Report, Error> {
    r.stage = 1;
    r.capacity = plan(prior, received, c)?;
    r.reserved = true;
    r.stage = 2;
    let before = state(m, base, ready, 72, 72, 656)?;
    if before.words != prior.words || !buffer.verified(saved) {
        return Err(Error::Unstable);
    }
    tx(m, false)?;
    sync();
    tx(m, false)?;
    if !buffer.verified(saved) {
        return Err(Error::Publication);
    }
    let checked = state(m, base, ready, 72, 72, 656)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.stage = 3;
    for (i, v) in PACKET.into_iter().enumerate() {
        io.byte(i, v);
        r.byte_writes += 1
    }
    r.stage = 4;
    sync();
    tx(m, true)?;
    sync();
    tx(m, true)?;
    if !buffer.verified(saved) {
        return Err(Error::Publication);
    }
    let staged = state(m, base, ready, 72, 72, 656)?;
    if staged.words != before.words {
        return Err(Error::Unstable);
    }
    let checked = state(m, base, ready, 72, 72, 656)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.stage = 5;
    io.head();
    r.head_store = true;
    r.stage = 6;
    sync();
    let published = state(m, base, ready, 144, 72, 656)?;
    tx(m, true)?;
    if !buffer.verified(saved) {
        return Err(Error::Publication);
    }
    state(m, base, ready, 144, published.words[0], published.words[3])
}
/// Decode only copied control metadata; an observed reuse reply grants no reuse.
pub(crate) fn controls(bytes: &[u8], r: &mut Report) -> Result<(), Error> {
    r.parsed = 0;
    r.records = 0;
    r.done_seen = false;
    r.done_command = 0;
    r.requests = 0;
    r.request_size = 0;
    r.advertised = 0;
    r.complete = false;
    r.header = None;
    r.stop_offset = 0;
    if bytes.len() > MAX_RX || bytes.len() % 8 != 0 {
        return Err(Error::Bounds);
    }
    while (r.parsed as usize) < bytes.len() {
        let at = r.parsed as usize;
        let b = &bytes[at..];
        let h = Header {
            command: u16::from_le_bytes(b[..2].try_into().unwrap()),
            param1: u16::from_le_bytes(b[2..4].try_into().unwrap()),
            param2: u32::from_le_bytes(b[4..8].try_into().unwrap()),
        };
        r.header = Some(h);
        r.stop_offset = at as u32;
        let n = match h.command {
            6 | 14 => {
                if h.param1 != 2 || h.param2 != 0 || r.done_seen {
                    return Err(Error::Payload);
                }
                8
            }
            5 => {
                if h.param1 != 2 || h.param2 == 0 || h.param2 > 63 {
                    return Err(Error::Payload);
                }
                8 + 8 * h.param2 as usize
            }
            7 | 15 => {
                if h.param1 != 2 {
                    return Err(Error::Payload);
                }
                8
            }
            13 => 8,
            _ => return Err(Error::Payload),
        };
        if n > b.len() {
            return Ok(());
        }
        match h.command {
            6 | 14 => {
                r.done_seen = true;
                r.done_command = h.command as u32
            }
            5 => r.advertised += h.param2,
            7 => {
                r.requests += 1;
                r.request_size = h.param2
            }
            _ => {}
        }
        r.parsed += n as u32;
        r.records += 1;
    }
    r.complete = true;
    Ok(())
}
pub(crate) fn sample(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    buffer: &Buffer,
    saved: &[u8; 52],
    r: &mut Report,
) -> Result<(), Error> {
    sample_capture(m, base, ready, buffer, saved, r).map(|_| ())
}
pub(crate) struct ControlCapture {
    pub bytes: [u8; MAX_RX],
    pub size: usize,
    pub first_payload: [u8; 52],
}
fn sample_capture(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    buffer: &Buffer,
    saved: &[u8; 52],
    r: &mut Report,
) -> Result<ControlCapture, Error> {
    let before = state(m, base, ready, 144, r.words[0], r.words[3])?;
    tx(m, true)?;
    if !buffer.verified(saved) {
        return Err(Error::Publication);
    }
    let item = before.inventory.items[2].ok_or(Error::ItemSize)?;
    let n = (before.available as usize).min(MAX_RX);
    let mut one = [0; MAX_RX];
    let mut two = [0; MAX_RX];
    for i in 0..n {
        one[i] = m.read8(item.offset + 656 + i)
    }
    sync();
    for i in 0..n {
        two[i] = m.read8(item.offset + 656 + i)
    }
    if one != two {
        return Err(Error::Unstable);
    }
    sync();
    tx(m, true)?;
    if !buffer.verified(saved) {
        return Err(Error::Publication);
    }
    let after = state(m, base, ready, 144, before.words[0], before.words[3])?;
    if before.words != after.words || before.available != after.available {
        return Err(Error::Unstable);
    }
    r.words = after.words;
    r.available = after.available;
    r.captured = n as u32;
    r.tx_consumed = after.words[0] == 144;
    controls(&one[..n], r)?;
    if n as u32 != after.available {
        return Err(Error::Bounds);
    }
    let checked = state(m, base, ready, 144, after.words[0], after.words[3])?;
    if checked.words != after.words || !buffer.verified(saved) {
        return Err(Error::Unstable);
    }
    r.guards = true;
    Ok(ControlCapture {
        bytes: one,
        size: n,
        first_payload: *saved,
    })
}
pub(crate) fn run(
    w: &mut Window,
    checkpoint: &mut SendCheckpoint<'_>,
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Report {
    run_capture(w, checkpoint, io, clock, progress).0
}
fn run_capture(
    w: &mut Window,
    checkpoint: &mut SendCheckpoint<'_>,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> (Report, Option<ControlCapture>) {
    let (completion, prior) = checkpoint.parts();
    let (ready, base, _, c, received, buffer) = completion.parts();
    let saved = buffer.snapshot();
    let mut r = Report::new();
    let frequency = clock.frequency();
    if !(1_000_000..=1_000_000_000).contains(&frequency) {
        r.error = Some(Error::Clock);
        return (r, None);
    }
    let start = clock.now();
    let mut advanced = false;
    for _ in 0..10000 {
        if clock.now() != start {
            advanced = true;
            break;
        }
    }
    if !advanced {
        r.error = Some(Error::Clock);
        return (r, None);
    }
    let sent = match w.locked(|m| {
        submit(
            m, base, ready, prior, received, c, buffer, &saved, io, &mut r,
        )
    }) {
        Ok(s) => s,
        Err(e) => {
            r.error = Some(e);
            return (r, None);
        }
    };
    r.words = sent.words;
    r.available = sent.available;
    r.stage = 7;
    sync();
    io.notify();
    r.notifications = 1;
    sync();
    let mut captured = None;
    for poll in 0..512 {
        r.samples = poll + 1;
        r.stage = 8;
        match w.locked(|m| sample_capture(m, base, ready, buffer, &saved, &mut r)) {
            Ok(c) => captured = Some(c),
            Err(e) => {
                r.error = Some(e);
                break;
            }
        }
        if r.passed() {
            break;
        }
        if clock.now().wrapping_sub(start) >= frequency * 5 {
            r.error = Some(Error::Timeout);
            break;
        }
        if poll % 25 == 0 {
            progress(&r)
        }
        let wait = clock.now();
        let mut advanced = false;
        for _ in 0..1_000_000 {
            if clock.now().wrapping_sub(wait) >= frequency / 100 {
                advanced = true;
                break;
            }
            core::hint::spin_loop()
        }
        if !advanced {
            r.error = Some(Error::Clock);
            break;
        }
    }
    if !r.passed() && r.error.is_none() {
        r.error = Some(Error::Timeout)
    }
    if !r.passed() {
        captured = None
    }
    (r, captured)
}
struct NativeIo {
    ap: *mut u8,
    doorbell: *mut u32,
}
impl Io for NativeIo {
    fn byte(&mut self, i: usize, v: u8) {
        assert!(i < 72);
        assert_eq!(v, PACKET[i]);
        unsafe { self.ap.add(TX.offset - PART + 72 + i).write_volatile(v) }
    }
    fn head(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(144u32.to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct HelloOnce<'a> {
    checkpoint: SendCheckpoint<'a>,
    spent: bool,
    io: NativeIo,
}
pub struct HelloCheckpoint<'a> {
    send: SendCheckpoint<'a>,
    report: Report,
    capture: ControlCapture,
}
impl<'a> HelloCheckpoint<'a> {
    pub(crate) fn parts(&mut self) -> (&mut SendCheckpoint<'a>, &Report, &ControlCapture) {
        (&mut self.send, &self.report, &self.capture)
    }
}
pub struct HelloStopped<'a> {
    pub report: Report,
    _parked: HelloOnce<'a>,
}
impl<'a> HelloOnce<'a> {
    /// # Safety
    /// Exclusive exact five-page uncached/APCS mappings, no aliases/IRQ/other
    /// AP writer. Keep this object and its buffer/peer-intent leases to restart.
    pub unsafe fn new(checkpoint: SendCheckpoint<'a>, ap: *mut u8, doorbell: *mut u32) -> Self {
        Self {
            checkpoint,
            spent: false,
            io: NativeIo { ap, doorbell },
        }
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
        run(w, &mut self.checkpoint, &mut self.io, clock, progress)
    }
    pub fn into_receive_checkpoint(
        mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Result<HelloCheckpoint<'a>, HelloStopped<'a>> {
        if self.spent {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return Err(HelloStopped {
                report: r,
                _parked: self,
            });
        }
        self.spent = true;
        let (report, capture) = run_capture(w, &mut self.checkpoint, &mut self.io, clock, progress);
        match capture {
            Some(capture) if report.passed() => Ok(HelloCheckpoint {
                send: self.checkpoint,
                report,
                capture,
            }),
            _ => Err(HelloStopped {
                report,
                _parked: self,
            }),
        }
    }
}
