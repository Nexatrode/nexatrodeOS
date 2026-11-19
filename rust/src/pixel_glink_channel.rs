//! Acknowledge only the proven IPCRTR peer OPEN, then inspect control replies.
//! No receive-tail store, intent advertisement, QRTR data or general send API.
use crate::{
    pixel_glink_open::{self as open, OpenCheckpoint},
    pixel_glink_pending::Snapshot,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, Header},
    pixel_glink_version as version,
    pixel_modem_status::{Clock, Window, sync},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};
pub const ACK: [u8; 8] = [4, 0, 2, 0, 0, 0, 0, 0];
#[cfg(not(feature = "sunfish-glink-intents-probe"))]
pub const MAX_NEW_RX: usize = 128;
#[cfg(feature = "sunfish-glink-intents-probe")]
pub const MAX_NEW_RX: usize = crate::pixel_glink_intents::MAX_BYTES;
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub staged: bool,
    pub head_store: bool,
    pub notifications: u32,
    pub tx_consumed: bool,
    pub words: [u32; 8],
    pub available: u32,
    pub new_bytes: u32,
    pub header: Option<Header>,
    pub records: u32,
    pub intent_records: u32,
    pub intent_entries: u32,
    pub samples: u32,
    pub byte_writes: u32,
    pub guard_stage: u32,
    #[cfg(feature = "sunfish-glink-intents-probe")]
    pub intents: Option<crate::pixel_glink_intents::Inventory>,
    #[cfg(feature = "sunfish-glink-intents-probe")]
    pub intent_scan: Option<crate::pixel_glink_intents::Scan>,
}
impl Report {
    fn new() -> Self {
        Self {
            error: None,
            staged: false,
            head_store: false,
            notifications: 0,
            tx_consumed: false,
            words: [0; 8],
            available: 0,
            new_bytes: 0,
            header: None,
            records: 0,
            intent_records: 0,
            intent_entries: 0,
            samples: 0,
            byte_writes: 0,
            guard_stage: 0,
            #[cfg(feature = "sunfish-glink-intents-probe")]
            intents: None,
            #[cfg(feature = "sunfish-glink-intents-probe")]
            intent_scan: None,
        }
    }
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.staged
            && self.head_store
            && self.notifications == 1
            && self.tx_consumed
    }
}
pub(crate) trait Io {
    fn byte(&mut self, index: usize, value: u8);
    fn head(&mut self);
    fn notify(&mut self);
}
fn plan(old: &Snapshot, request: &open::Report) -> Result<(), Error> {
    open::plan(old)?;
    if !request.passed()
        || request.words != [32, 32, 8, 96, 0, 0, 0, 0]
        || request.available != 88
        || request.new_bytes != 8
        || request.header != Some(open::OPEN_ACK)
    {
        return Err(Error::Payload);
    }
    Ok(())
}
fn tx(m: &impl Read, staged: bool) -> Result<(), Error> {
    for i in 0..TX.size {
        let want = if i < 16 {
            version::PACKETS[i]
        } else if i < 32 {
            open::REQUEST[i - 16]
        } else if staged && i < 40 {
            ACK[i - 32]
        } else {
            0
        };
        if m.read8(TX.offset + i) != want {
            return Err(Error::Publication);
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
    last_head: u32,
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
        || s.words[3] < last_head
        || s.words[4..] != [0; 4]
    {
        return Err(Error::Outbound);
    }
    Ok(s)
}
fn prefix(m: &impl Read, s: &receive::Report, old: &Snapshot) -> Result<(), Error> {
    if s.available < 88 {
        return Err(Error::Bounds);
    }
    let rx = s.inventory.items[2].ok_or(Error::ItemSize)?;
    for i in 0..88 {
        let want = if i < 80 {
            old.bytes[i]
        } else {
            [4, 0, 1, 0, 0, 0, 0, 0][i - 80]
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
    request: &open::Report,
    io: &mut impl Io,
    r: &mut Report,
) -> Result<receive::Report, Error> {
    r.guard_stage = 1;
    plan(old, request)?;
    r.guard_stage = 2;
    let before = state(m, base, ready, 32, 32, 96)?;
    if before.words != request.words || before.available != 88 {
        return Err(Error::Unstable);
    }
    prefix(m, &before, old)?;
    tx(m, false)?;
    sync();
    prefix(m, &before, old)?;
    tx(m, false)?;
    let checked = state(m, base, ready, 32, 32, 96)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.guard_stage = 3;
    for (i, v) in ACK.into_iter().enumerate() {
        io.byte(i, v);
        r.byte_writes += 1
    }
    r.guard_stage = 4;
    sync();
    tx(m, true)?;
    sync();
    tx(m, true)?;
    let staged = state(m, base, ready, 32, 32, 96)?;
    prefix(m, &staged, old)?;
    if staged.words != before.words {
        return Err(Error::Unstable);
    }
    let checked = state(m, base, ready, 32, 32, 96)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.staged = true;
    r.guard_stage = 5;
    io.head();
    r.head_store = true;
    r.guard_stage = 6;
    sync();
    let published = state(m, base, ready, 40, 32, 96)?;
    prefix(m, &published, old)?;
    tx(m, true)?;
    state(m, base, ready, 40, published.words[0], published.words[3])
}
/// Parse only bounded control metadata; intent size/ID words are never used.
pub(crate) fn controls(bytes: &[u8]) -> Result<(u32, u32, u32), Error> {
    if bytes.len() > MAX_NEW_RX || bytes.len() % 8 != 0 {
        return Err(Error::Bounds);
    }
    let (mut at, mut records, mut intents, mut entries) = (0, 0, 0, 0);
    while at < bytes.len() {
        let b = &bytes[at..at + 8];
        let command = u16::from_le_bytes(b[..2].try_into().unwrap());
        let cid = u16::from_le_bytes(b[2..4].try_into().unwrap());
        let count = u32::from_le_bytes(b[4..8].try_into().unwrap());
        let size = match command {
            5 => {
                if cid != 2 || count == 0 || count > 15 {
                    return Err(Error::Payload);
                }
                intents += 1;
                entries += count;
                8 + 8 * count as usize
            }
            13 => 8,
            15 => {
                if cid != 2 {
                    return Err(Error::Payload);
                }
                8
            }
            _ => return Err(Error::Payload),
        };
        if size > bytes.len() - at {
            return Err(Error::Bounds);
        }
        at += size;
        records += 1;
    }
    Ok((records, intents, entries))
}
pub(crate) fn sample(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    old: &Snapshot,
    last_tail: u32,
    last_head: u32,
    r: &mut Report,
) -> Result<(), Error> {
    sample_capture(m, base, ready, old, last_tail, last_head, r).map(|_| ())
}
pub(crate) struct Capture {
    pub bytes: [u8; MAX_NEW_RX],
    pub size: usize,
    pub state: receive::Report,
}
/// Owned continuation of this actual fixed exchange; copied reports cannot mint it.
pub struct ChannelCheckpoint {
    ready: ReadyObservation,
    baseline: receive::Report,
    old: Snapshot,
    capture: Capture,
    report: Report,
}
impl ChannelCheckpoint {
    pub const fn generation(&self) -> u64 {
        self.ready.generation()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (ReadyObservation, receive::Report, Snapshot, Capture, Report) {
        (
            self.ready,
            self.baseline,
            self.old,
            self.capture,
            self.report,
        )
    }
}
fn sample_capture(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    old: &Snapshot,
    last_tail: u32,
    last_head: u32,
    r: &mut Report,
) -> Result<Capture, Error> {
    #[cfg(feature = "sunfish-glink-intents-probe")]
    {
        r.intents = None;
        r.intent_scan = None;
    }
    let before = state(m, base, ready, 40, last_tail, last_head)?;
    prefix(m, &before, old)?;
    tx(m, true)?;
    let n = (before.available as usize - 88).min(MAX_NEW_RX);
    let rx = before.inventory.items[2].ok_or(Error::ItemSize)?;
    let mut one = [0; MAX_NEW_RX];
    let mut two = [0; MAX_NEW_RX];
    for (i, v) in one[..n].iter_mut().enumerate() {
        *v = m.read8(rx.offset + 96 + i)
    }
    sync();
    for (i, v) in two[..n].iter_mut().enumerate() {
        *v = m.read8(rx.offset + 96 + i)
    }
    if one != two {
        return Err(Error::Unstable);
    }
    sync();
    prefix(m, &before, old)?;
    tx(m, true)?;
    let after = state(m, base, ready, 40, before.words[0], before.words[3])?;
    if before.words != after.words || before.available != after.available {
        return Err(Error::Unstable);
    }
    r.words = after.words;
    r.available = after.available;
    r.new_bytes = n as u32;
    r.tx_consumed = after.words[0] == 40;
    r.header = if n >= 8 {
        Some(Header {
            command: u16::from_le_bytes(one[..2].try_into().unwrap()),
            param1: u16::from_le_bytes(one[2..4].try_into().unwrap()),
            param2: u32::from_le_bytes(one[4..8].try_into().unwrap()),
        })
    } else {
        None
    };
    #[cfg(not(feature = "sunfish-glink-intents-probe"))]
    let (records, intents, entries) = controls(&one[..n])?;
    #[cfg(feature = "sunfish-glink-intents-probe")]
    let (records, intents, entries) = {
        let scan = crate::pixel_glink_intents::inspect(&one[..n]);
        let i = scan.inventory;
        r.intent_scan = Some(scan);
        r.records = i.records;
        r.intent_records = i.intent_records;
        r.intent_entries = i.entries;
        if let Some(e) = scan.error {
            return Err(e);
        }
        r.intents = Some(i);
        (i.records, i.intent_records, i.entries)
    };
    r.records = records;
    r.intent_records = intents;
    r.intent_entries = entries;
    Ok(Capture {
        bytes: one,
        size: n,
        state: after,
    })
}
pub(crate) fn run(
    w: &mut Window,
    checkpoint: OpenCheckpoint,
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Report {
    run_inner(w, checkpoint, io, clock, progress).0
}
pub(crate) fn checkpoint_run(
    w: &mut Window,
    checkpoint: OpenCheckpoint,
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Result<ChannelCheckpoint, Report> {
    let (r, next) = run_inner(w, checkpoint, io, clock, progress);
    next.ok_or(r)
}
fn run_inner(
    w: &mut Window,
    checkpoint: OpenCheckpoint,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> (Report, Option<ChannelCheckpoint>) {
    let (ready, base, old, request) = checkpoint.into_parts();
    let mut r = Report::new();
    let frequency = clock.frequency();
    if !(1_000_000..=1_000_000_000).contains(&frequency) {
        r.error = Some(Error::Clock);
        return (r, None);
    }
    let start = clock.now();
    let mut advancing = false;
    for _ in 0..10000 {
        if clock.now() != start {
            advancing = true;
            break;
        }
    }
    if !advancing {
        r.error = Some(Error::Clock);
        return (r, None);
    }
    let sent = match w.locked(|m| submit(m, &base, &ready, &old, &request, io, &mut r)) {
        Ok(s) => s,
        Err(e) => {
            r.error = Some(e);
            return (r, None);
        }
    };
    r.words = sent.words;
    r.available = sent.available;
    r.guard_stage = 7;
    sync();
    io.notify();
    r.notifications = 1;
    sync();
    let mut captured = None;
    for i in 0..512 {
        r.samples = i + 1;
        r.guard_stage = 8;
        match w.locked(|m| sample_capture(m, &base, &ready, &old, r.words[0], r.words[3], &mut r)) {
            Ok(s) => captured = Some(s),
            Err(e) => {
                r.error = Some(e);
                break;
            }
        }
        if r.tx_consumed {
            break;
        }
        if clock.now().wrapping_sub(start) >= frequency * 5 {
            r.error = Some(Error::Timeout);
            break;
        }
        if i % 25 == 0 {
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
    let next = if r.passed() {
        match captured {
            Some(capture)
                if capture.size as u32 == r.available.saturating_sub(88).min(MAX_NEW_RX as u32)
                    && r.new_bytes == r.available.saturating_sub(88).min(MAX_NEW_RX as u32) =>
            {
                Some(ChannelCheckpoint {
                    ready,
                    baseline: base,
                    old,
                    capture,
                    report: r,
                })
            }
            _ => None,
        }
    } else {
        None
    };
    (r, next)
}
struct NativeIo {
    ap: *mut u8,
    doorbell: *mut u32,
}
impl Io for NativeIo {
    fn byte(&mut self, index: usize, value: u8) {
        assert!(index < 8);
        assert_eq!(value, ACK[index]);
        unsafe {
            self.ap
                .add(TX.offset - PART + 32 + index)
                .write_volatile(value)
        }
    }
    fn head(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(40u32.to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct ChannelOnce {
    checkpoint: Option<OpenCheckpoint>,
    io: NativeIo,
}
impl ChannelOnce {
    /// # Safety
    /// Same exclusive uncached five-page SMEM and APCS mappings as OpenOnce;
    /// consume the continuation of this actual boot, with no other AP writer.
    pub unsafe fn new(checkpoint: OpenCheckpoint, ap: *mut u8, doorbell: *mut u32) -> Self {
        Self {
            checkpoint: Some(checkpoint),
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
        run(w, checkpoint, &mut self.io, clock, progress)
    }
    pub fn into_receive_checkpoint(
        mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Result<ChannelCheckpoint, Report> {
        let Some(checkpoint) = self.checkpoint.take() else {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return Err(r);
        };
        checkpoint_run(w, checkpoint, &mut self.io, clock, progress)
    }
}
