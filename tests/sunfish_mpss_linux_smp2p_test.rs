//! Host RAM only; actual retained predecessor/allocation, injected remote peer.
use super::*;
use sunfish_mpss_linux_smem::{self as mem, Extension, Payload};
use sunfish_mpss_linux_smp2p::{
    self as smp, Controller, Field, Lifecycle, Observation, Phase, Poll, Power, Signals, SmpIo,
};
#[test]
fn complete_smp2p_snapshot_uses_bounded_words_and_preserves_every_byte() {
    use std::cell::Cell;
    struct Words { bytes: Vec<u8>, reads: Cell<usize> }
    impl qcom_modem_status::Read for Words {
        fn read8(&self, at: usize) -> u8 { self.bytes[at] }
        fn read_le32(&self, at: usize) -> u32 {
            self.reads.set(self.reads.get() + 1);
            u32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap())
        }
    }
    let bytes: Vec<u8> = (0..smp::BYTES + 8).map(|i| (i % 251) as u8).collect();
    let input = Words { bytes: bytes.clone(), reads: Cell::new(0) };
    let mut snapshot = [0; smp::BYTES];
    smp::read_item(&input, 4, &mut snapshot);
    assert_eq!(&snapshot[..], &bytes[4..4 + smp::BYTES]);
    assert_eq!(input.reads.get(), smp::BYTES / 4);
}
#[derive(Default)]
struct Effects {
    writes: Vec<(usize, u32, Field)>,
    kicks: Vec<Vec<u8>>,
    claims: usize,
    fail_store: Option<(usize, bool)>,
    corrupt: Option<usize>,
    panic_store: Option<usize>,
    fail_kick: Option<usize>,
    deny: Option<usize>,
    observations: usize,
    change: Option<(usize, Observation)>,
    dropped: usize,
}
struct State {
    effects: Effects,
    observation: Observation,
}
type StateRef = Rc<RefCell<State>>;
struct Writer {
    bus: Bus,
    state: StateRef,
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.state.borrow_mut().effects.dropped += 1;
    }
}
impl Payload for Writer {
    fn guard(&mut self, g: u64) -> Result<(), mem::Error> {
        assert_eq!(g, 91);
        Ok(())
    }
    fn store(&mut self, r: &mem::Store) -> Result<(), mem::Error> {
        let mut b = self.bus.0.borrow_mut();
        assert_eq!(b.lock, 1);
        word(&mut b.bytes, r.offset(), r.value());
        Ok(())
    }
}
impl SmpIo for Writer {
    fn claim_smp2p(&mut self) -> Result<(), smp::Error> {
        let mut s = self.state.borrow_mut();
        s.effects.claims += 1;
        assert_eq!(s.effects.claims, 1);
        Ok(())
    }
    fn smp_store(&mut self, r: &smp::Write) -> Result<(), smp::Error> {
        let mut s = self.state.borrow_mut();
        let e = &mut s.effects;
        let n = e.writes.len();
        assert_ne!(r.at(), 36, "master-kernel control value must stay zero");
        assert!(r.at() + 4 <= 340);
        assert_eq!(self.bus.0.borrow().lock, 1);
        e.writes.push((r.at(), r.value(), r.field()));
        if e.fail_store == Some((n, false)) {
            return Err(smp::Error::Store);
        }
        word(
            &mut self.bus.0.borrow_mut().bytes,
            smp::AP.offset + r.at(),
            r.value() ^ u32::from(e.corrupt == Some(n)),
        );
        assert!(e.panic_store != Some(n), "uncertain SMP2P store");
        if e.fail_store == Some((n, true)) {
            return Err(smp::Error::Store);
        }
        Ok(())
    }
}
struct Signal {
    bus: Bus,
    state: StateRef,
}
impl Drop for Signal {
    fn drop(&mut self) {
        self.state.borrow_mut().effects.dropped += 1;
    }
}
impl Signals for Signal {
    fn observe(&mut self, g: u64) -> Result<Observation, smp::Error> {
        assert_eq!(g, 91);
        let mut s = self.state.borrow_mut();
        s.effects.observations += 1;
        let n = s.effects.observations;
        if s.effects.deny == Some(n) {
            return Err(smp::Error::Guard);
        }
        if let Some((at, o)) = s.effects.change {
            if at == n {
                s.observation = o;
            }
        }
        Ok(s.observation)
    }
    fn notify(&mut self) -> Result<(), smp::Error> {
        assert_eq!(self.bus.0.borrow().lock, 1);
        let mut s = self.state.borrow_mut();
        let a = smp::AP.offset;
        s.effects
            .kicks
            .push(self.bus.0.borrow().bytes[a..a + 340].to_vec());
        if s.effects.fail_kick == Some(s.effects.kicks.len()) {
            return Err(smp::Error::Notify);
        }
        Ok(())
    }
}
type Test = Controller<Bus, Writer, Signal>;
fn setup() -> (Test, Bus, StateRef) {
    let b = Bus::new();
    let mut p = Owner::new(b.clone());
    p.allocate_once().unwrap();
    let s = Rc::new(RefCell::new(State {
        effects: Effects::default(),
        observation: Observation {
            generation: 91,
            lifecycle: Lifecycle::PreAuth,
            power: Power::Active,
        },
    }));
    let mut e = Extension::new(
        p,
        Writer {
            bus: b.clone(),
            state: s.clone(),
        },
        91,
    );
    for _ in 0..64 {
        if e.report().phase == mem::Phase::Prepared {
            break;
        }
        e.step().unwrap();
    }
    let r = e.take_prepared().unwrap();
    (
        Controller::new(
            e,
            r,
            Signal {
                bus: b.clone(),
                state: s.clone(),
            },
        ),
        b,
        s,
    )
}
fn peer(version: u8, features: u32, flags: u32, kernel: u32, ipa: u32) -> [u8; 340] {
    let mut p = [0; 340];
    word(&mut p, 0, 0x504d5324);
    word(&mut p, 4, u32::from(version) | (features << 8));
    word(&mut p, 8, 1);
    word(&mut p, 12, 0x20010);
    word(&mut p, 16, flags);
    p[20..32].copy_from_slice(b"slave-kernel");
    word(&mut p, 36, kernel);
    p[40..43].copy_from_slice(b"ipa");
    word(&mut p, 56, ipa);
    p
}
fn publish(b: &Bus, p: &[u8; 340]) {
    let h = PART + mem::FINAL_FREE;
    let mut m = b.0.borrow_mut();
    word(&mut m.bytes, h, (435 << 16) | 0xa5a5);
    word(&mut m.bytes, h + 4, 344);
    word(&mut m.bytes, h + 8, 4);
    m.bytes[h + 16..h + 356].copy_from_slice(p);
    word(&mut m.bytes, PART + 12, (mem::FINAL_FREE + 360) as u32);
}
fn launched(
    version: u8,
    features: u32,
    flags: u32,
    kernel: u32,
    ipa: u32,
) -> (Test, Bus, StateRef) {
    let (mut c, b, s) = setup();
    c.initialize().unwrap();
    s.borrow_mut().observation.lifecycle = Lifecycle::AuthSubmitted;
    publish(&b, &peer(version, features, flags, kernel, ipa));
    (c, b, s)
}
fn stopped(c: &mut Test, s: &StateRef) {
    let n = s.borrow().effects.writes.len();
    let k = s.borrow().effects.kicks.len();
    let o = s.borrow().effects.observations;
    assert!(c.poll().is_err());
    assert!(c.initialize().is_err());
    assert!(c.take_ready_observation().is_err());
    assert_eq!(
        (n, k, o),
        (
            s.borrow().effects.writes.len(),
            s.borrow().effects.kicks.len(),
            s.borrow().effects.observations
        )
    );
}
#[test]
fn init_matches_original_linux_bytes_and_both_publication_kicks() {
    let (mut c, b, s) = setup();
    let before = b.0.borrow().bytes.clone();
    c.initialize().unwrap();
    assert_eq!(c.report().initialized, 16);
    assert_eq!(c.report().submitted, 16);
    assert_eq!(c.report().completed, 16);
    assert_eq!(s.borrow().effects.kicks.len(), 2);
    assert_eq!(b.0.borrow().lock, 0);
    assert_eq!(c.poll(), Ok(Poll::Waiting));
    let a = smp::AP.offset;
    let after = &b.0.borrow().bytes;
    assert_eq!(&after[..a], &before[..a]);
    assert_eq!(&after[a + 340..], &before[a + 340..]);
    assert_eq!(&after[a + 36..a + 40], &[0; 4]);
    if let Some(dir) = std::env::var_os("SUNFISH_MPSS_SMP2P_ORACLE") {
        for (n, snapshot) in s.borrow().effects.kicks.iter().enumerate() {
            assert_eq!(
                snapshot,
                &std::fs::read(std::path::Path::new(&dir).join(format!("init-{n}.bin"))).unwrap()
            );
        }
    }
    drop(c);
    assert_eq!(s.borrow().effects.dropped, 0);
}
#[test]
fn version_features_power_and_initial_ssr_match_original_linux() {
    for version in [1, 2] {
        for active in [false, true] {
            for features in [0, 1, 0xffffff] {
                let (mut c, b, s) = launched(version, features, 1, 6, 3);
                s.borrow_mut().observation.power = if active {
                    Power::Active
                } else {
                    Power::Inactive
                };
                if version == 1 {
                    assert_eq!(c.poll(), Ok(Poll::VersionChanged));
                }
                assert_eq!(c.poll(), Ok(Poll::Observed));
                let r = c.report();
                assert!(r.ready_seen && r.handover_seen && r.setup_seen && r.power_notified);
                assert_eq!(r.features, features & 1);
                assert_eq!(r.restart_acks, u32::from(features & 1 != 0));
                assert_eq!(
                    r.power_reported,
                    Some(if active {
                        Power::Active
                    } else {
                        Power::Inactive
                    })
                );
                assert_eq!(c.take_ready_observation().unwrap().generation(), 91);
                let n = s.borrow().effects.writes.len();
                assert_eq!(c.poll(), Ok(Poll::Observed));
                assert_eq!(s.borrow().effects.writes.len(), n);
                if features == 1 {
                    if let Some(dir) = std::env::var_os("SUNFISH_MPSS_SMP2P_ORACLE") {
                        let expected = std::fs::read(
                            std::path::Path::new(&dir)
                                .join(format!("v{version}-on{}.bin", u8::from(active))),
                        )
                        .unwrap();
                        let a = smp::AP.offset;
                        assert_eq!(&b.0.borrow().bytes[a..a + 340], &expected);
                        let events = std::fs::read(
                            std::path::Path::new(&dir)
                                .join(format!("v{version}-on{}-kicks.bin", u8::from(active))),
                        )
                        .unwrap();
                        let actual: Vec<u8> =
                            s.borrow().effects.kicks.iter().flatten().copied().collect();
                        assert_eq!(actual, events);
                    }
                }
            }
        }
    }
}
#[test]
fn initialization_every_store_error_readback_and_notification_failure_is_consumed() {
    for n in 0..16 {
        for after in [false, true] {
            let (mut c, b, s) = setup();
            s.borrow_mut().effects.fail_store = Some((n, after));
            assert_eq!(c.initialize(), Err(smp::Error::Store));
            assert_eq!(s.borrow().effects.writes.len(), n + 1);
            assert_eq!(b.0.borrow().lock, 0);
            stopped(&mut c, &s);
        }
    }
    for n in 0..16 {
        let (mut c, _, s) = setup();
        s.borrow_mut().effects.corrupt = Some(n);
        assert_eq!(c.initialize(), Err(smp::Error::Readback));
        stopped(&mut c, &s);
    }
    for k in [1, 2] {
        let (mut c, _, s) = setup();
        s.borrow_mut().effects.fail_kick = Some(k);
        assert_eq!(c.initialize(), Err(smp::Error::Notify));
        assert_eq!(s.borrow().effects.kicks.len(), k);
        stopped(&mut c, &s);
    }
}
#[test]
fn runtime_every_effect_failure_is_terminal_without_republication() {
    for n in 16..20 {
        for after in [false, true] {
            let (mut c, b, s) = launched(2, 1, 1, 2, 1);
            s.borrow_mut().effects.fail_store = Some((n, after));
            assert_eq!(c.poll(), Err(smp::Error::Store));
            assert_eq!(b.0.borrow().lock, 0);
            stopped(&mut c, &s);
        }
    }
    for k in 3..=5 {
        let (mut c, _, s) = launched(2, 1, 1, 2, 1);
        s.borrow_mut().effects.fail_kick = Some(k);
        assert_eq!(c.poll(), Err(smp::Error::Notify));
        stopped(&mut c, &s);
    }
}
#[test]
fn peer_fatal_stop_reset_and_target_name_mutations_never_publish_power() {
    for value in [1, 3, 8, 128, 130] {
        let (mut c, _, s) = launched(2, 1, 0, value, 1);
        assert!(c.poll().is_err());
        assert_eq!(s.borrow().effects.writes.len(), 16);
        stopped(&mut c, &s);
    }
    for mode in 0..6 {
        let (mut c, b, s) = launched(2, 1, 0, 2, 0);
        c.poll().unwrap();
        let n = s.borrow().effects.writes.len();
        let mut p = peer(2, 1, 0, 2, 0);
        match mode {
            0 => word(&mut p, 36, 0),
            1 => word(&mut p, 16, 1),
            2 => p[4] = 0,
            3 => p[4] = 1,
            4 => half(&mut p, 14, 1),
            _ => p[20] = b'X',
        }
        publish(&b, &p);
        assert!(c.poll().is_err());
        assert_eq!(s.borrow().effects.writes.len(), n);
        stopped(&mut c, &s);
    }
    let (mut c, b, s) = launched(1, 1, 0, 0, 0);
    c.poll().unwrap();
    let mut p = peer(1, 1, 0, 0, 0);
    p[20] = b'X';
    publish(&b, &p);
    assert_eq!(c.poll(), Err(smp::Error::PeerNames));
    stopped(&mut c, &s);
}
#[test]
fn peer_decoder_rejects_corrupt_headers_duplicates_and_does_not_invent_ready() {
    for mode in 0..8 {
        let mut p = peer(2, 1, 0, 0, 0);
        match mode {
            0 => p[0] = 0,
            1 => p[4] = 3,
            2 => word(&mut p, 8, 0),
            3 => half(&mut p, 12, 17),
            4 => half(&mut p, 14, 17),
            5 => p[20] = 0,
            6 => p[20] = 255,
            _ => {
                p[40..56].fill(0);
                p[40..52].copy_from_slice(b"slave-kernel");
            }
        }
        assert!(smp::decode(&p).is_err(), "mode {mode}");
    }
    let (mut c, _, s) = launched(2, 1, 0, 0, 0);
    assert_eq!(c.poll(), Ok(Poll::Observed));
    assert!(!c.report().ready_seen);
    assert!(c.take_ready_observation().is_err());
    stopped(&mut c, &s);
}
#[test]
fn torn_peer_samples_are_retryable_without_effects_and_appended_entries_are_accepted() {
    let (mut c, b, s) = launched(2, 1, 0, 0, 0);
    let a = PART + mem::FINAL_FREE + 16;
    {
        let mut m = b.0.borrow_mut();
        m.watch = a;
        m.change_at_read = Some((2, a + 36, 2));
    }
    assert_eq!(c.poll(), Ok(Poll::Unstable));
    assert_eq!(s.borrow().effects.writes.len(), 16);
    assert_eq!(c.poll(), Ok(Poll::Observed));
    assert!(c.report().ready_seen);
    let mut p = peer(2, 1, 0, 2, 0);
    half(&mut p, 14, 3);
    p[60..65].copy_from_slice(b"other");
    publish(&b, &p);
    assert_eq!(c.poll(), Ok(Poll::Observed));
    assert_eq!(c.report().peer.unwrap().valid, 3);
}
#[test]
fn changed_directory_between_batched_snapshots_is_rejected_without_stores() {
    let (mut c, b, s) = launched(2, 1, 0, 2, 0);
    let writes = s.borrow().effects.writes.len();
    {
        let mut m = b.0.borrow_mut();
        m.watch = 192;
        m.watched_reads = 0;
        // Change the peer item's valid padding between the two metadata scans.
        m.change_at_read = Some((2, PART + mem::FINAL_FREE + 8, 5));
    }
    assert_eq!(c.poll(), Err(smp::Error::Directory));
    assert_eq!(s.borrow().effects.writes.len(), writes);
    assert_eq!(b.0.borrow().lock, 0);
}
#[test]
fn lifecycle_generation_power_and_outbound_corruption_are_checked_before_effects() {
    for mode in 0..5 {
        let (mut c, b, s) = launched(2, 1, 0, 2, 1);
        match mode {
            0 => s.borrow_mut().observation.generation = 92,
            1 => {
                let n = s.borrow().effects.observations;
                s.borrow_mut().effects.deny = Some(n + 1);
            }
            2 => word(&mut b.0.borrow_mut().bytes, smp::AP.offset + 36, 1),
            3 => word(&mut b.0.borrow_mut().bytes, TOC + 32 + 48, PART as u32),
            _ => word(
                &mut b.0.borrow_mut().bytes,
                PART + 12,
                (mem::FINAL_FREE - 8) as u32,
            ),
        }
        if mode == 3 {
            word(&mut b.0.borrow_mut().bytes, TOC + 36 + 48, 4096);
        }
        assert!(c.poll().is_err());
        assert_eq!(s.borrow().effects.writes.len(), 16);
        stopped(&mut c, &s);
    }
    let (mut c, _, s) = launched(2, 1, 0, 2, 0);
    s.borrow_mut().observation.lifecycle = Lifecycle::AuthReturned;
    c.poll().unwrap();
    s.borrow_mut().observation.lifecycle = Lifecycle::AuthSubmitted;
    assert_eq!(c.poll(), Err(smp::Error::PeerRestart));
    stopped(&mut c, &s);
    let (mut c, _, s) = launched(2, 1, 0, 2, 1);
    let n = s.borrow().effects.observations;
    s.borrow_mut().effects.change = Some((
        n + 6,
        Observation {
            generation: 91,
            lifecycle: Lifecycle::AuthSubmitted,
            power: Power::Inactive,
        },
    ));
    assert!(c.poll().is_err());
    assert!(!c.report().power_notified);
    stopped(&mut c, &s);
}
#[test]
fn cold_absence_and_lock_ownership_are_real_requirements() {
    let (mut c, b, s) = setup();
    publish(&b, &peer(2, 1, 0, 2, 1));
    assert!(c.initialize().is_err());
    assert!(s.borrow().effects.writes.is_empty());
    stopped(&mut c, &s);
    let (mut c, b, s) = setup();
    b.0.borrow_mut().busy = true;
    assert_eq!(c.initialize(), Err(smp::Error::LockBusy));
    assert!(s.borrow().effects.writes.is_empty());
    stopped(&mut c, &s);
    let (mut c, b, s) = setup();
    let n = b.0.borrow().lock_reads;
    b.0.borrow_mut().lose_at = Some(n + 3);
    assert_eq!(c.initialize(), Err(smp::Error::LockLost));
    assert_eq!(b.0.borrow().lock, 2);
    stopped(&mut c, &s);
}
#[test]
fn unwind_retains_all_owners_and_cannot_replay_an_uncertain_store() {
    let (mut c, _, s) = setup();
    s.borrow_mut().effects.panic_store = Some(8);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.initialize())).is_err());
    assert_eq!(c.report().phase, Phase::Failed);
    assert_eq!(c.initialize(), Err(smp::Error::Consumed));
    stopped(&mut c, &s);
    drop(c);
    assert_eq!(s.borrow().effects.dropped, 0);
}

#[repr(C, align(4096))]
#[derive(Clone)]
pub(crate) struct Page(pub(crate) [u8; 4096]);
pub(crate) type NativeExtension = Extension<
    sunfish_ipa_smem_native::Window<'static>,
    sunfish_mpss_linux_smem_native::Native<'static, fn(u64) -> Result<(), mem::Error>>,
>;
pub(crate) fn prepared_native() -> (NativeExtension, Vec<Page>, Box<Page>) {
    let mut pages = vec![Page([0; 4096]); 512];
    for (p, b) in pages.iter_mut().zip(fixture().chunks_exact(4096)) {
        p.0.copy_from_slice(b)
    }
    let b = pages.as_mut_ptr().cast::<u8>();
    let mut lock = Box::new(Page([0; 4096]));
    // Host RAM only; returned allocations stay live through the last use of
    // the private test owner. No production lifetime or capability is forged.
    let (w, p) = unsafe {
        (
            sunfish_ipa_smem_native::Window::new(
                b,
                0x86000000,
                b.add(PART).cast(),
                0x8611d000,
                lock.0.as_mut_ptr().cast(),
                0x1f43000,
            )
            .unwrap(),
            sunfish_mpss_linux_smem_native::Native::new(
                b,
                0x86000000,
                b.add(PART).cast(),
                0x8611d000,
                b.add(0x11f000).cast(),
                0x8611f000,
                cold_guard as fn(u64) -> Result<(), mem::Error>,
            )
            .map_err(|(_, e)| e)
            .unwrap(),
        )
    };
    let mut parent = Owner::new(w);
    parent.allocate_once().unwrap();
    let mut e = Extension::new(parent, p, 91);
    for _ in 0..64 {
        if e.report().phase == mem::Phase::Prepared {
            break;
        }
        e.step().unwrap();
    }
    let r = e.take_prepared().unwrap();
    e.claim_smp2p(r).unwrap();
    let (i, p) = e.smp2p_parts().unwrap();
    i.lock_write(1);
    p.claim_smp2p().unwrap();
    (e, pages, lock)
}
struct NativeGate(StateRef);
impl sunfish_mpss_linux_smp2p_native::Gate for NativeGate {
    fn observe(&mut self, g: u64) -> Result<Observation, smp::Error> {
        let mut s = self.0.borrow_mut();
        s.effects.observations += 1;
        if s.effects.deny == Some(s.effects.observations) {
            return Err(smp::Error::Guard);
        }
        assert_eq!(g, 91);
        Ok(s.observation)
    }
}
fn cold_guard(g: u64) -> Result<(), mem::Error> {
    assert_eq!(g, 91);
    Ok(())
}
#[test]
fn actual_volatile_adapters_preserve_neighbours_and_match_wire_bytes() {
    for version in [1, 2] {
        for on in [false, true] {
            let mut pages = vec![Page([0; 4096]); 512];
            for (p, b) in pages.iter_mut().zip(fixture().chunks_exact(4096)) {
                p.0.copy_from_slice(b)
            }
            let b = pages.as_mut_ptr().cast::<u8>();
            let mut lock = Box::new(Page([0; 4096]));
            let mut apcs = 0xcafeu32;
            let s = Rc::new(RefCell::new(State {
                effects: Effects::default(),
                observation: Observation {
                    generation: 91,
                    lifecycle: Lifecycle::PreAuth,
                    power: if on { Power::Active } else { Power::Inactive },
                },
            }));
            // Aligned host allocations simulate the declared physical views only.
            let (window, payload, signal) = unsafe {
                (
                    sunfish_ipa_smem_native::Window::new(
                        b,
                        0x86000000,
                        b.add(PART).cast(),
                        0x8611d000,
                        lock.0.as_mut_ptr().cast(),
                        0x1f43000,
                    )
                    .unwrap(),
                    sunfish_mpss_linux_smem_native::Native::new(
                        b,
                        0x86000000,
                        b.add(PART).cast(),
                        0x8611d000,
                        b.add(0x11f000).cast(),
                        0x8611f000,
                        cold_guard as fn(u64) -> Result<(), mem::Error>,
                    )
                    .map_err(|(_, e)| e)
                    .unwrap(),
                    sunfish_mpss_linux_smp2p_native::Native::new(
                        &mut apcs,
                        0x17c0000c,
                        NativeGate(s.clone()),
                    )
                    .map_err(|(_, e)| e)
                    .unwrap(),
                )
            };
            let mut parent = Owner::new(window);
            parent.allocate_once().unwrap();
            let mut e = Extension::new(parent, payload, 91);
            for _ in 0..64 {
                if e.report().phase == mem::Phase::Prepared {
                    break;
                }
                e.step().unwrap();
            }
            let receipt = e.take_prepared().unwrap();
            let before: Vec<u8> = pages.iter().flat_map(|p| p.0).collect();
            let mut c = Controller::new(e, receipt, signal);
            c.initialize().unwrap();
            assert_eq!(apcs, 1 << 14);
            s.borrow_mut().observation.lifecycle = Lifecycle::AuthReturned;
            let p = peer(version, 1, 1, 6, 3);
            let h = PART + mem::FINAL_FREE;
            // Simulate remote allocation/publication, not a host runtime API.
            let mut after = before.clone();
            word(&mut after, h, (435 << 16) | 0xa5a5);
            word(&mut after, h + 4, 344);
            word(&mut after, h + 8, 4);
            after[h + 16..h + 356].copy_from_slice(&p);
            word(&mut after, PART + 12, (mem::FINAL_FREE + 360) as u32);
            for at in [PART + 12, h, h + 4, h + 8] {
                pages[at / 4096].0[at % 4096..at % 4096 + 4].copy_from_slice(&after[at..at + 4]);
            }
            for (n, v) in p.iter().enumerate() {
                let at = h + 16 + n;
                pages[at / 4096].0[at % 4096] = *v;
            }
            if version == 1 {
                assert_eq!(c.poll(), Ok(Poll::VersionChanged));
            }
            assert_eq!(c.poll(), Ok(Poll::Observed));
            assert_eq!(c.take_ready_observation().unwrap().generation(), 91);
            assert_eq!(apcs, 1 << 14);
            assert_eq!(u32::from_le_bytes(lock.0[..4].try_into().unwrap()), 0);
            let actual: Vec<u8> = pages.iter().flat_map(|p| p.0).collect();
            for i in 0..SIZE {
                if !(smp::AP.offset..smp::AP.offset + 340).contains(&i) {
                    assert_eq!(actual[i], after[i], "foreign native byte {i:x}");
                }
            }
            if let Some(dir) = std::env::var_os("SUNFISH_MPSS_SMP2P_ORACLE") {
                let expected = std::fs::read(
                    std::path::Path::new(&dir).join(format!("v{version}-on{}.bin", u8::from(on))),
                )
                .unwrap();
                assert_eq!(&actual[smp::AP.offset..smp::AP.offset + 340], expected);
            }
        }
    }
}
#[test]
fn native_notification_requires_fresh_live_observation_and_never_retries_failure() {
    use sunfish_mpss_linux_smp2p_native::Native;
    let (_, _, s) = setup();
    let mut apcs = 0xfeedu32;
    assert!(unsafe { Native::new(&mut apcs, 0x17c00008, NativeGate(s.clone())) }.is_err());
    let mut n = unsafe { Native::new(&mut apcs, 0x17c0000c, NativeGate(s.clone())) }
        .map_err(|(_, e)| e)
        .unwrap();
    assert_eq!(n.notify(), Err(smp::Error::Guard));
    assert_eq!(apcs, 0xfeed);
    assert!(n.observe(91).is_err());
    let mut n = unsafe { Native::new(&mut apcs, 0x17c0000c, NativeGate(s.clone())) }
        .map_err(|(_, e)| e)
        .unwrap();
    n.observe(91).unwrap();
    s.borrow_mut().observation.power = Power::Inactive;
    assert_eq!(n.notify(), Err(smp::Error::Guard));
    assert_eq!(apcs, 0xfeed);
    assert!(n.notify().is_err());
}
