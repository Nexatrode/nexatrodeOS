//! Bounded diagnostic text for the Pixel's uppercase-only framebuffer font.
use core::fmt::{self, Write};

pub(super) struct ErrorText {
    bytes: [u8; 72],
    len: usize,
    truncated: bool,
}

impl ErrorText {
    pub(super) fn from_debug(value: &impl fmt::Debug) -> Self {
        let mut text = Self { bytes: [b' '; 72], len: 0, truncated: false };
        let _ = write!(&mut text, "{value:?}");
        text
    }
    pub(super) fn line(&self, row: usize) -> &str {
        let start = row.saturating_mul(24);
        if start >= self.len { return ""; }
        let end = (start + 24).min(self.len);
        // All bytes inserted by Write are restricted to ASCII below.
        core::str::from_utf8(&self.bytes[start..end]).unwrap_or("")
    }
    pub(super) fn truncated(&self) -> bool { self.truncated }
}
impl Write for ErrorText {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        for byte in value.bytes() {
            if self.len == self.bytes.len() { self.truncated = true; break; }
            self.bytes[self.len] = match byte {
                b'a'..=b'z' => byte.to_ascii_uppercase(),
                b'A'..=b'Z' | b'0'..=b'9' => byte,
                _ => b' ',
            };
            self.len += 1;
        }
        Ok(())
    }
}

/// Throttle routine repaints while the first TFTP OACK is in flight. Errors
/// and the completion transition remain immediately visible. This is display
/// policy only: it never holds, drops, or delays any transport work itself.
#[cfg(any(test, feature = "sunfish-tftp-quiet-oack-experiment"))]
#[derive(Default)]
pub(super) struct OackPaintGate { since: Option<u64> }
#[cfg(any(test, feature = "sunfish-tftp-quiet-oack-experiment"))]
impl OackPaintGate {
    pub(super) fn allow(&mut self, waiting: bool, error: bool, now: u64, hz: u64) -> bool {
        if error || !waiting {
            self.since = None;
            return true;
        }
        let Some(interval) = hz.checked_mul(5).filter(|ticks| *ticks != 0 && *ticks < (1u64 << 63)) else {
            self.since = None;
            return true;
        };
        let Some(previous) = self.since else { self.since = Some(now); return false; };
        let elapsed = now.wrapping_sub(previous);
        if elapsed >= (1u64 << 63) || elapsed >= interval {
            self.since = Some(now);
            true
        } else {
            false
        }
    }
}

/// First host-observed WRQ through the first service pass. Times are sampled
/// after calls return; absent or reversed timestamps remain unavailable.
#[cfg(any(test, feature = "sunfish-tftp-oack-phase-observer"))]
#[derive(Default)]
pub(super) struct OackPhases { origin: Option<u64>, ends: [Option<u64>; 4] }
#[cfg(any(test, feature = "sunfish-tftp-oack-phase-observer"))]
impl OackPhases {
    pub(super) fn begin(&mut self, request: Option<u64>, dispatch_end: u64) {
        if self.origin.is_none() {
            if let Some(origin) = request { self.origin = Some(origin); self.ends[0] = Some(dispatch_end); }
        }
    }
    pub(super) fn mark(&mut self, slot: usize, now: u64) {
        if self.origin.is_some() {
            if let Some(end) = self.ends.get_mut(slot) {
                if end.is_none() { *end = Some(now); }
            }
        }
    }
    pub(super) fn age_ms(&self, slot: usize, hz: u64) -> u32 {
        match (self.origin, self.ends.get(slot).copied().flatten()) {
            (Some(start), Some(end)) => end.checked_sub(start).map_or(u32::MAX, |v| ticks_to_ms(v, hz)),
            _ => u32::MAX,
        }
    }
}

/// Delta of existing IO profiler counters over the owner turn in which the
/// first OACK becomes published. This is a whole turn, not just that write.
#[cfg(any(test, feature = "sunfish-tftp-publish-io-observer"))]
#[derive(Default)]
pub(super) struct OackPublishIo<const N: usize = 4> { totals: Option<[Option<u64>; N]> }
#[cfg(any(test, feature = "sunfish-tftp-publish-io-observer"))]
impl<const N: usize> OackPublishIo<N> {
    pub(super) fn observe(&mut self, before_flags: Option<u32>, after_flags: Option<u32>, before: [u64; N], after: [u64; N]) {
        if self.totals.is_some() || !before_flags.is_some_and(|f| f & 1 != 0 && f & 4 == 0)
            || !after_flags.is_some_and(|f| f & 4 != 0) { return; }
        self.totals = Some(core::array::from_fn(|i| {
            if after[i] == u64::MAX { None } else { after[i].checked_sub(before[i]) }
        }));
    }
    pub(super) fn ms(&self, slot: usize, hz: u64) -> u32 {
        self.totals.and_then(|v| v.get(slot).copied().flatten())
            .map_or(u32::MAX, |v| ticks_to_ms(v, hz))
    }
}

/// First receive dispatch failure, captured before quarantine can hide it.
pub(super) struct ReceiveFailure {
    pub badge: u64,
    pub scope: u32,
    pub native_error: u32,
    pub text: ErrorText,
}
impl ReceiveFailure {
    pub(super) fn capture(badge: u64, scope: u32, native_error: u32, error: &impl fmt::Debug) -> Self {
        Self { badge, scope, native_error, text: ErrorText::from_debug(error) }
    }
}

/// Monotonic software-wakeup evidence and most recent bounded service checkpoint.
#[derive(Default)]
pub(super) struct ProgressTrace {
    pub phase: u32,
    pub wake_sent: u32,
    pub wake_received: u32,
}
impl ProgressTrace {
    pub fn checkpoint(&mut self, phase: u32) { self.phase = phase; }
    pub fn sent(&mut self) { self.wake_sent = self.wake_sent.saturating_add(1); }
    pub fn received(&mut self) { self.wake_received = self.wake_received.saturating_add(1); }
}

/// Bounded timing counters; slots are owner, storage, IPA, painting and service registry.
#[derive(Default)]
pub(super) struct TimingTrace {
    pub total: [u64; 5],
    pub max: [u64; 5],
}
impl TimingTrace {
    pub fn record(&mut self, slot: usize, ticks: u64) {
        self.total[slot] = self.total[slot].saturating_add(ticks);
        self.max[slot] = self.max[slot].max(ticks);
    }
}

#[cfg(feature = "sunfish-wlan-elf-registry-gap")]
struct RegistryEntry {
    at: u64,
    totals: [u64; 5],
    #[cfg(feature = "sunfish-wlan-elf-gap-io")]
    router_io: [u64; 12],
    outside: u64,
    notifications: u32,
}

#[cfg(feature = "sunfish-wlan-elf-registry-gap")]
pub(super) struct RegistryGapSample {
    pub elapsed: u64,
    pub work: [u64; 5],
    pub outside: u64,
    pub unaccounted: u64,
    pub notifications: u32,
    pub next_badge: u64,
    #[cfg(feature = "sunfish-wlan-elf-gap-io")]
    pub router_io: [u64; 12],
}

/// Time between registry entries while the first ELF request awaits selection.
/// Work buckets reuse the runtime's nonoverlapping timing totals.
#[cfg(feature = "sunfish-wlan-elf-registry-gap")]
#[derive(Default)]
pub(super) struct RegistryGap {
    previous: Option<RegistryEntry>,
    #[cfg(feature = "sunfish-wlan-elf-gap-io")]
    router_io: [u64; 12],
    last_callback_exit: Option<u64>,
    outside: u64,
    notifications: u32,
    badge: u64,
    pub longest: Option<RegistryGapSample>,
}

#[cfg(feature = "sunfish-wlan-elf-registry-gap")]
impl RegistryGap {
    #[cfg(feature = "sunfish-wlan-elf-gap-io")]
    pub fn record_router_io(&mut self, before: [u64; 12], after: [u64; 12]) {
        for i in 0..self.router_io.len() {
            let delta = if before[i] == u64::MAX || after[i] == u64::MAX {
                u64::MAX
            } else {
                after[i].checked_sub(before[i]).unwrap_or(u64::MAX)
            };
            self.router_io[i] = self.router_io[i].saturating_add(delta);
        }
    }

    pub fn begin_callback(&mut self, now: u64, badge: u64) {
        if let Some(exit) = self.last_callback_exit.take() {
            self.outside = self.outside.saturating_add(now.wrapping_sub(exit));
        }
        self.notifications = self.notifications.saturating_add(1);
        self.badge = badge;
    }

    pub fn end_callback(&mut self, now: u64) {
        self.last_callback_exit = Some(now);
    }

    pub fn enter_registry(&mut self, at: u64, totals: [u64; 5]) {
        let entry = RegistryEntry {
            at,
            totals,
            #[cfg(feature = "sunfish-wlan-elf-gap-io")]
            router_io: self.router_io,
            outside: self.outside,
            notifications: self.notifications,
        };
        let Some(previous) = self.previous.replace(entry) else {
            return;
        };
        let elapsed = at.wrapping_sub(previous.at);
        if self.longest.as_ref().is_some_and(|sample| sample.elapsed >= elapsed) {
            return;
        }
        let work = core::array::from_fn(|i| totals[i].saturating_sub(previous.totals[i]));
        let outside = self.outside.saturating_sub(previous.outside);
        let accounted = work.iter().copied().fold(outside, u64::saturating_add);
        self.longest = Some(RegistryGapSample {
            elapsed,
            work,
            outside,
            unaccounted: elapsed.saturating_sub(accounted),
            notifications: self.notifications.saturating_sub(previous.notifications),
            next_badge: self.badge,
            #[cfg(feature = "sunfish-wlan-elf-gap-io")]
            router_io: core::array::from_fn(|i| {
                if self.router_io[i] == u64::MAX || previous.router_io[i] == u64::MAX {
                    u64::MAX
                } else {
                    self.router_io[i].saturating_sub(previous.router_io[i])
                }
            }),
        });
    }
}

/// Missing, saturated or reversed counters remain unavailable, never zero.
#[cfg(feature = "sunfish-wlan-elf-phase-io")]
pub(super) fn elf_io_delta(before: Option<[u64; 12]>, after: Option<[u64; 12]>) -> [u64; 12] {
    let (Some(before), Some(after)) = (before, after) else { return [u64::MAX; 12]; };
    core::array::from_fn(|i| {
        if before[i] == u64::MAX || after[i] == u64::MAX { u64::MAX }
        else { after[i].checked_sub(before[i]).unwrap_or(u64::MAX) }
    })
}

pub(super) fn ticks_to_ms(ticks: u64, frequency: u64) -> u32 {
    if frequency == 0 { return u32::MAX; }
    ((u128::from(ticks) * 1000 / u128::from(frequency)).min(u128::from(u32::MAX))) as u32
}

/// Classify only the first successfully observed RRQ path. This grants no
/// access and 0 means unavailable, not proof that no request arrived.
#[cfg(any(test, feature = "sunfish-tftp-first-rrq-observer"))]
pub(super) fn first_rrq_kind(path: Option<&str>) -> u32 {
    match path {
        None => 0,
        Some("/readwrite/ota_firewall/ruleset") => 1,
        Some("/readwrite/modem_efs") => 2,
        Some(value) if value.ends_with("/wlanmdsp.mbn") => 3,
        Some(_) => 4,
    }
}

/// CPU-counter deadline for the single experimental outbound intent request.
/// The caller must deliver a wake while waiting; this type is not a timer.
#[derive(Default)]
pub(super) struct IntentDeadline { started: Option<u64> }
impl IntentDeadline {
    pub(super) fn update(&mut self, state: u32, now: u64, hz: u64) -> (bool, bool) {
        if state != 1 && state != 2 { self.started = None; return (false, false); }
        if hz == 0 || hz > u64::MAX / 10 { return (false, true); }
        let started = *self.started.get_or_insert(now);
        if now.wrapping_sub(started) >= hz * 10 { (false, true) } else { (true, false) }
    }
}
pub(super) const fn intent_deadline_only(state: u32, badge: u64, wake: u64, continuation: bool, pending: u64) -> bool {
    (state == 1 || state == 2) && badge == wake && !continuation && pending == 0
}

/// Repaint on new IPA state even when the only wake is a software continuation.
/// An unchanged continuation remains cheap and does not cause display churn.
pub(super) const fn should_repaint(error: u32, badge: u64, continuation: u64, ipa_changed: bool) -> bool {
    error != 0 || badge & !continuation != 0 || ipa_changed
}

/// Encode ten path bytes per screen row so case and punctuation are preserved.
#[cfg(any(test, feature = "sunfish-native-tftp-observer", feature = "sunfish-tftp-first-rrq-observer"))]
pub(super) fn path_hex_chunk(path: &[u8], part: usize) -> ([u8; 20], usize) {
    let mut out = [b' '; 20];
    let start = part.saturating_mul(10).min(path.len());
    let chunk = &path[start..path.len().min(start.saturating_add(10))];
    for (i, &byte) in chunk.iter().enumerate() {
        out[i * 2] = b"0123456789ABCDEF"[usize::from(byte >> 4)];
        out[i * 2 + 1] = b"0123456789ABCDEF"[usize::from(byte & 15)];
    }
    (out, chunk.len() * 2)
}

/// IPA report flags: pending, queued, held response, protocol completion seen,
/// then modem-ready, QMI-uC-ready, indication requested/sent, initial boot,
/// and failed. Bit 10 records whether a protocol core existed at snapshot.
pub(super) fn ipa_flags(pending: bool, queued: bool, held: bool, complete: bool, state: Option<[bool; 6]>) -> u32 {
    let mut flags = u32::from(pending) | u32::from(queued) << 1
        | u32::from(held) << 2 | u32::from(complete) << 3;
    if let Some(bits) = state {
        flags |= 1 << 10;
        for (index, value) in bits.into_iter().enumerate() {
            flags |= u32::from(value) << (index + 4);
        }
    }
    flags
}


/// Disjoint physical-counter intervals, split at the actual retained IPA
/// timestamps: queue, publication, retirement, commit, registration prepared.
/// Buckets: router, storage, IPA, paint, registry, TFTP (including its nested
/// router turn), outside callbacks, other. No scheduling decisions use this.
#[cfg(any(test, feature = "sunfish-ipa-schedule-trace"))]
#[derive(Default)]
pub(super) struct IpaScheduleTiming {
    previous: Option<u64>,
    milestones: [Option<u64>; 5],
    pub work: [[u64; 8]; 4],
    pub invalid: bool,
}
#[cfg(any(test, feature = "sunfish-ipa-schedule-trace"))]
impl IpaScheduleTiming {
    pub fn observe(&mut self, now: u64, bucket: usize, milestones: [Option<u64>; 5]) {
        let previous = self.previous.replace(now);
        if self.invalid { return; }
        if bucket >= 8 || previous.is_some_and(|old| now < old) {
            self.invalid = true;
            return;
        }
        let mut last = None;
        let mut missing = false;
        for (old, new) in self.milestones.iter().zip(milestones) {
            if old.is_some() && *old != new { self.invalid = true; return; }
            if let Some(at) = new {
                if missing || at > now || last.is_some_and(|last| at < last)
                    || (old.is_none() && previous.is_some_and(|old| at < old)) {
                    self.invalid = true;
                    return;
                }
                last = Some(at);
            } else { missing = true; }
        }
        self.milestones = milestones;
        let Some(previous) = previous else { return; };
        for phase in 0..4 {
            let Some(start) = milestones[phase] else { break; };
            let start = previous.max(start);
            let end = now.min(milestones[phase + 1].unwrap_or(now));
            if end > start {
                self.work[phase][bucket] = self.work[phase][bucket].saturating_add(end - start);
            }
        }
    }

    pub fn phase_ms(&self, phase: usize, hz: u64) -> u32 {
        if self.invalid || phase >= 4 { return u32::MAX; }
        match (self.milestones[phase], self.milestones[phase + 1]) {
            (Some(start), Some(end)) => ticks_to_ms(end - start, hz),
            _ => u32::MAX,
        }
    }

    /// High half: queue -> publication; low half: publication -> registration
    /// preparation. FFFF is unavailable, FFFE is a saturated millisecond value.
    pub fn packed_ms(&self, bucket: usize, hz: u64) -> u32 {
        if self.invalid || bucket >= 8 || hz == 0 { return u32::MAX; }
        let part = |available, ticks| if available {
            ticks_to_ms(ticks, hz).min(0xfffe)
        } else { 0xffff };
        let tx = part(self.milestones[0].is_some(), self.work[0][bucket]);
        let post = part(self.milestones[1].is_some(), (1..4).fold(0u64, |sum, phase|
            sum.saturating_add(self.work[phase][bucket])));
        (tx << 16) | post
    }
}

#[cfg(any(
    test,
    feature = "sunfish-ipa-schedule-trace",
    feature = "sunfish-ipa-registration-tx-trace",
))]
pub(super) fn packed_counts(high: u32, low: u32) -> u32 {
    (high.min(0xffff) << 16) | low.min(0xffff)
}
