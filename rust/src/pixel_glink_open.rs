//! Single fixed IPCRTR OPEN request after a private actual version checkpoint.
//! No peer OPEN_ACK, receive-index store, intent advertisement or QRTR data.
use crate::{
    pixel_glink_pending::Snapshot,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, Header},
    pixel_glink_version::{self as version, VersionCheckpoint},
    pixel_modem_status::{Clock, Window, sync},
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};
pub const REQUEST: [u8; 16] = [
    2, 0, 1, 0, 7, 0, 0, 0, b'I', b'P', b'C', b'R', b'T', b'R', 0, 0,
];
pub const OPEN_ACK: Header = Header {
    command: 4,
    param1: 1,
    param2: 0,
};
pub const MAX_NEW_RX: usize = 128;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanPacketDiagnostic {
    pub offset: u32,
    pub command: u32,
    pub id: u32,
    pub bytes: u32,
    pub name_prefix: [u8; 12],
    pub name_len: u32,
}
const EMPTY_PLAN_PACKET: PlanPacketDiagnostic = PlanPacketDiagnostic {
    offset: 0,
    command: 0,
    id: 0,
    bytes: 0,
    name_prefix: [0; 12],
    name_len: 0,
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanShapeDiagnostic {
    pub packet: u32,
    pub expected_offset: u32,
    pub offset: u32,
    pub expected_id: u32,
    pub id: u32,
    pub expected_name_bytes: u32,
    pub name_bytes: u32,
    pub command: u32,
    pub name_prefix: [u8; 12],
    pub name_len: u32,
    pub packets: [PlanPacketDiagnostic; 4],
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub staged: bool,
    pub head_store: bool,
    pub notifications: u32,
    pub tx_consumed: bool,
    pub open_ack: bool,
    pub words: [u32; 8],
    pub available: u32,
    pub new_bytes: u32,
    pub header: Option<Header>,
    pub samples: u32,
    pub guard_stage: u32,
    pub plan_detail: u32,
    pub plan_shape: Option<PlanShapeDiagnostic>,
    pub byte_writes: u32,
}
/// Stable bounded copy of the current GLINK control queue, consumed from the
/// private version checkpoint without constructing an OPEN writer.
#[derive(Clone, Copy, Debug)]
pub struct PlanSnapshotReport {
    pub available: u32,
    pub captured: u32,
    pub words: [u32; 8],
    pub bytes: [u8; crate::pixel_glink_pending::MAX_BYTES],
    pub plan_detail: u32,
}
impl Report {
    fn new() -> Self {
        Self {
            error: None,
            staged: false,
            head_store: false,
            notifications: 0,
            tx_consumed: false,
            open_ack: false,
            words: [0; 8],
            available: 0,
            new_bytes: 0,
            header: None,
            samples: 0,
            guard_stage: 0,
            plan_detail: 0,
            plan_shape: None,
            byte_writes: 0,
        }
    }
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.staged
            && self.head_store
            && self.notifications == 1
            && self.tx_consumed
            && self.open_ack
    }
}
pub(crate) trait Io {
    fn byte(&mut self, index: usize, value: u8);
    fn head(&mut self);
    fn notify(&mut self);
}
/// 1=queue length,2=indices,3=parser,4=OPEN shape,5=target name,6=ACK.
pub(crate) fn plan_detail(s: &Snapshot) -> u32 {
    if s.available != 80 || s.captured != 80 {
        return 1;
    }
    if s.words[..4] != [16, 16, 8, 88] || s.words[4..] != [0; 4] {
        return 2;
    }
    if s.parsed.count != 4
        || s.parsed.parsed_bytes != 80
        || s.parsed.error.is_some()
        || s.parsed.unknown_command
        || !s.parsed.ack_in_prefix
    {
        return 3;
    }
    // The peer may advertise its two first channels in either order. Keep
    // their IDs, offsets, lengths, and the exact IPCRTR target shape checked;
    // the spelling of the other offer does not authorize this request.
    let first = &s.parsed.packets[0];
    let second = &s.parsed.packets[1];
    if first.offset != 0
        || first.header.command != 2
        || first.header.param1 != 1
        || !matches!(first.header.param2, 7 | 10)
        || first.name_len + 1 != first.header.param2 as usize
    {
        return 4;
    }
    let second_offset = if first.header.param2 == 7 { 16 } else { 24 };
    if second.offset != second_offset
        || second.header.command != 2
        || second.header.param1 != 2
        || !matches!(second.header.param2, 7 | 10)
        || second.name_len + 1 != second.header.param2 as usize
        || first.header.param2 == second.header.param2
    {
        return 4;
    }
    let target_matches = [first, second]
        .into_iter()
        .filter(|p| p.header.param2 == 7 && p.name_len == 6 && &p.name[..p.name_len] == b"IPCRTR")
        .count();
    if target_matches != 1 {
        return 5;
    }
    let p = &s.parsed.packets[2];
    if p.offset != 40
        || p.header
            != (Header {
                command: 2,
                param1: 3,
                param2: 19,
            })
        || p.name_len + 1 != 19
    {
        return 4;
    }
    if s.parsed.packets[3].offset != 72 || s.parsed.packets[3].header != version::ACK {
        return 6;
    }
    0
}
/// Consume the sealed version checkpoint and expose only its already-captured
/// bytes. This path has no transport writer, lock, or doorbell capability.
pub(crate) fn snapshot_plan(checkpoint: version::VersionCheckpoint) -> PlanSnapshotReport {
    let (_, _, snapshot) = checkpoint.into_parts();
    PlanSnapshotReport {
        available: snapshot.available,
        captured: snapshot.captured as u32,
        words: snapshot.words,
        bytes: snapshot.bytes,
        plan_detail: plan_detail(&snapshot),
    }
}
/// Preserve the first rejected OPEN's bounded wire fields for diagnostics.
/// `param2` is the declared name length including the NUL terminator.
pub(crate) fn plan_shape_diagnostic(s: &Snapshot) -> Option<PlanShapeDiagnostic> {
    if plan_detail(s) != 4 {
        return None;
    }
    let first_len = if s.parsed.packets[0].header.param2 == 7 {
        7u32
    } else {
        10
    };
    let second_len = if first_len == 7 { 10u32 } else { 7 };
    let second_offset = if first_len == 7 { 16usize } else { 24 };
    for (i, (name_bytes, offset)) in [
        (first_len, 0usize),
        (second_len, second_offset),
        (19u32, 40),
    ]
    .into_iter()
    .enumerate()
    {
        let p = &s.parsed.packets[i];
        if p.offset != offset
            || p.header
                != (Header {
                    command: 2,
                    param1: (i + 1) as u16,
                    param2: name_bytes,
                })
            || p.name_len + 1 != name_bytes as usize
        {
            let mut name_prefix = [0; 12];
            let name_len = p.name_len.min(name_prefix.len());
            name_prefix[..name_len].copy_from_slice(&p.name[..name_len]);
            let mut packets = [EMPTY_PLAN_PACKET; 4];
            for (slot, packet) in packets.iter_mut().zip(s.parsed.packets.iter()) {
                let name_len = packet.name_len.min(slot.name_prefix.len());
                slot.offset = packet.offset as u32;
                slot.command = packet.header.command as u32;
                slot.id = packet.header.param1 as u32;
                slot.bytes = packet.header.param2;
                slot.name_prefix[..name_len].copy_from_slice(&packet.name[..name_len]);
                slot.name_len = packet.name_len as u32;
            }
            return Some(PlanShapeDiagnostic {
                packet: (i + 1) as u32,
                expected_offset: offset as u32,
                offset: p.offset as u32,
                expected_id: (i + 1) as u32,
                id: p.header.param1 as u32,
                expected_name_bytes: name_bytes,
                name_bytes: p.header.param2,
                command: p.header.command as u32,
                name_prefix,
                name_len: p.name_len as u32,
                packets,
            });
        }
    }
    None
}
pub(crate) fn plan(s: &Snapshot) -> Result<(), Error> {
    if plan_detail(s) == 0 {
        Ok(())
    } else {
        Err(Error::Payload)
    }
}
fn tx(m: &impl Read, staged: bool) -> Result<(), Error> {
    for i in 0..TX.size {
        let want = if i < 16 {
            version::PACKETS[i]
        } else if staged && i < 32 {
            REQUEST[i - 16]
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
    last_rx_head: u32,
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
        || s.words[3] < last_rx_head
        || s.words[4..] != [0; 4]
    {
        return Err(Error::Outbound);
    }
    Ok(s)
}
fn prefix(m: &impl Read, s: &receive::Report, old: &Snapshot) -> Result<(), Error> {
    if s.available < 80 {
        return Err(Error::Bounds);
    }
    let rx = s.inventory.items[2].ok_or(Error::ItemSize)?;
    for i in 0..80 {
        if m.read8(rx.offset + 8 + i) != old.bytes[i] {
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
    io: &mut impl Io,
    r: &mut Report,
) -> Result<receive::Report, Error> {
    r.guard_stage = 1;
    r.plan_detail = plan_detail(old);
    r.plan_shape = plan_shape_diagnostic(old);
    plan(old)?;
    r.guard_stage = 2;
    let before = state(m, base, ready, 16, 16, 88)?;
    if before.words != old.words || before.available != 80 {
        return Err(Error::Unstable);
    }
    prefix(m, &before, old)?;
    tx(m, false)?;
    sync();
    prefix(m, &before, old)?;
    tx(m, false)?;
    let checked = state(m, base, ready, 16, 16, 88)?;
    if checked.words != before.words {
        return Err(Error::Unstable);
    }
    r.guard_stage = 3;
    for (i, v) in REQUEST.into_iter().enumerate() {
        io.byte(i, v);
        r.byte_writes += 1
    }
    r.guard_stage = 4;
    sync();
    tx(m, true)?;
    sync();
    tx(m, true)?;
    let staged = state(m, base, ready, 16, 16, 88)?;
    prefix(m, &staged, old)?;
    if staged.words != before.words {
        return Err(Error::Unstable);
    }
    let final_check = state(m, base, ready, 16, 16, 88)?;
    if final_check.words != before.words {
        return Err(Error::Unstable);
    }
    r.staged = true;
    r.guard_stage = 5;
    io.head();
    r.head_store = true;
    r.guard_stage = 6;
    sync();
    let published = state(m, base, ready, 32, 16, 88)?;
    prefix(m, &published, old)?;
    tx(m, true)?;
    state(m, base, ready, 32, published.words[0], published.words[3])
}
/// Only classify bounded control records in copied bytes. Intent metadata is
/// skipped by a checked size; no advertised address/ID is dereferenced or used.
pub(crate) fn reply(bytes: &[u8]) -> Result<(bool, Option<Header>), Error> {
    if bytes.len() > MAX_NEW_RX || bytes.len() % 8 != 0 {
        return Err(Error::Bounds);
    }
    let mut at = 0;
    let mut ack = false;
    let mut first = None;
    while at < bytes.len() {
        let b = &bytes[at..at + 8];
        let h = Header {
            command: u16::from_le_bytes(b[..2].try_into().unwrap()),
            param1: u16::from_le_bytes(b[2..4].try_into().unwrap()),
            param2: u32::from_le_bytes(b[4..8].try_into().unwrap()),
        };
        if first.is_none() {
            first = Some(h)
        }
        let size = match h.command {
            4 => {
                if h != OPEN_ACK {
                    return Err(Error::Payload);
                }
                ack = true;
                8
            }
            5 => {
                if h.param1 != 2 || h.param2 == 0 || h.param2 > 15 {
                    return Err(Error::Payload);
                }
                8 + 8 * h.param2 as usize
            }
            13 => 8,
            15 => {
                if h.param1 != 2 {
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
    }
    Ok((ack, first))
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
    let before = state(m, base, ready, 32, last_tail, last_head)?;
    prefix(m, &before, old)?;
    tx(m, true)?;
    let n = (before.available as usize - 80).min(MAX_NEW_RX);
    let rx = before.inventory.items[2].ok_or(Error::ItemSize)?;
    let mut one = [0; MAX_NEW_RX];
    let mut two = [0; MAX_NEW_RX];
    for (i, v) in one[..n].iter_mut().enumerate() {
        *v = m.read8(rx.offset + 88 + i)
    }
    sync();
    for (i, v) in two[..n].iter_mut().enumerate() {
        *v = m.read8(rx.offset + 88 + i)
    }
    if one != two {
        return Err(Error::Unstable);
    }
    sync();
    prefix(m, &before, old)?;
    tx(m, true)?;
    let after = state(m, base, ready, 32, before.words[0], before.words[3])?;
    if before.words != after.words || before.available != after.available {
        return Err(Error::Unstable);
    }
    r.words = after.words;
    r.available = after.available;
    r.new_bytes = n as u32;
    r.tx_consumed = after.words[0] == 32;
    // Preserve the first copied header even when classification rejects it,
    // so a stopped native diagnostic still identifies the unexpected reply.
    r.header = if n >= 8 {
        Some(Header {
            command: u16::from_le_bytes(one[..2].try_into().unwrap()),
            param1: u16::from_le_bytes(one[2..4].try_into().unwrap()),
            param2: u32::from_le_bytes(one[4..8].try_into().unwrap()),
        })
    } else {
        None
    };
    let (ack, _) = reply(&one[..n])?;
    r.open_ack = ack;
    Ok(())
}
/// Continuation owned by the actual request execution, never by a copied report.
pub struct OpenCheckpoint {
    ready: ReadyObservation,
    baseline: receive::Report,
    snapshot: Snapshot,
    report: Report,
}
impl OpenCheckpoint {
    pub(crate) fn into_parts(self) -> (ReadyObservation, receive::Report, Snapshot, Report) {
        (self.ready, self.baseline, self.snapshot, self.report)
    }
}
pub(crate) fn run(
    w: &mut Window,
    checkpoint: VersionCheckpoint,
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Report {
    run_inner(w, checkpoint, io, clock, progress).0
}
pub(crate) fn checkpoint_run(
    w: &mut Window,
    checkpoint: VersionCheckpoint,
    io: &mut impl Io,
    clock: &mut impl Clock,
    progress: impl FnMut(&Report),
) -> Result<OpenCheckpoint, Report> {
    let (r, next) = run_inner(w, checkpoint, io, clock, progress);
    next.ok_or(r)
}
fn run_inner(
    w: &mut Window,
    checkpoint: VersionCheckpoint,
    io: &mut impl Io,
    clock: &mut impl Clock,
    mut progress: impl FnMut(&Report),
) -> (Report, Option<OpenCheckpoint>) {
    let (ready, base, old) = checkpoint.into_parts();
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
    let sent = match w.locked(|m| submit(m, &base, &ready, &old, io, &mut r)) {
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
    for i in 0..512 {
        r.samples = i + 1;
        r.guard_stage = 8;
        match w.locked(|m| sample(m, &base, &ready, &old, r.words[0], r.words[3], &mut r)) {
            Ok(()) => {}
            Err(e) => {
                r.error = Some(e);
                break;
            }
        }
        if r.tx_consumed && r.open_ack {
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
        Some(OpenCheckpoint {
            ready,
            baseline: base,
            snapshot: old,
            report: r,
        })
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
        assert!(index < 16);
        assert_eq!(value, REQUEST[index]);
        unsafe {
            self.ap
                .add(TX.offset - PART + 16 + index)
                .write_volatile(value)
        }
    }
    fn head(&mut self) {
        unsafe {
            self.ap
                .add(DESCRIPTOR.offset - PART + 4)
                .cast::<u32>()
                .write_volatile(32u32.to_le())
        }
    }
    fn notify(&mut self) {
        unsafe { self.doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
    }
}
pub struct OpenOnce {
    checkpoint: Option<VersionCheckpoint>,
    io: NativeIo,
}
impl OpenOnce {
    /// # Safety
    /// Exclusive uncached AP page0x8611d000 and APCS page0x17c00000 from
    /// the exact experiment manifest, without aliases, IRQ handler or AP client.
    pub unsafe fn new(checkpoint: VersionCheckpoint, ap: *mut u8, doorbell: *mut u32) -> Self {
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
    pub fn into_channel_checkpoint(
        mut self,
        w: &mut Window,
        clock: &mut impl Clock,
        progress: impl FnMut(&Report),
    ) -> Result<OpenCheckpoint, Report> {
        let Some(checkpoint) = self.checkpoint.take() else {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return Err(r);
        };
        checkpoint_run(w, checkpoint, &mut self.io, clock, progress)
    }
}
