//! One fresh fixed receive promise after the actual HELLO completion/request.
//! Preserve both private buffers and RX tail656; no RX_DONE, intent reuse,
//! extra QRTR transmission, service request or host DMA.
use crate::{
    pixel_glink_channel, pixel_glink_done, pixel_glink_open,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, Header},
    pixel_glink_rx::{self as rx, Buffer},
    pixel_glink_version,
    pixel_modem_status::{Clock, Window, sync},
    pixel_qrtr_hello::{self as hello, ControlCapture, HelloCheckpoint},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};
pub const PACKETS: [u8; 24] = [
    5, 0, 1, 0, 1, 0, 0, 0, 64, 0, 0, 0, 2, 0, 0, 0, 8, 0, 1, 0, 1, 0, 0, 0,
];
pub const DATA_AT: usize = 672;
pub const MAX_PACKET: usize = 80;
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub stage: u32,
    pub byte_writes: u32,
    pub head_store: bool,
    pub notifications: u32,
    pub tx_consumed: bool,
    pub words: [u32; 8],
    pub available: u32,
    pub header: Option<Header>,
    pub chunk: u32,
    pub left: u32,
    pub copied: u32,
    pub guards: bool,
    pub first_guards: bool,
    pub qrtr_words: [u32; 8],
    pub body_words: [u32; 5],
    pub qrtr_v1: bool,
    pub samples: u32,
}
impl Report {
    fn new() -> Self {
        Self {
            error: None,
            stage: 0,
            byte_writes: 0,
            head_store: false,
            notifications: 0,
            tx_consumed: false,
            words: [0; 8],
            available: 0,
            header: None,
            chunk: 0,
            left: 0,
            copied: 0,
            guards: false,
            first_guards: false,
            qrtr_words: [0; 8],
            body_words: [0; 5],
            qrtr_v1: false,
            samples: 0,
        }
    }
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.head_store
            && self.notifications == 1
            && self.tx_consumed
            && self.copied == 52
            && self.guards
            && self.first_guards
    }
}
pub(crate) trait Io {
    fn byte(&mut self, i: usize, v: u8);
    fn head(&mut self);
    fn notify(&mut self);
}
pub(crate) fn plan(prior: &hello::Report, c: &ControlCapture) -> Result<(), Error> {
    if !prior.passed()
        || prior.words != [144, 144, 656, 672, 0, 0, 0, 0]
        || prior.available != 16
        || prior.captured != 16
        || prior.parsed != 16
        || prior.records != 2
        || prior.done_command != 14
        || prior.requests != 1
        || prior.request_size != 52
        || prior.advertised != 0
        || c.size != 16
    {
        return Err(Error::Payload);
    }
    let mut done = 0;
    let mut request = 0;
    for b in c.bytes[..16].chunks_exact(8) {
        match b {
            [14, 0, 2, 0, 0, 0, 0, 0] => done += 1,
            [7, 0, 2, 0, 52, 0, 0, 0] => request += 1,
            _ => return Err(Error::Payload),
        }
    }
    if done != 1 || request != 1 {
        return Err(Error::Payload);
    }
    Ok(())
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
            pixel_glink_channel::ACK[i - 32]
        } else if i < 64 {
            rx::PACKETS[i - 40]
        } else if i < 72 {
            pixel_glink_done::DONE[i - 64]
        } else if i < 144 {
            hello::PACKET[i - 72]
        } else if staged && i < 168 {
            PACKETS[i - 144]
        } else {
            0
        };
        if m.read8(TX.offset + i) != want {
            return Err(Error::Publication);
        }
    }
    Ok(())
}
fn prefix(m: &impl Read, s: &receive::Report, c: &ControlCapture) -> Result<(), Error> {
    if s.available < 16 {
        return Err(Error::Bounds);
    }
    let item = s.inventory.items[2].ok_or(Error::ItemSize)?;
    for i in 0..16 {
        if m.read8(item.offset + 656 + i) != c.bytes[i] {
            return Err(Error::Unstable);
        }
    }
    Ok(())
}
fn buffers(first: &Buffer, saved: &[u8; 52], next: &Buffer) -> Result<(), Error> {
    if !first.verified(saved) || !next.empty() {
        return Err(Error::Publication);
    }
    Ok(())
}
fn submit(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    prior: &hello::Report,
    c: &ControlCapture,
    first: &Buffer,
    saved: &[u8; 52],
    next: &Buffer,
    io: &mut impl Io,
    r: &mut Report,
) -> Result<receive::Report, Error> {
    r.stage = 1;
    plan(prior, c)?;
    buffers(first, saved, next)?;
    r.stage = 2;
    let before = state(m, base, ready, 144, 144, 672)?;
    if before.words != prior.words {
        return Err(Error::Unstable);
    }
    prefix(m, &before, c)?;
    tx(m, false)?;
    sync();
    prefix(m, &before, c)?;
    tx(m, false)?;
    buffers(first, saved, next)?;
    let checked = state(m, base, ready, 144, 144, 672)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.stage = 3;
    for (i, v) in PACKETS.into_iter().enumerate() {
        io.byte(i, v);
        r.byte_writes += 1
    }
    r.stage = 4;
    sync();
    tx(m, true)?;
    sync();
    tx(m, true)?;
    let staged = state(m, base, ready, 144, 144, 672)?;
    prefix(m, &staged, c)?;
    buffers(first, saved, next)?;
    if staged.words != before.words {
        return Err(Error::Unstable);
    }
    let checked = state(m, base, ready, 144, 144, 672)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.stage = 5;
    io.head();
    r.head_store = true;
    r.stage = 6;
    sync();
    let published = state(m, base, ready, 168, 144, 672)?;
    prefix(m, &published, c)?;
    tx(m, true)?;
    buffers(first, saved, next)?;
    state(m, base, ready, 168, published.words[0], published.words[3])
}
pub(crate) fn packet(bytes: &[u8]) -> Result<(Header, u32, u32), Error> {
    if bytes.len() < 16 || bytes.len() > MAX_PACKET {
        return Err(Error::Bounds);
    }
    let h = Header {
        command: u16::from_le_bytes(bytes[..2].try_into().unwrap()),
        param1: u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
        param2: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
    };
    let chunk = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let left = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    if h != (Header {
        command: 9,
        param1: 2,
        param2: 2,
    }) || chunk != 52
        || left != 0
    {
        return Err(Error::Payload);
    }
    if bytes.len() != 72 {
        return Err(Error::Bounds);
    }
    Ok((h, chunk, left))
}
pub(crate) fn sample(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    c: &ControlCapture,
    first: &Buffer,
    saved: &[u8; 52],
    next: &mut Buffer,
    r: &mut Report,
) -> Result<(), Error> {
    buffers(first, saved, next)?;
    let before = state(m, base, ready, 168, r.words[0], r.words[3])?;
    prefix(m, &before, c)?;
    tx(m, true)?;
    let n = (before.available as usize - 16).min(MAX_PACKET);
    let item = before.inventory.items[2].ok_or(Error::ItemSize)?;
    let mut one = [0; MAX_PACKET];
    let mut two = [0; MAX_PACKET];
    for i in 0..n {
        one[i] = m.read8(item.offset + DATA_AT + i)
    }
    sync();
    for i in 0..n {
        two[i] = m.read8(item.offset + DATA_AT + i)
    }
    if one != two {
        return Err(Error::Unstable);
    }
    sync();
    prefix(m, &before, c)?;
    tx(m, true)?;
    buffers(first, saved, next)?;
    let after = state(m, base, ready, 168, before.words[0], before.words[3])?;
    if before.words != after.words || before.available != after.available {
        return Err(Error::Unstable);
    }
    r.words = after.words;
    r.available = after.available;
    r.tx_consumed = after.words[0] == 168;
    r.guards = next.guards();
    r.first_guards = true;
    if n == 0 {
        return Ok(());
    }
    if n >= 8 {
        r.header = Some(Header {
            command: u16::from_le_bytes(one[..2].try_into().unwrap()),
            param1: u16::from_le_bytes(one[2..4].try_into().unwrap()),
            param2: u32::from_le_bytes(one[4..8].try_into().unwrap()),
        })
    }
    if n >= 16 {
        r.chunk = u32::from_le_bytes(one[8..12].try_into().unwrap());
        r.left = u32::from_le_bytes(one[12..16].try_into().unwrap())
    }
    if r.header
        != Some(Header {
            command: 9,
            param1: 2,
            param2: 2,
        })
    {
        return Err(Error::Payload);
    }
    if n < 16 {
        return Ok(());
    }
    if r.chunk != 52 || r.left != 0 {
        return Err(Error::Payload);
    }
    if n < 72 {
        return Ok(());
    }
    let (h, chunk, left) = packet(&one[..72])?;
    if !r.tx_consumed {
        return Ok(());
    }
    let checked = state(m, base, ready, 168, 168, after.words[3])?;
    if checked.words != after.words {
        return Err(Error::Unstable);
    }
    buffers(first, saved, next)?;
    r.stage = 9;
    let payload: &[u8; 52] = one[16..68].try_into().unwrap();
    next.copy52(payload);
    sync();
    if !next.verified(payload) || !first.verified(saved) {
        return Err(Error::Publication);
    }
    let checked = state(m, base, ready, 168, 168, after.words[3])?;
    prefix(m, &checked, c)?;
    tx(m, true)?;
    if checked.words != after.words {
        return Err(Error::Unstable);
    }
    sync();
    if !next.verified(payload) || !first.verified(saved) {
        return Err(Error::Publication);
    }
    let final_check = state(m, base, ready, 168, 168, checked.words[3])?;
    if final_check.words != after.words {
        return Err(Error::Unstable);
    }
    r.header = Some(h);
    r.chunk = chunk;
    r.left = left;
    r.copied = 52;
    r.guards = true;
    r.first_guards = true;
    let bytes = next.snapshot();
    for (i, v) in r.qrtr_words.iter_mut().enumerate() {
        *v = u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap())
    }
    for (i, v) in r.body_words.iter_mut().enumerate() {
        *v = u32::from_le_bytes(bytes[32 + 4 * i..36 + 4 * i].try_into().unwrap())
    }
    r.qrtr_v1 = r.qrtr_words[0] == 1 && r.qrtr_words[5] == 20;
    Ok(())
}
pub(crate) fn run(
    w: &mut Window,
    checkpoint: &mut HelloCheckpoint<'_>,
    next: &mut Buffer,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> Report {
    let (send, prior, c) = checkpoint.parts();
    let (completion, _) = send.parts();
    let (ready, base, _, _, _, first) = completion.parts();
    let saved = c.first_payload;
    let mut r = Report::new();
    let frequency = clock.frequency();
    if !(1_000_000..=1_000_000_000).contains(&frequency) {
        r.error = Some(Error::Clock);
        return r;
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
        return r;
    }
    let sent = match w.locked(|m| submit(m, base, ready, prior, c, first, &saved, next, io, &mut r))
    {
        Ok(s) => s,
        Err(e) => {
            r.error = Some(e);
            return r;
        }
    };
    r.words = sent.words;
    r.available = sent.available;
    r.stage = 7;
    sync();
    io.notify();
    r.notifications = 1;
    sync();
    for poll in 0..512 {
        r.samples = poll + 1;
        r.stage = 8;
        match w.locked(|m| sample(m, base, ready, c, first, &saved, next, &mut r)) {
            Ok(()) => {}
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
    r
}
struct NativeIo {
    ap: *mut u8,
    doorbell: *mut u32,
}
impl Io for NativeIo {
    fn byte(&mut self, i: usize, v: u8) {
        assert!(i < 24);
        assert_eq!(v, PACKETS[i]);
        unsafe { self.ap.add(TX.offset - PART + 144 + i).write_volatile(v) }
    }
    fn head(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(168u32.to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct NextOnce<'a, 'b> {
    checkpoint: HelloCheckpoint<'a>,
    next: &'b mut Buffer,
    spent: bool,
    io: NativeIo,
}
pub(crate) struct WireCapture {
    pub bytes: [u8; 80],
    pub size: usize,
}
pub struct DiscoveryCheckpoint<'a, 'b> {
    held: NextOnce<'a, 'b>,
    report: Report,
    wire: WireCapture,
}
impl<'a, 'b> DiscoveryCheckpoint<'a, 'b> {
    pub(crate) fn parts(
        &mut self,
    ) -> (&mut HelloCheckpoint<'a>, &mut Buffer, &Report, &WireCapture) {
        (
            &mut self.held.checkpoint,
            self.held.next,
            &self.report,
            &self.wire,
        )
    }
}
pub struct NextStopped<'a, 'b> {
    pub report: Report,
    _parked: NextOnce<'a, 'b>,
}
impl<'a, 'b> NextOnce<'a, 'b> {
    /// # Safety
    /// Exact exclusive uncached five-page/APCS mappings, no alias/IRQ/other
    /// AP writer. Fresh private buffer is disjoint from the retained first
    /// buffer; reserve both and this object through restart on every outcome.
    pub unsafe fn new(
        checkpoint: HelloCheckpoint<'a>,
        next: &'b mut Buffer,
        ap: *mut u8,
        doorbell: *mut u32,
    ) -> Self {
        Self {
            checkpoint,
            next,
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
        run(
            w,
            &mut self.checkpoint,
            self.next,
            &mut self.io,
            clock,
            progress,
        )
    }
    pub fn into_discovery_checkpoint(
        mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Result<DiscoveryCheckpoint<'a, 'b>, NextStopped<'a, 'b>> {
        let mut report = self.run(w, clock, progress);
        if !report.passed() {
            return Err(NextStopped {
                report,
                _parked: self,
            });
        }
        let (hello, _, control) = self.checkpoint.parts();
        let (completion, _) = hello.parts();
        let (ready, base, _, _, _, first) = completion.parts();
        let captured = w.locked(|m| {
            let before = state(m, base, ready, 168, 168, report.words[3])?;
            if !(88..=96).contains(&before.available) {
                return Err(Error::Bounds);
            }
            prefix(m, &before, control)?;
            tx(m, true)?;
            let item = before.inventory.items[2].ok_or(Error::ItemSize)?;
            let n = before.available as usize - 16;
            let mut one = [0; 80];
            let mut two = [0; 80];
            for i in 0..n {
                one[i] = m.read8(item.offset + 672 + i)
            }
            sync();
            for i in 0..n {
                two[i] = m.read8(item.offset + 672 + i)
            }
            if one != two {
                return Err(Error::Unstable);
            }
            packet(&one[..72])?;
            let mut expected = [0; 52];
            for (i, v) in report
                .qrtr_words
                .into_iter()
                .chain(report.body_words)
                .enumerate()
            {
                expected[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes())
            }
            if one[16..68] != expected
                || !self.next.verified(&expected)
                || !first.verified(&control.first_payload)
            {
                return Err(Error::Publication);
            }
            tx(m, true)?;
            prefix(m, &before, control)?;
            let after = state(m, base, ready, 168, 168, before.words[3])?;
            if before.words != after.words {
                return Err(Error::Unstable);
            }
            report.words = after.words;
            report.available = after.available;
            Ok(WireCapture {
                bytes: one,
                size: n,
            })
        });
        match captured {
            Ok(wire) => Ok(DiscoveryCheckpoint {
                held: self,
                report,
                wire,
            }),
            Err(e) => {
                report.error = Some(e);
                Err(NextStopped {
                    report,
                    _parked: self,
                })
            }
        }
    }
}
