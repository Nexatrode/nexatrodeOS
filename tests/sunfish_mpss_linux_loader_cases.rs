use super::*;
use std::{cell::RefCell, rc::Rc};

#[cfg(feature = "sunfish-linux-mpss-auth")]
pub(crate) mod boot_handoff {
    use super::*;
    include!("sunfish_mpss_linux_boot_cases.rs");
}

const SMALL: usize = 0x20000;
const META_PA: u64 = 0x81000000;
fn digest(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b);
    h.finish()
}
fn put(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn fixture() -> Vec<u8> {
    let mut b = vec![0xee; 0x800];
    b[..52 + 31 * 32].fill(0);
    b[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    b[18..20].copy_from_slice(&164u16.to_le_bytes());
    put(&mut b, 20, 1);
    put(&mut b, 28, 52);
    b[40..42].copy_from_slice(&52u16.to_le_bytes());
    b[42..44].copy_from_slice(&32u16.to_le_bytes());
    b[44..46].copy_from_slice(&31u16.to_le_bytes());
    for i in 0..31 {
        let fields = match i {
            0 => [0, 0, 0, 0, 1044, 0, 7 << 24, 0],
            1 => [0, 0x500, 0, 0, 64, 64, 2 << 24, 0],
            _ => {
                let n = i - 2;
                let offset = n * 4096 + if n % 2 == 1 { 3 } else { 0 };
                [
                    1,
                    if n == 28 {
                        u32::MAX
                    } else {
                        0x600 + n as u32 * 8
                    },
                    0,
                    BASE as u32 + offset as u32,
                    if n == 28 { 0 } else { 5 },
                    if n == 28 { (SMALL - offset) as u32 } else { 11 },
                    1 << 27,
                    0,
                ]
            }
        };
        for (j, v) in fields.into_iter().enumerate() {
            put(&mut b, 52 + i * 32 + j * 4, v);
        }
    }
    b
}
fn image(b: &[u8]) -> Image<'_> {
    Image::identity(b, b.len(), digest(b), SMALL).ok().unwrap()
}
#[derive(Default)]
struct State {
    metadata: Vec<u8>,
    target: Vec<u8>,
    submitted: bool,
    authenticated: bool,
    active: bool,
    calls: Vec<(Kind, [u64; 8])>,
    acquisitions: usize,
    releases: usize,
    guards: usize,
    deny_guard: Option<usize>,
    deny_acquire: bool,
    deny_release: bool,
    bad: Option<(usize, u8)>,
    panic_call: Option<usize>,
    panic_write: bool,
    erase_metadata: bool,
    bad_meta: bool,
    bad_target: bool,
    dropped: usize,
    memory_accesses: usize,
}
#[derive(Clone)]
struct Cpu {
    state: Rc<RefCell<State>>,
    pa: u64,
}
impl Drop for Cpu {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped += 1;
    }
}
impl Memory for Cpu {
    fn metadata_physical(&self) -> u64 {
        self.pa
    }
    fn metadata_write(&mut self, at: usize, v: u8) -> Result<(), Error> {
        let mut s = self.state.borrow_mut();
        assert!(!s.submitted);
        s.memory_accesses += 1;
        s.metadata[at] = v;
        Ok(())
    }
    fn metadata_read(&mut self, at: usize) -> Result<u8, Error> {
        let mut s = self.state.borrow_mut();
        assert!(!s.submitted);
        s.memory_accesses += 1;
        Ok(s.metadata[at] ^ u8::from(s.bad_meta && at == 22))
    }
    fn target_write8(&mut self, at: usize, v: u8) -> Result<(), Error> {
        let mut s = self.state.borrow_mut();
        assert!(s.submitted && !s.authenticated);
        s.memory_accesses += 1;
        s.target[at] = v;
        assert!(!s.panic_write, "uncertain write");
        Ok(())
    }
    fn target_read8(&mut self, at: usize) -> Result<u8, Error> {
        let mut s = self.state.borrow_mut();
        assert!(s.submitted && !s.authenticated);
        s.memory_accesses += 1;
        Ok(s.target[at] ^ u8::from(s.bad_target && at == 0))
    }
    fn target_write32(&mut self, at: usize, v: u32) -> Result<(), Error> {
        assert_eq!(at % 4, 0);
        let mut s = self.state.borrow_mut();
        assert!(s.submitted && !s.authenticated);
        s.memory_accesses += 1;
        s.target[at..at + 4].copy_from_slice(&v.to_le_bytes());
        assert!(!s.panic_write, "uncertain write");
        Ok(())
    }
    fn target_read32(&mut self, at: usize) -> Result<u32, Error> {
        assert_eq!(at % 4, 0);
        let mut s = self.state.borrow_mut();
        assert!(s.submitted && !s.authenticated);
        s.memory_accesses += 1;
        Ok(u32::from_le_bytes(s.target[at..at + 4].try_into().unwrap())
            ^ u32::from(s.bad_target && at == 0))
    }
    fn sync(&mut self) -> Result<(), Error> {
        assert!(!self.state.borrow().authenticated);
        Ok(())
    }
}
struct Platform(Rc<RefCell<State>>);
impl Backend for Platform {
    type Lease = usize;
    fn guard(&mut self, g: Guard) -> Result<(), Error> {
        let mut s = self.0.borrow_mut();
        s.guards += 1;
        assert_eq!(g.generation, 77);
        assert!(!s.submitted || g.metadata_submitted);
        assert!(!s.authenticated || g.auth_submitted);
        if s.deny_guard == Some(s.guards) {
            Err(Error::Guard)
        } else {
            Ok(())
        }
    }
    fn acquire(&mut self) -> Result<usize, Error> {
        let mut s = self.0.borrow_mut();
        assert!(!s.active);
        s.acquisitions += 1;
        if s.deny_acquire {
            return Err(Error::Acquire);
        }
        s.active = true;
        Ok(s.acquisitions)
    }
    fn release(&mut self, lease: usize) -> Result<(), Error> {
        let mut s = self.0.borrow_mut();
        assert!(s.active);
        assert_eq!(lease, s.acquisitions);
        s.active = false;
        s.releases += 1;
        if s.deny_release {
            Err(Error::Release)
        } else {
            Ok(())
        }
    }
    fn invoke(&mut self, r: &Request) -> Result<[u64; 8], Error> {
        let mut s = self.0.borrow_mut();
        assert_eq!(s.active, r.kind() != Kind::Availability);
        if r.kind() == Kind::InitImage {
            s.submitted = true;
            if s.erase_metadata {
                s.metadata.fill(0);
            }
        }
        if r.kind() == Kind::AuthAndReset {
            s.authenticated = true;
        }
        let i = s.calls.len();
        s.calls.push((r.kind(), *r.words()));
        assert!(s.panic_call != Some(i), "uncertain secure call");
        let mut reply = [0; 8];
        reply[1] = u64::from(r.kind() == Kind::Availability);
        if let Some((at, how)) = s.bad {
            if at == i {
                match how {
                    0 => return Err(Error::Transport),
                    1 => reply[0] = u64::MAX,
                    2 => reply[1] ^= 1,
                    _ => unreachable!(),
                }
            }
        }
        Ok(reply)
    }
}
fn setup(b: &[u8]) -> (Loader<'_, Cpu>, Platform, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State {
        metadata: vec![0xa5; METADATA_BYTES],
        target: vec![0xa5; SMALL],
        ..State::default()
    }));
    let cpu = Cpu {
        state: state.clone(),
        pa: META_PA,
    };
    let l = Loader::new(77, image(b), cpu).ok().unwrap();
    (l, Platform(state.clone()), state)
}
fn advance(l: &mut Loader<'_, Cpu>, p: &mut Platform, until: Phase) {
    for _ in 0..200000 {
        if l.report().phase == until {
            return;
        }
        l.step(p).unwrap();
    }
    panic!("unbounded loader");
}
fn stopped(l: &mut Loader<'_, Cpu>, p: &mut Platform, s: &Rc<RefCell<State>>) {
    let count = s.borrow().calls.len();
    let accesses = s.borrow().memory_accesses;
    assert!(l.take_auth_returned().is_err());
    assert_eq!(l.step(p), Err(Error::Consumed));
    assert_eq!(s.borrow().calls.len(), count);
    assert_eq!(s.borrow().memory_accesses, accesses);
}
#[test]
fn mpss_identity_and_geometry_checked_before_effects() {
    let b = fixture();
    assert!(matches!(Image::private_reference(&b), Err(Error::Identity)));
    assert!(matches!(
        Image::identity(&b, b.len() + 1, digest(&b), SMALL),
        Err(Error::Identity)
    ));
    assert!(matches!(
        Image::identity(&b, b.len(), [0; 32], SMALL),
        Err(Error::Identity)
    ));
    assert!(matches!(
        Image::identity(&b, b.len(), digest(&b), SMALL + 4096),
        Err(Error::Geometry)
    ));
    let mut b = fixture();
    put(&mut b, 52 + 2 * 32 + 16, 12);
    assert!(matches!(
        Image::identity(&b, b.len(), digest(&b), SMALL),
        Err(Error::Image)
    ));
}
#[test]
fn mpss_source_order_metadata_bss_byte_edges_gaps_and_envelopes() {
    let b = fixture();
    let (mut l, mut p, s) = setup(&b);
    let mut old = l.report();
    while l.report().phase != Phase::AuthReturned {
        l.step(&mut p).unwrap();
        let r = l.report();
        assert!(r.metadata_writes - old.metadata_writes <= CHUNK);
        assert!(r.metadata_verified - old.metadata_verified <= CHUNK);
        assert!(r.target_writes - old.target_writes <= TARGET_CHUNK);
        assert!(r.target_verified - old.target_verified <= TARGET_CHUNK);
        old = r;
    }
    let s = s.borrow();
    let r = l.report();
    assert_eq!((r.attempted_mask, r.returned_mask), (63, 63));
    assert_eq!(
        (r.metadata_length, r.metadata_writes, r.metadata_verified),
        (1108, 12288, 12288)
    );
    assert_eq!(&s.metadata[..1044], &b[..1044]);
    assert_eq!(&s.metadata[1044..1108], &b[0x500..0x540]);
    assert!(s.metadata[1108..].iter().all(|x| *x == 0));
    let mut expected = vec![0xa5; SMALL];
    for n in 0..29 {
        let start = n * 4096 + if n % 2 == 1 { 3 } else { 0 };
        let size = if n == 28 { SMALL - start } else { 11 };
        expected[start..start + size].fill(0);
        if n != 28 {
            expected[start..start + 5].copy_from_slice(&b[0x600 + n * 8..0x605 + n * 8]);
        }
    }
    assert_eq!(s.target, expected);
    assert_eq!((s.acquisitions, s.releases), (3, 3));
    assert_eq!(
        s.calls.iter().map(|x| x.1).collect::<Vec<_>>(),
        vec![
            [0x42000601, 1, 0x02000201, 0, 0, 0, 0, 0],
            [0x42000601, 1, 0x02000202, 0, 0, 0, 0, 0],
            [0x42000601, 1, 0x02000205, 0, 0, 0, 0, 0],
            [0x42000201, 0x82, 4, META_PA, 0, 0, 0, 0],
            [0x42000202, 3, 4, BASE, SMALL as u64, 0, 0, 0],
            [0x42000205, 1, 4, 0, 0, 0, 0, 0]
        ]
    );
}
#[test]
fn mpss_metadata_erasure_is_not_a_required_post_init_fingerprint_and_owners_retained() {
    let b = fixture();
    let (mut l, mut p, s) = setup(&b);
    s.borrow_mut().erase_metadata = true;
    advance(&mut l, &mut p, Phase::AuthReturned);
    assert_eq!(l.take_auth_returned().unwrap().generation(), 77);
    assert!(s.borrow().metadata.iter().all(|x| *x == 0));
    drop(l);
    assert_eq!(s.borrow().dropped, 0);
}
#[test]
fn mpss_corrupt_metadata_prevents_all_secure_calls() {
    let b = fixture();
    let (mut l, mut p, s) = setup(&b);
    s.borrow_mut().bad_meta = true;
    advance(&mut l, &mut p, Phase::VerifyMetadata);
    assert_eq!(l.step(&mut p), Err(Error::Readback));
    assert!(s.borrow().calls.is_empty());
    stopped(&mut l, &mut p, &s);
}
#[test]
fn mpss_corrupt_target_prevents_auth() {
    let b = fixture();
    let (mut l, mut p, s) = setup(&b);
    advance(&mut l, &mut p, Phase::VerifyTarget);
    s.borrow_mut().bad_target = true;
    assert_eq!(l.step(&mut p), Err(Error::Readback));
    assert_eq!(s.borrow().calls.len(), 5);
    assert!(!l.report().auth_submitted);
    stopped(&mut l, &mut p, &s);
}
#[test]
fn mpss_every_transport_and_both_monitor_status_failures_stop_without_replay() {
    for index in 0..6 {
        for how in 0..3 {
            let b = fixture();
            let (mut l, mut p, s) = setup(&b);
            s.borrow_mut().bad = Some((index, how));
            let e = loop {
                if let Err(e) = l.step(&mut p) {
                    break e;
                }
            };
            assert_eq!(
                e,
                match how {
                    0 => Error::Transport,
                    1 => Error::Monitor,
                    _ =>
                        if index < 3 {
                            Error::Unavailable
                        } else {
                            Error::Service
                        },
                }
            );
            assert_eq!(l.report().attempted_mask, (1 << (index + 1)) - 1);
            assert_eq!(
                l.report().returned_mask,
                if how == 0 {
                    (1 << index) - 1
                } else {
                    (1 << (index + 1)) - 1
                }
            );
            assert_eq!(s.borrow().calls.len(), index + 1);
            assert!(!s.borrow().active);
            if index >= 3 {
                assert_eq!(s.borrow().acquisitions, s.borrow().releases);
            }
            stopped(&mut l, &mut p, &s);
        }
    }
}
#[test]
fn mpss_failed_acquire_and_release_are_terminal_with_return_diagnostics() {
    for release in [false, true] {
        let b = fixture();
        let (mut l, mut p, s) = setup(&b);
        advance(&mut l, &mut p, Phase::InitImage);
        if release {
            s.borrow_mut().deny_release = true;
        } else {
            s.borrow_mut().deny_acquire = true;
        }
        assert_eq!(
            l.step(&mut p),
            Err(if release {
                Error::Release
            } else {
                Error::Acquire
            })
        );
        assert_eq!(s.borrow().calls.len(), if release { 4 } else { 3 });
        assert_eq!(
            l.report().last_reply,
            if release { Some([0; 8]) } else { None }
        );
        assert_eq!(l.report().returned_mask, if release { 15 } else { 7 });
        stopped(&mut l, &mut p, &s);
    }
}
#[test]
fn mpss_lifecycle_loss_before_or_after_acquire_blocks_call_and_releases_lease() {
    for between in [false, true] {
        let b = fixture();
        let (mut l, mut p, s) = setup(&b);
        advance(&mut l, &mut p, Phase::InitImage);
        let next = s.borrow().guards + if between { 2 } else { 1 };
        s.borrow_mut().deny_guard = Some(next);
        assert_eq!(l.step(&mut p), Err(Error::Guard));
        assert_eq!(s.borrow().calls.len(), 3);
        assert_eq!(s.borrow().releases, usize::from(between));
        stopped(&mut l, &mut p, &s);
    }
}
#[test]
fn mpss_uncertain_call_or_write_consumes_attempt_and_retains_memory() {
    for call in [false, true] {
        let b = fixture();
        let (mut l, mut p, s) = setup(&b);
        advance(
            &mut l,
            &mut p,
            if call {
                Phase::AuthAndReset
            } else {
                Phase::Staging
            },
        );
        if call {
            s.borrow_mut().panic_call = Some(5);
        } else {
            s.borrow_mut().panic_write = true;
        }
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| l.step(&mut p))).is_err());
        assert_eq!(l.report().error, Some(Error::Consumed));
        // Check the next action before any other API can latch the failure.
        // Merely inspecting report() must not be needed to prevent replay.
        assert_eq!(l.step(&mut p), Err(Error::Consumed));
        stopped(&mut l, &mut p, &s);
        drop(l);
        assert_eq!(s.borrow().dropped, 0);
    }
}
#[test]
fn mpss_lifecycle_loss_after_success_does_not_issue_an_auth_receipt() {
    for phase in [Phase::InitImage, Phase::MemSetup, Phase::AuthAndReset] {
        let b = fixture();
        let (mut l, mut p, s) = setup(&b);
        advance(&mut l, &mut p, phase);
        let next = s.borrow().guards + 3;
        s.borrow_mut().deny_guard = Some(next);
        assert_eq!(l.step(&mut p), Err(Error::Guard));
        assert_eq!(s.borrow().acquisitions, s.borrow().releases);
        assert_eq!(l.report().last_reply, Some([0; 8]));
        if phase == Phase::AuthAndReset {
            assert!(s.borrow().authenticated);
        }
        stopped(&mut l, &mut p, &s);
    }
}
#[test]
fn mpss_invalid_mapping_returns_owners_without_any_access() {
    for (generation, pa) in [
        (0, META_PA),
        (77, 0x81000001),
        (77, 0x70000000),
        (77, BASE),
        (77, (1 << 40) - 4096),
    ] {
        let b = fixture();
        let s = Rc::new(RefCell::new(State::default()));
        let (_, cpu, e) = Loader::new(
            generation,
            image(&b),
            Cpu {
                state: s.clone(),
                pa,
            },
        )
        .err()
        .unwrap();
        assert_eq!(e, Error::Geometry);
        assert_eq!(s.borrow().memory_accesses, 0);
        drop(cpu);
        assert_eq!(s.borrow().dropped, 1);
    }
}
#[test]
fn mpss_auth_receipt_is_not_ready_and_cannot_be_minted_early_or_twice() {
    let b = fixture();
    let (mut l, mut p, s) = setup(&b);
    assert!(l.take_auth_returned().is_err());
    stopped(&mut l, &mut p, &s);
    let (mut l, mut p, s) = setup(&b);
    advance(&mut l, &mut p, Phase::AuthReturned);
    assert_eq!(l.take_auth_returned().unwrap().generation(), 77);
    stopped(&mut l, &mut p, &s);
}
#[test]
#[ignore = "requires exact private firmware and compiled pinned Linux C oracle"]
fn mpss_real_private_image_matches_original_linux_c_every_target_byte() {
    let firmware = std::fs::read(std::env::var_os("SUNFISH_MPSS_FIRMWARE").unwrap()).unwrap();
    let expected_meta =
        std::fs::read(std::env::var_os("SUNFISH_MPSS_C_METADATA").unwrap()).unwrap();
    let expected_target =
        std::fs::read(std::env::var_os("SUNFISH_MPSS_C_TARGET").unwrap()).unwrap();
    let s = Rc::new(RefCell::new(State {
        metadata: vec![0xa5; METADATA_BYTES],
        target: vec![0xa5; BYTES],
        ..State::default()
    }));
    let mut l = Loader::new(
        77,
        Image::private_reference(&firmware).ok().unwrap(),
        Cpu {
            state: s.clone(),
            pa: META_PA,
        },
    )
    .ok()
    .unwrap();
    let expected_steps: usize = l.image.packed.segments()
        .map(|s| (s.descriptor.memory_size as usize).div_ceil(TARGET_CHUNK)).sum::<usize>() * 2;
    let mut p = Platform(s.clone());
    let mut target_steps = 0;
    while l.report().phase != Phase::AuthReturned {
        target_steps += usize::from(matches!(l.report().phase, Phase::Staging | Phase::VerifyTarget));
        l.step(&mut p).unwrap();
    }
    assert_eq!(target_steps, expected_steps);
    println!("target_chunk={TARGET_CHUNK} target_steps={target_steps} bytes_written={} bytes_verified={}",
             l.report().target_writes, l.report().target_verified);
    let s = s.borrow();
    assert_eq!(&s.metadata[..expected_meta.len()], expected_meta);
    assert_eq!(s.target, expected_target);
    assert_eq!(expected_meta.len(), 9100);
    assert_eq!(
        (l.report().target_writes, l.report().target_verified),
        (157744117, 157744117)
    );
    assert_eq!(
        s.calls[4].1,
        [0x42000202, 3, 4, BASE, BYTES as u64, 0, 0, 0]
    );
    let trace = std::fs::read_to_string(std::env::var_os("SUNFISH_MPSS_C_TRACE").unwrap()).unwrap();
    let original: Vec<Vec<u64>> = trace
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|word| u64::from_str_radix(word, 16).unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        s.calls[3..]
            .iter()
            .map(|c| c.1.to_vec())
            .collect::<Vec<_>>(),
        original
    );
    assert_eq!(l.report().attempted_mask, 63);
}

// Larger than either target budget, with an unaligned final segment, a file
// boundary inside a word, multiple BSS chunks and untouched inter-segment gaps.
fn target_budget_fixture() -> (Vec<u8>, usize, usize, usize) {
    let mut b = fixture();
    let extent = SMALL * 3;
    let start = 28 * 4096 + 3;
    let file_bytes = 65539;
    b.resize(0x800 + file_bytes, 0);
    for (i, byte) in b[0x800..].iter_mut().enumerate() {
        *byte = (i as u8).wrapping_mul(37).wrapping_add(11);
    }
    for (field, value) in [(4, 0x800), (12, BASE as usize + start),
                           (16, file_bytes), (20, extent - start)] {
        put(&mut b, 52 + 30 * 32 + field, value as u32);
    }
    (b, extent, start, file_bytes)
}
fn target_budget_setup(b: &[u8], extent: usize) -> (Loader<'_, Cpu>, Platform, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State {
        metadata: vec![0xa5; METADATA_BYTES], target: vec![0xa5; extent],
        ..State::default()
    }));
    let image = Image::identity(b, b.len(), digest(b), extent).ok().unwrap();
    let cpu = Cpu { state: state.clone(), pa: META_PA };
    (Loader::new(77, image, cpu).ok().unwrap(), Platform(state.clone()), state)
}
#[test]
fn mpss_target_budget_preserves_byte_edges_bss_gaps_and_each_guard_boundary() {
    let (b, extent, start, file_bytes) = target_budget_fixture();
    let (mut l, mut p, s) = target_budget_setup(&b, extent);
    advance(&mut l, &mut p, Phase::Staging);
    let mut steps = [0; 2];
    while matches!(l.report().phase, Phase::Staging | Phase::VerifyTarget) {
        let old = l.report();
        let guards = s.borrow().guards;
        let writing = old.phase == Phase::Staging;
        let remaining = l.image.packed.segments().nth(l.segment).unwrap()
            .descriptor.memory_size as usize - l.at;
        l.step(&mut p).unwrap();
        let now = l.report();
        assert_eq!(s.borrow().guards - guards, 2);
        assert_eq!(if writing { now.target_writes - old.target_writes }
                   else { now.target_verified - old.target_verified },
                   remaining.min(TARGET_CHUNK));
        steps[usize::from(!writing)] += 1;
        assert!(!s.borrow().authenticated);
    }
    assert_eq!(steps, [28 + (extent - start).div_ceil(TARGET_CHUNK); 2]);
    let mut expected = vec![0xa5; extent];
    for n in 0..28 {
        let at = n * 4096 + if n % 2 == 1 { 3 } else { 0 };
        expected[at..at+11].fill(0);
        expected[at..at+5].copy_from_slice(&b[0x600+n*8..0x605+n*8]);
    }
    expected[start..].fill(0);
    expected[start..start+file_bytes].copy_from_slice(&b[0x800..]);
    assert_eq!(s.borrow().target, expected);
    assert_eq!(l.report().target_writes, l.report().target_verified);
    l.step(&mut p).unwrap();
    assert_eq!(l.report().phase, Phase::AuthReturned);
}
#[test]
fn mpss_target_budget_guard_loss_stops_before_or_after_one_bounded_chunk() {
    for verify in [false, true] {
        for after in [false, true] {
            let (b, extent, _, _) = target_budget_fixture();
            let (mut l, mut p, s) = target_budget_setup(&b, extent);
            advance(&mut l, &mut p, if verify { Phase::VerifyTarget } else { Phase::Staging });
            // Advance through the 28 small segments to the multi-chunk segment.
            for _ in 0..28 { l.step(&mut p).unwrap(); }
            let before = l.report();
            let next = s.borrow().guards + if after { 2 } else { 1 };
            s.borrow_mut().deny_guard = Some(next);
            assert_eq!(l.step(&mut p), Err(Error::Guard));
            let r = l.report();
            let delta = if verify { r.target_verified - before.target_verified }
                        else { r.target_writes - before.target_writes };
            assert_eq!(delta, if after { TARGET_CHUNK } else { 0 });
            assert!(!s.borrow().authenticated);
            stopped(&mut l, &mut p, &s);
        }
    }
}
#[test]
fn mpss_target_budget_corruption_at_chunk_file_and_bss_edges_prevents_auth() {
    let (b, extent, start, file_bytes) = target_budget_fixture();
    for at in [start, start+TARGET_CHUNK-1, start+TARGET_CHUNK,
               start+file_bytes-1, start+file_bytes, extent-1] {
        let (mut l, mut p, s) = target_budget_setup(&b, extent);
        advance(&mut l, &mut p, Phase::VerifyTarget);
        s.borrow_mut().target[at] ^= 0x80;
        loop {
            match l.step(&mut p) {
                Ok(Phase::VerifyTarget) => {},
                Err(Error::Readback) => break,
                other => panic!("corruption at {at:#x} accepted: {other:?}"),
            }
        }
        assert!(!s.borrow().authenticated);
        stopped(&mut l, &mut p, &s);
    }
}
