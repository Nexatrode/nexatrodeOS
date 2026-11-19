//! Fixed one-shot experimental VERSION exchange. Only a fresh authenticated
//! preparation plus separately observed ready state can enter it. No channel,
//! WLAN, recovery, generic packet or production lifecycle capability exists.
use crate::{
    pixel_glink_boot::FreshBoot,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, Header},
    pixel_modem_status::{Clock, Window, sync},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};
// ACK the observed peer VERSION, then initiate our VERSION with no optional
// features. Both8-byte messages are fixed; no channel ID/name/data is admitted.
pub const PACKETS: [u8; 16] = [1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0];
pub const OFFER: Header = Header {
    command: 0,
    param1: 1,
    param2: 7,
};
pub const ACK: Header = Header {
    command: 1,
    param1: 1,
    param2: 0,
};
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub staged: bool,
    pub head_published: bool,
    pub offer_consumed: bool,
    pub notifications: u32,
    pub tx_consumed: bool,
    pub ack_seen: bool,
    pub samples: u32,
    pub words: [u32; 8],
    pub available: u32,
    pub header: Option<Header>,
    pub pending: Option<Result<crate::pixel_glink_pending::Snapshot, Error>>,
}
impl Report {
    fn new() -> Self {
        Self {
            error: None,
            staged: false,
            head_published: false,
            offer_consumed: false,
            notifications: 0,
            tx_consumed: false,
            ack_seen: false,
            samples: 0,
            words: [0; 8],
            available: 0,
            header: None,
            pending: None,
        }
    }
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.staged
            && self.head_published
            && self.offer_consumed
            && self.notifications == 1
            && self.tx_consumed
            && self.ack_seen
    }
}
pub(crate) trait Io {
    fn byte(&mut self, index: usize, value: u8);
    fn head(&mut self);
    fn tail(&mut self);
    fn notify(&mut self);
}
fn tx_exact(m: &impl Read) -> Result<(), Error> {
    for i in 0..TX.size {
        if m.read8(TX.offset + i) != if i < 16 { PACKETS[i] } else { 0 } {
            return Err(Error::Publication);
        }
    }
    Ok(())
}
fn same_items(a: &receive::Report, b: &receive::Report) -> bool {
    a.inventory.items == b.inventory.items
        && a.inventory.directory.route == b.inventory.directory.route
}
fn active(r: &receive::Report, old_tail: u32, consumed: bool) -> Result<(), Error> {
    if r.inventory.items[0] != Some(DESCRIPTOR) || r.inventory.items[1] != Some(TX) {
        return Err(Error::RouteChanged);
    }
    if r.words[1] != 16
        || r.words[2] != if consumed { 8 } else { 0 }
        || r.words[0] > 16
        || r.words[0] < old_tail
        || r.words[3] < 8
        || r.words[4..] != [0; 4]
    {
        return Err(Error::Outbound);
    }
    Ok(())
}
fn submit(
    m: &impl Read,
    fresh: &FreshBoot,
    ready: &ReadyObservation,
    io: &mut impl Io,
    r: &mut Report,
) -> Result<receive::Report, Error> {
    if fresh.items() != (DESCRIPTOR, TX) {
        return Err(Error::RouteChanged);
    }
    let before = fresh.inspect(m, ready)?.receive;
    r.words = before.words;
    r.available = before.available;
    r.header = before.header;
    if before.available != 8 || before.words[3] != 8 || before.header != Some(OFFER) {
        return Err(Error::Payload);
    }
    for (i, v) in PACKETS.into_iter().enumerate() {
        io.byte(i, v);
    }
    sync();
    tx_exact(m)?;
    sync();
    tx_exact(m)?;
    // Recheck live ready/identities/offer after unpublished data staging.
    let staged = receive::inspect_ref(m, ready)?;
    if !same_items(&before, &staged)
        || staged.words != before.words
        || staged.available != 8
        || staged.header != Some(OFFER)
    {
        return Err(Error::Unstable);
    }
    r.staged = true;
    io.head();
    r.head_published = true;
    sync();
    let published = receive::inspect_ref(m, ready)?;
    r.words = published.words;
    r.available = published.available;
    r.header = published.header;
    active(&published, 0, false)?;
    if !same_items(&before, &published) || published.header != Some(OFFER) {
        return Err(Error::RouteChanged);
    }
    // Consume only the verified initial8-byte offer; do not reset a FIFO.
    io.tail();
    r.offer_consumed = true;
    sync();
    let consumed = receive::inspect_ref(m, ready)?;
    r.words = consumed.words;
    r.available = consumed.available;
    r.header = consumed.header;
    active(&consumed, published.words[0], true)?;
    if !same_items(&before, &consumed) {
        return Err(Error::RouteChanged);
    }
    Ok(consumed)
}
pub(crate) fn sample(
    m: &impl Read,
    baseline: &receive::Report,
    ready: &ReadyObservation,
    last_tail: u32,
    last_head: u32,
) -> Result<receive::Report, Error> {
    let s = receive::inspect_ref(m, ready)?;
    active(&s, last_tail, true)?;
    if !same_items(baseline, &s) || s.words[3] < last_head {
        return Err(Error::RouteChanged);
    }
    tx_exact(m)?;
    sync();
    let after = receive::inspect_ref(m, ready)?;
    active(&after, s.words[0], true)?;
    if !same_items(baseline, &after) || after.words[3] < s.words[3] {
        return Err(Error::RouteChanged);
    }
    Ok(after)
}
pub(crate) fn exchange(
    w: &mut Window,
    fresh: FreshBoot,
    ready: ReadyObservation,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> Report {
    exchange_inner::<false>(w, fresh, ready, io, clock, &mut progress).0
}
pub(crate) fn exchange_pending(
    w: &mut Window,
    fresh: FreshBoot,
    ready: ReadyObservation,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> Report {
    exchange_inner::<true>(w, fresh, ready, io, clock, &mut progress).0
}
/// Non-copyable continuation of this actual fresh boot and fixed exchange.
/// A copied Report or Snapshot cannot construct this experimental capability.
pub struct VersionCheckpoint {
    ready: ReadyObservation,
    baseline: receive::Report,
    snapshot: crate::pixel_glink_pending::Snapshot,
}
impl VersionCheckpoint {
    pub(crate) fn into_parts(
        self,
    ) -> (
        ReadyObservation,
        receive::Report,
        crate::pixel_glink_pending::Snapshot,
    ) {
        (self.ready, self.baseline, self.snapshot)
    }
}
pub(crate) fn checkpoint_exchange(
    w: &mut Window,
    fresh: FreshBoot,
    ready: ReadyObservation,
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Result<VersionCheckpoint, Report> {
    let (r, checkpoint) = exchange_inner::<true>(w, fresh, ready, io, clock, progress);
    checkpoint.ok_or(r)
}
fn exchange_inner<const CAPTURE_PENDING: bool>(
    w: &mut Window,
    fresh: FreshBoot,
    ready: ReadyObservation,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> (Report, Option<VersionCheckpoint>) {
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
    progress(&r);
    let baseline = match w.locked(|m| submit(m, &fresh, &ready, io, &mut r)) {
        Ok(s) => s,
        Err(e) => {
            r.error = Some(e);
            return (r, None);
        }
    };
    // No notification while the directory lock is held. Both AP index stores
    // and the exact FIFO readback precede this sole fixed bit12 notification.
    sync();
    io.notify();
    r.notifications = 1;
    sync();
    let mut last_tail = baseline.words[0];
    let mut last_head = baseline.words[3];
    for poll in 0..512 {
        r.samples = poll + 1;
        let sample = w.locked(|m| sample(m, &baseline, &ready, last_tail, last_head));
        match sample {
            Err(e) => {
                r.error = Some(e);
                break;
            }
            Ok(s) => {
                r.words = s.words;
                r.available = s.available;
                r.header = s.header;
                last_tail = s.words[0];
                last_head = s.words[3];
                r.tx_consumed = s.words[0] == 16;
                if let Some(header) = s.header {
                    if header != ACK {
                        r.error = Some(Error::Payload);
                        break;
                    }
                    r.ack_seen = true;
                }
                if r.tx_consumed && r.ack_seen {
                    break;
                }
            }
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
            ::core::hint::spin_loop();
        }
        if !advanced {
            r.error = Some(Error::Clock);
            break;
        }
    }
    if !r.passed() && r.error.is_none() {
        r.error = Some(Error::Timeout)
    }
    if CAPTURE_PENDING && r.notifications == 1 && r.tx_consumed && r.header.is_some() {
        r.pending = Some(w.locked(|m| {
            crate::pixel_glink_pending::read(m, &baseline, &ready, r.words[0], r.words[3])
        }));
    }
    let checkpoint = if CAPTURE_PENDING
        && r.tx_consumed
        && r.notifications == 1
        && matches!(r.error, None | Some(Error::Payload))
    {
        match r.pending {
            Some(Ok(s))
                if s.parsed.ack_in_prefix
                    && s.parsed.error.is_none()
                    && !s.parsed.unknown_command
                    && s.parsed.parsed_bytes == s.captured
                    && s.captured as u32 == s.available =>
            {
                Some(VersionCheckpoint {
                    ready,
                    baseline,
                    snapshot: s,
                })
            }
            _ => None,
        }
    } else {
        None
    };
    (r, checkpoint)
}
struct NativeIo {
    ap: *mut u8,
    doorbell: *mut u32,
}
impl Io for NativeIo {
    fn byte(&mut self, index: usize, value: u8) {
        assert!(index < 16);
        assert_eq!(value, PACKETS[index]);
        unsafe { self.ap.add(TX.offset - PART + index).write_volatile(value) }
    }
    fn head(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(16u32.to_le())
        }
    }
    fn tail(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 8)
                .cast::<u32>()
                .write_volatile(8u32.to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct VersionOnce {
    fresh: Option<FreshBoot>,
    io: NativeIo,
}
impl VersionOnce {
    /// # Safety
    /// Exclusive uncached RW AP page0x8611d000 and APCS page0x17c00000 from
    /// the exact five-page experiment manifest. No alias, other AP client or
    /// IRQ handler. The fresh evidence must come from this actual boot; the
    /// caller transfers its separate ready observation to run exactly once.
    pub unsafe fn new(fresh: FreshBoot, ap: *mut u8, doorbell: *mut u32) -> Self {
        Self {
            fresh: Some(fresh),
            io: NativeIo { ap, doorbell },
        }
    }
    pub fn run(
        &mut self,
        w: &mut Window,
        ready: ReadyObservation,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Report {
        let Some(fresh) = self.fresh.take() else {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return r;
        };
        exchange(w, fresh, ready, &mut self.io, clock, progress)
    }
    pub fn run_pending(
        &mut self,
        w: &mut Window,
        ready: ReadyObservation,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Report {
        let Some(fresh) = self.fresh.take() else {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return r;
        };
        exchange_pending(w, fresh, ready, &mut self.io, clock, progress)
    }
    pub fn into_open_checkpoint(
        mut self,
        w: &mut Window,
        ready: ReadyObservation,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Result<VersionCheckpoint, Report> {
        let Some(fresh) = self.fresh.take() else {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return Err(r);
        };
        checkpoint_exchange(w, fresh, ready, &mut self.io, clock, progress)
    }
}
