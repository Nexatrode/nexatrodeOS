//! One fixed64-byte local intent and positive request ACK; one bounded packet.
//! No RX-tail write, RX_DONE, outgoing QRTR payload, intent reuse or host DMA.
use crate::{
    pixel_glink_channel::{self as channel, Capture, ChannelCheckpoint},
    pixel_glink_intents, pixel_glink_open,
    pixel_glink_pending::Snapshot,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, Header},
    pixel_glink_version,
    pixel_modem_status::{Clock, Window, sync},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};
pub const PACKETS: [u8; 24] = [
    5, 0, 1, 0, 1, 0, 0, 0, 64, 0, 0, 0, 1, 0, 0, 0, 8, 0, 1, 0, 1, 0, 0, 0,
];
pub const DATA_AT: usize = 584;
pub const MAX_PACKET: usize = 80;
#[repr(C)]
pub struct Buffer {
    before: [u8; 16],
    data: [u8; 64],
    after: [u8; 16],
}
impl Buffer {
    pub const fn new() -> Self {
        Self {
            before: [0xa5; 16],
            data: [0; 64],
            after: [0x5a; 16],
        }
    }
    pub fn guards(&self) -> bool {
        for i in 0..16 {
            if unsafe { self.before.as_ptr().add(i).read_volatile() } != 0xa5
                || unsafe { self.after.as_ptr().add(i).read_volatile() } != 0x5a
            {
                return false;
            }
        }
        true
    }
    fn peek(&self, i: usize) -> u8 {
        assert!(i < 64);
        unsafe { self.data.as_ptr().add(i).read_volatile() }
    }
    pub fn empty(&self) -> bool {
        self.guards() && (0..64).all(|i| self.peek(i) == 0)
    }
    pub(crate) fn verified(&self, expected: &[u8]) -> bool {
        expected.len() == 52
            && self.guards()
            && (0..64).all(|i| self.peek(i) == if i < 52 { expected[i] } else { 0 })
    }
    pub fn bytes(&self) -> &[u8; 64] {
        &self.data
    }
    pub(crate) fn snapshot(&self) -> [u8; 52] {
        let mut b = [0; 52];
        for (i, v) in b.iter_mut().enumerate() {
            *v = self.peek(i)
        }
        b
    }
    pub(crate) fn copy52(&mut self, bytes: &[u8; 52]) {
        for (i, v) in bytes.iter().enumerate() {
            unsafe { self.data.as_mut_ptr().add(i).write_volatile(*v) }
        }
    }
    pub(crate) fn copy_exact(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > 64 {
            return false;
        }
        for i in 0..64 {
            let v = bytes.get(i).copied().unwrap_or(0);
            unsafe { self.data.as_mut_ptr().add(i).write_volatile(v) }
        }
        true
    }
    pub(crate) fn verified_exact(&self, bytes: &[u8]) -> bool {
        bytes.len() <= 64
            && self.guards()
            && (0..64).all(|i| self.peek(i) == bytes.get(i).copied().unwrap_or(0))
    }
}
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
    pub qrtr_words: [u32; 8],
    pub control_word: u32,
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
            qrtr_words: [0; 8],
            control_word: 0,
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
    }
}
pub(crate) trait Io {
    fn byte(&mut self, i: usize, v: u8);
    fn head(&mut self);
    fn notify(&mut self);
}
fn plan(old: &Snapshot, c: &Capture, r: &channel::Report) -> Result<(), Error> {
    pixel_glink_open::plan(old)?;
    if !r.passed()
        || r.words != [40, 40, 8, 584, 0, 0, 0, 0]
        || c.state.words != r.words
        || r.available != 576
        || c.size != 488
    {
        return Err(Error::Payload);
    }
    let s = pixel_glink_intents::inspect(&c.bytes[..c.size]);
    if s.error.is_some() || s.parsed_bytes != 488 {
        return Err(Error::Payload);
    }
    let i = s.inventory;
    if (
        i.records,
        i.intent_records,
        i.entries,
        i.intent_requests,
        i.last_request_size,
        i.min_id,
        i.max_id,
        i.duplicate_ids,
        i.zero_sizes,
    ) != (31, 30, 30, 1, 52, 0, 29, 0, 0)
        || i.group_count != 4
    {
        return Err(Error::Payload);
    }
    for (g, (size, count)) in i.groups[..4]
        .iter()
        .zip([(128, 16), (512, 8), (1152, 5), (8320, 1)])
    {
        if (g.size, g.count) != (size, count) {
            return Err(Error::Payload);
        }
    }
    Ok(())
}
fn state(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    head: u32,
    last_tail: u32,
    last_rx: u32,
) -> Result<receive::Report, Error> {
    let s = receive::inspect_ref(m, ready)?;
    if s.inventory.items != base.inventory.items
        || s.inventory.directory.route != base.inventory.directory.route
    {
        return Err(Error::RouteChanged);
    }
    if s.words[1] != head
        || s.words[2] != 8
        || s.words[0] < last_tail
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
        } else if staged && i < 64 {
            PACKETS[i - 40]
        } else {
            0
        };
        if m.read8(TX.offset + i) != want {
            return Err(Error::Publication);
        }
    }
    Ok(())
}
pub(crate) fn prefix(
    m: &impl Read,
    s: &receive::Report,
    old: &Snapshot,
    c: &Capture,
) -> Result<(), Error> {
    if s.available < 576 {
        return Err(Error::Bounds);
    }
    let rx = s.inventory.items[2].ok_or(Error::ItemSize)?;
    for i in 0..576 {
        let want = if i < 80 {
            old.bytes[i]
        } else if i < 88 {
            [4, 0, 1, 0, 0, 0, 0, 0][i - 80]
        } else {
            c.bytes[i - 88]
        };
        if m.read8(rx.offset + 8 + i) != want {
            return Err(Error::Unstable);
        }
    }
    Ok(())
}
fn submit(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    old: &Snapshot,
    c: &Capture,
    prior: &channel::Report,
    buffer: &Buffer,
    io: &mut impl Io,
    r: &mut Report,
) -> Result<receive::Report, Error> {
    r.stage = 1;
    plan(old, c, prior)?;
    if !buffer.empty() {
        return Err(Error::Publication);
    }
    r.stage = 2;
    let before = state(m, base, ready, 40, 40, 584)?;
    if before.words != c.state.words || before.available != 576 {
        return Err(Error::Unstable);
    }
    prefix(m, &before, old, c)?;
    tx(m, false)?;
    sync();
    prefix(m, &before, old, c)?;
    tx(m, false)?;
    let checked = state(m, base, ready, 40, 40, 584)?;
    if checked.words != before.words || !buffer.empty() {
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
    let staged = state(m, base, ready, 40, 40, 584)?;
    prefix(m, &staged, old, c)?;
    if staged.words != before.words || !buffer.empty() {
        return Err(Error::Unstable);
    }
    let checked = state(m, base, ready, 40, 40, 584)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.stage = 5;
    io.head();
    r.head_store = true;
    r.stage = 6;
    sync();
    let published = state(m, base, ready, 64, 40, 584)?;
    prefix(m, &published, old, c)?;
    tx(m, true)?;
    if !buffer.empty() {
        return Err(Error::Publication);
    }
    state(m, base, ready, 64, published.words[0], published.words[3])
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
        param2: 1,
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
    old: &Snapshot,
    c: &Capture,
    buffer: &mut Buffer,
    r: &mut Report,
) -> Result<(), Error> {
    if !buffer.empty() {
        return Err(Error::Publication);
    }
    let before = state(m, base, ready, 64, r.words[0], r.words[3])?;
    prefix(m, &before, old, c)?;
    tx(m, true)?;
    let n = (before.available as usize - 576).min(MAX_PACKET);
    let rx = before.inventory.items[2].ok_or(Error::ItemSize)?;
    let mut one = [0; MAX_PACKET];
    let mut two = [0; MAX_PACKET];
    for (i, v) in one[..n].iter_mut().enumerate() {
        *v = m.read8(rx.offset + DATA_AT + i)
    }
    sync();
    for (i, v) in two[..n].iter_mut().enumerate() {
        *v = m.read8(rx.offset + DATA_AT + i)
    }
    if one != two {
        return Err(Error::Unstable);
    }
    sync();
    prefix(m, &before, old, c)?;
    tx(m, true)?;
    let after = state(m, base, ready, 64, before.words[0], before.words[3])?;
    if before.words != after.words || before.available != after.available {
        return Err(Error::Unstable);
    }
    r.words = after.words;
    r.available = after.available;
    r.tx_consumed = after.words[0] == 64;
    r.guards = buffer.guards();
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
            param2: 1,
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
    // The first frame is self-delimiting. Later queued control bytes remain
    // unclassified and unconsumed; they do not widen this receive operation.
    let (h, chunk, left) = packet(&one[..72])?;
    if !r.tx_consumed {
        return Ok(());
    }
    // Revalidate readiness after copied-header classification and before the
    // sole local copy. No peer-controlled size/address selects a destination.
    let final_state = state(m, base, ready, 64, 64, after.words[3])?;
    if final_state.words != after.words || !buffer.empty() {
        return Err(Error::Unstable);
    }
    r.stage = 9;
    for i in 0..52 {
        unsafe { buffer.data.as_mut_ptr().add(i).write_volatile(one[16 + i]) }
    }
    sync();
    if !buffer.verified(&one[16..68]) {
        return Err(Error::Publication);
    }
    let checked = state(m, base, ready, 64, 64, after.words[3])?;
    prefix(m, &checked, old, c)?;
    tx(m, true)?;
    if checked.words != after.words {
        return Err(Error::Unstable);
    }
    sync();
    if !buffer.verified(&one[16..68]) {
        return Err(Error::Publication);
    }
    let final_check = state(m, base, ready, 64, 64, checked.words[3])?;
    if final_check.words != after.words {
        return Err(Error::Unstable);
    }
    r.header = Some(h);
    r.chunk = chunk;
    r.left = left;
    r.copied = 52;
    r.guards = true;
    for (i, v) in r.qrtr_words.iter_mut().enumerate() {
        *v = u32::from_le_bytes([
            buffer.peek(4 * i),
            buffer.peek(4 * i + 1),
            buffer.peek(4 * i + 2),
            buffer.peek(4 * i + 3),
        ]);
    }
    r.control_word = u32::from_le_bytes([
        buffer.peek(32),
        buffer.peek(33),
        buffer.peek(34),
        buffer.peek(35),
    ]);
    r.qrtr_v1 = r.qrtr_words[0] == 1 && r.qrtr_words[5] == 20;
    Ok(())
}
pub(crate) fn run(
    w: &mut Window,
    checkpoint: ChannelCheckpoint,
    buffer: &mut Buffer,
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Report {
    let (ready, base, old, c, prior) = checkpoint.into_parts();
    run_parts(
        w, &ready, &base, &old, &c, &prior, buffer, io, clock, progress,
    )
}
fn run_parts(
    w: &mut Window,
    ready: &ReadyObservation,
    base: &receive::Report,
    old: &Snapshot,
    c: &Capture,
    prior: &channel::Report,
    buffer: &mut Buffer,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> Report {
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
    let sent = match w.locked(|m| submit(m, &base, &ready, &old, &c, &prior, buffer, io, &mut r)) {
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
        match w.locked(|m| sample(m, &base, &ready, &old, &c, buffer, &mut r)) {
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
        unsafe { self.ap.add(TX.offset - PART + 40 + i).write_volatile(v) }
    }
    fn head(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(64u32.to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct ReceiveOnce<'a> {
    checkpoint: Option<ChannelCheckpoint>,
    buffer: &'a mut Buffer,
    io: NativeIo,
}
/// The live successful receive and its unique buffer lease, never a copied report.
pub struct CompletionCheckpoint<'a> {
    ready: ReadyObservation,
    base: receive::Report,
    old: Snapshot,
    c: Capture,
    report: Report,
    buffer: &'a mut Buffer,
}
impl CompletionCheckpoint<'_> {
    pub(crate) fn parts(
        &mut self,
    ) -> (
        &ReadyObservation,
        &receive::Report,
        &Snapshot,
        &Capture,
        &Report,
        &mut Buffer,
    ) {
        (
            &self.ready,
            &self.base,
            &self.old,
            &self.c,
            &self.report,
            self.buffer,
        )
    }
}
pub struct ReceiveStopped<'a> {
    pub report: Report,
    _buffer: &'a mut Buffer,
}
impl<'a> ReceiveOnce<'a> {
    /// # Safety
    /// Exact exclusive uncached five-page SMEM/APCS experiment mappings, no
    /// aliases/IRQ/other AP client. Buffer is unique private PD RAM reserved
    /// through restart; its address is never transmitted to the modem.
    pub unsafe fn new(
        checkpoint: ChannelCheckpoint,
        buffer: &'a mut Buffer,
        ap: *mut u8,
        doorbell: *mut u32,
    ) -> Self {
        Self {
            checkpoint: Some(checkpoint),
            buffer,
            io: NativeIo { ap, doorbell },
        }
    }
    pub fn run(
        &mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Report {
        let Some(checkpoint) = self.checkpoint.take() else {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return r;
        };
        run(w, checkpoint, self.buffer, &mut self.io, clock, progress)
    }
    pub fn into_completion_checkpoint(
        mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Result<CompletionCheckpoint<'a>, ReceiveStopped<'a>> {
        let Some(checkpoint) = self.checkpoint.take() else {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return Err(ReceiveStopped {
                report: r,
                _buffer: self.buffer,
            });
        };
        let (ready, base, old, c, prior) = checkpoint.into_parts();
        let report = run_parts(
            w,
            &ready,
            &base,
            &old,
            &c,
            &prior,
            self.buffer,
            &mut self.io,
            clock,
            progress,
        );
        if !report.passed() {
            return Err(ReceiveStopped {
                report,
                _buffer: self.buffer,
            });
        }
        Ok(CompletionCheckpoint {
            ready,
            base,
            old,
            c,
            report,
            buffer: self.buffer,
        })
    }
}
