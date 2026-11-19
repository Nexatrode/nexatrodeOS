//! One non-reusable completion for the actual guarded first receive.
//! Consume only the revalidated648-byte queue, send fixed RX_DONE6/1/1.
//! No new intent, outgoing QRTR data, buffer reuse, retry or cleanup.
use crate::{
    pixel_glink_channel::{self as channel, Capture},
    pixel_glink_open,
    pixel_glink_pending::Snapshot,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive},
    pixel_glink_rx::{self as rx, Buffer, CompletionCheckpoint},
    pixel_glink_version,
    pixel_modem_status::{Clock, Window, sync},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};
pub const DONE: [u8; 8] = [6, 0, 1, 0, 1, 0, 0, 0];
pub const CONSUMED_TAIL: u32 = 656;
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub stage: u32,
    pub byte_writes: u32,
    pub tail_store: bool,
    pub head_store: bool,
    pub notifications: u32,
    pub tx_consumed: bool,
    pub words: [u32; 8],
    pub available: u32,
    pub guards: bool,
    pub retired: bool,
    pub samples: u32,
}
impl Report {
    fn new() -> Self {
        Self {
            error: None,
            stage: 0,
            byte_writes: 0,
            tail_store: false,
            head_store: false,
            notifications: 0,
            tx_consumed: false,
            words: [0; 8],
            available: 0,
            guards: false,
            retired: false,
            samples: 0,
        }
    }
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.tail_store
            && self.head_store
            && self.notifications == 1
            && self.tx_consumed
            && self.guards
            && self.retired
    }
}
pub(crate) trait Io {
    fn byte(&mut self, i: usize, v: u8);
    fn tail(&mut self);
    fn head(&mut self);
    fn notify(&mut self);
}
fn plan(r: &rx::Report) -> Result<(), Error> {
    if !r.passed()
        || r.words != [64, 64, 8, 656, 0, 0, 0, 0]
        || r.available != 648
        || r.header
            != Some(receive::Header {
                command: 9,
                param1: 2,
                param2: 1,
            })
        || r.chunk != 52
        || r.left != 0
        || !r.qrtr_v1
        || r.qrtr_words[1] != 2
        || r.qrtr_words[3] != 0xfffffffe
        || r.qrtr_words[4] != 0
        || r.qrtr_words[7] != 0xfffffffe
        || r.control_word != 2
    {
        return Err(Error::Payload);
    }
    Ok(())
}
fn state(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    head: u32,
    tail: u32,
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
        || s.words[2] != tail
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
        } else if staged && i < 72 {
            DONE[i - 64]
        } else {
            0
        };
        if m.read8(TX.offset + i) != want {
            return Err(Error::Publication);
        }
    }
    Ok(())
}
pub(crate) fn queue(
    m: &impl Read,
    s: &receive::Report,
    old: &Snapshot,
    c: &Capture,
    buffer: &Buffer,
    prior: &rx::Report,
) -> Result<[u8; 52], Error> {
    if s.words != prior.words || s.available != 648 {
        return Err(Error::Unstable);
    }
    rx::prefix(m, s, old, c)?;
    let item = s.inventory.items[2].ok_or(Error::ItemSize)?;
    let mut one = [0; 72];
    let mut two = [0; 72];
    for i in 0..72 {
        one[i] = m.read8(item.offset + rx::DATA_AT + i)
    }
    sync();
    for i in 0..72 {
        two[i] = m.read8(item.offset + rx::DATA_AT + i)
    }
    if one != two {
        return Err(Error::Unstable);
    }
    rx::packet(&one)?;
    if !buffer.verified(&one[16..68]) {
        return Err(Error::Publication);
    }
    for (i, v) in prior.qrtr_words.iter().enumerate() {
        if *v != u32::from_le_bytes(one[16 + 4 * i..20 + 4 * i].try_into().unwrap()) {
            return Err(Error::Unstable);
        }
    }
    if prior.control_word != u32::from_le_bytes(one[48..52].try_into().unwrap()) {
        return Err(Error::Unstable);
    }
    rx::prefix(m, s, old, c)?;
    Ok(one[16..68].try_into().unwrap())
}
fn submit(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    old: &Snapshot,
    c: &Capture,
    prior: &rx::Report,
    buffer: &Buffer,
    io: &mut impl Io,
    r: &mut Report,
) -> Result<[u8; 52], Error> {
    r.stage = 1;
    plan(prior)?;
    r.stage = 2;
    let before = state(m, base, ready, 64, 8, 64, 656)?;
    let saved = queue(m, &before, old, c, buffer, prior)?;
    tx(m, false)?;
    sync();
    tx(m, false)?;
    queue(m, &before, old, c, buffer, prior)?;
    let checked = state(m, base, ready, 64, 8, 64, 656)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.stage = 3;
    for (i, v) in DONE.into_iter().enumerate() {
        io.byte(i, v);
        r.byte_writes += 1
    }
    r.stage = 4;
    sync();
    tx(m, true)?;
    sync();
    tx(m, true)?;
    let staged = state(m, base, ready, 64, 8, 64, 656)?;
    queue(m, &staged, old, c, buffer, prior)?;
    let checked = state(m, base, ready, 64, 8, 64, 656)?;
    if checked.words != before.words || !buffer.verified(&saved) {
        return Err(Error::Unstable);
    }
    // After publishing the tail, those RX bytes belong to the peer. Never
    // reread the consumed prefix or packet, even for later verification.
    r.stage = 5;
    io.tail();
    r.tail_store = true;
    sync();
    let consumed = state(m, base, ready, 64, CONSUMED_TAIL, 64, 656)?;
    tx(m, true)?;
    if !buffer.verified(&saved) {
        return Err(Error::Publication);
    }
    let checked = state(m, base, ready, 64, CONSUMED_TAIL, 64, consumed.words[3])?;
    if checked.words != consumed.words {
        return Err(Error::Unstable);
    }
    r.stage = 6;
    io.head();
    r.head_store = true;
    sync();
    let published = state(m, base, ready, 72, CONSUMED_TAIL, 64, checked.words[3])?;
    tx(m, true)?;
    if !buffer.verified(&saved) {
        return Err(Error::Publication);
    }
    let final_check = state(
        m,
        base,
        ready,
        72,
        CONSUMED_TAIL,
        published.words[0],
        published.words[3],
    )?;
    r.words = final_check.words;
    r.available = final_check.available;
    Ok(saved)
}
pub(crate) fn sample(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    saved: &[u8; 52],
    buffer: &Buffer,
    r: &mut Report,
) -> Result<(), Error> {
    let before = state(m, base, ready, 72, CONSUMED_TAIL, r.words[0], r.words[3])?;
    tx(m, true)?;
    if !buffer.verified(saved) {
        return Err(Error::Publication);
    }
    sync();
    tx(m, true)?;
    if !buffer.verified(saved) {
        return Err(Error::Publication);
    }
    let after = state(
        m,
        base,
        ready,
        72,
        CONSUMED_TAIL,
        before.words[0],
        before.words[3],
    )?;
    if before.words != after.words {
        return Err(Error::Unstable);
    }
    r.words = after.words;
    r.available = after.available;
    r.guards = true;
    r.tx_consumed = after.words[0] == 72;
    r.retired = r.tx_consumed;
    Ok(())
}
pub(crate) fn run(
    w: &mut Window,
    checkpoint: &mut CompletionCheckpoint<'_>,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> Report {
    let (ready, base, old, c, prior, buffer) = checkpoint.parts();
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
    let saved = match w.locked(|m| submit(m, base, ready, old, c, prior, buffer, io, &mut r)) {
        Ok(s) => s,
        Err(e) => {
            r.error = Some(e);
            return r;
        }
    };
    r.stage = 7;
    sync();
    io.notify();
    r.notifications = 1;
    sync();
    for poll in 0..512 {
        r.samples = poll + 1;
        r.stage = 8;
        match w.locked(|m| sample(m, base, ready, &saved, buffer, &mut r)) {
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
        assert!(i < 8);
        assert_eq!(v, DONE[i]);
        unsafe { self.ap.add(TX.offset - PART + 64 + i).write_volatile(v) }
    }
    fn tail(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 8)
                .cast::<u32>()
                .write_volatile(CONSUMED_TAIL.to_le())
        }
    }
    fn head(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(72u32.to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct CompleteOnce<'a> {
    checkpoint: CompletionCheckpoint<'a>,
    spent: bool,
    io: NativeIo,
}
/// Actual consumed completion plus the retained exchange and buffer lease.
pub struct SendCheckpoint<'a> {
    completion: CompletionCheckpoint<'a>,
    report: Report,
}
impl<'a> SendCheckpoint<'a> {
    pub(crate) fn parts(&mut self) -> (&mut CompletionCheckpoint<'a>, &Report) {
        (&mut self.completion, &self.report)
    }
}
pub struct CompletionStopped<'a> {
    pub report: Report,
    _parked: CompleteOnce<'a>,
}
impl<'a> CompleteOnce<'a> {
    /// # Safety
    /// Exclusive uncached five-page/APCS experiment mappings, no other AP
    /// client or IRQ. Keep this object and its private buffer until restart.
    pub unsafe fn new(
        checkpoint: CompletionCheckpoint<'a>,
        ap: *mut u8,
        doorbell: *mut u32,
    ) -> Self {
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
    pub fn into_send_checkpoint(
        mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Result<SendCheckpoint<'a>, CompletionStopped<'a>> {
        let report = self.run(w, clock, progress);
        if !report.passed() {
            return Err(CompletionStopped {
                report,
                _parked: self,
            });
        }
        Ok(SendCheckpoint {
            completion: self.checkpoint,
            report,
        })
    }
}
