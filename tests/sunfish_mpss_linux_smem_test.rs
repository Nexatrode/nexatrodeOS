use super::*;
use sunfish_mpss_linux_smem::{self as ext, Extension, Kind, Payload, Phase, Store};
use sunfish_mpss_linux_smem_native as native;
#[derive(Default)]
struct Effects {
    writes: Vec<(usize, u32, Kind)>,
    fail: Option<(usize, bool)>,
    corrupt: Option<usize>,
    guards: usize,
    deny: Option<usize>,
    panic: Option<usize>,
    dropped: usize,
}
struct Writer {
    bus: Bus,
    effects: Rc<RefCell<Effects>>,
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.effects.borrow_mut().dropped += 1;
    }
}
impl Payload for Writer {
    fn guard(&mut self, generation: u64) -> Result<(), ext::Error> {
        assert_eq!(generation, 91);
        let mut e = self.effects.borrow_mut();
        e.guards += 1;
        if e.deny == Some(e.guards) {
            Err(ext::Error::Guard)
        } else {
            Ok(())
        }
    }
    fn store(&mut self, r: &Store) -> Result<(), ext::Error> {
        let mut e = self.effects.borrow_mut();
        let mut m = self.bus.0.borrow_mut();
        assert_eq!(m.lock, 1);
        assert!(r.offset() == PART + 12 || (PART + 0x2030..PART + 0x61d8).contains(&r.offset()));
        assert_eq!(r.offset() % 4, 0);
        let index = e.writes.len();
        e.writes.push((r.offset(), r.value(), r.kind()));
        if e.fail == Some((index, false)) {
            return Err(ext::Error::Store);
        }
        word(
            &mut m.bytes,
            r.offset(),
            r.value() ^ u32::from(e.corrupt == Some(index)),
        );
        assert!(e.panic != Some(index), "uncertain publication");
        if e.fail == Some((index, true)) {
            return Err(ext::Error::Store);
        }
        Ok(())
    }
}
type TestExtension = Extension<Bus, Writer>;
fn setup() -> (TestExtension, Bus, Rc<RefCell<Effects>>) {
    let bus = Bus::new();
    let mut parent = Owner::new(bus.clone());
    parent.allocate_once().unwrap();
    let e = Rc::new(RefCell::new(Effects::default()));
    let owner = Extension::new(
        parent,
        Writer {
            bus: bus.clone(),
            effects: e.clone(),
        },
        91,
    );
    (owner, bus, e)
}
fn until(x: &mut TestExtension, phase: Phase) {
    for _ in 0..64 {
        if x.report().phase == phase {
            return;
        }
        x.step().unwrap();
    }
    panic!("unbounded extension");
}
fn stopped(x: &mut TestExtension, e: &Rc<RefCell<Effects>>) {
    let count = e.borrow().writes.len();
    let guards = e.borrow().guards;
    assert_eq!(x.step(), Err(ext::Error::Consumed));
    assert!(x.take_prepared().is_err());
    assert_eq!(e.borrow().writes.len(), count);
    assert_eq!(e.borrow().guards, guards);
}
#[test]
fn mpss_smem_extends_actual_ipa_owner_and_matches_original_linux_layout() {
    let (mut x, bus, e) = setup();
    let before = bus.0.borrow().bytes.clone();
    let mut old = x.report();
    while x.report().phase != Phase::Prepared {
        x.step().unwrap();
        let now = x.report();
        assert!(now.zeroed - old.zeroed <= 4096);
        assert!(now.verified - old.verified <= 4096);
        old = now;
    }
    let r = x.report();
    assert_eq!(
        (
            r.allocations,
            r.zeroed,
            r.verified,
            r.submitted,
            r.completed
        ),
        (3, 16756, 16756, 4201, 4201)
    );
    assert_eq!(r.locks, r.releases);
    assert_eq!(x.parent().retained_item().unwrap().physical(), 0x8611d030);
    assert_eq!(x.take_prepared().unwrap().generation(), 91);
    let oracle =
        std::fs::read_to_string(std::env::var_os("SUNFISH_MPSS_SMEM_C_LAYOUT").unwrap()).unwrap();
    for line in oracle.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let values: Vec<usize> = fields
            .iter()
            .map(|s| usize::from_str_radix(s, 16).unwrap())
            .collect();
        let id = values[0] as u16;
        let d = qcom_modem_status::allocation_directory(&bus, id).unwrap();
        assert_eq!(
            d.item,
            Some(qcom_modem_status::Item {
                offset: PART + values[1],
                size: values[2]
            })
        );
    }
    let writes = e.borrow();
    let m = bus.0.borrow();
    assert_eq!(m.stores, EXPECTED); // no old writer reused
    for i in 0..SIZE {
        if writes
            .writes
            .iter()
            .take(12)
            .any(|(at, _, _)| (*at..*at + 4).contains(&i))
            || ext::ITEMS
                .iter()
                .any(|(_, item)| (item.offset..item.offset + item.size).contains(&i))
        {
            continue;
        }
        assert_eq!(
            m.bytes[i], before[i],
            "foreign/reserved/IPA byte changed at {i:x}"
        );
    }
    for (_, item) in ext::ITEMS {
        assert!(
            m.bytes[item.offset..item.offset + item.size]
                .iter()
                .all(|b| *b == 0)
        );
    }
    assert_eq!(
        &m.bytes[PART + 0x2040 + 340..PART + 0x2040 + 344],
        &before[PART + 0x2040 + 340..PART + 0x2040 + 344]
    );
}
#[test]
fn mpss_smem_requires_real_successful_released_predecessor() {
    for mode in 0..3 {
        let bus = Bus::new();
        let e = Rc::new(RefCell::new(Effects::default()));
        let mut parent = Owner::new(bus.clone());
        if mode == 1 {
            bus.0.borrow_mut().fail_store = Some((3, true));
            let _ = parent.allocate_once();
        }
        if mode == 2 {
            parent.allocate_once().unwrap();
        }
        let mut x = Extension::new(
            parent,
            Writer {
                bus,
                effects: e.clone(),
            },
            if mode == 2 { 0 } else { 91 },
        );
        assert_eq!(x.report().error, Some(ext::Error::Predecessor));
        stopped(&mut x, &e);
        drop(x);
        assert_eq!(e.borrow().dropped, 0);
    }
}
#[test]
fn mpss_smem_duplicate_cursor_route_header_and_foreign_overlap_stop_before_writes() {
    for mode in 0..8 {
        let (mut x, bus, e) = setup();
        let mut m = bus.0.borrow_mut();
        match mode {
            0 => word(&mut m.bytes, PART + 12, 0x2038),
            1 => word(&mut m.bytes, PART + 40, 1),
            2 => word(&mut m.bytes, PART + 36, 0x1ff8),
            3 => half(&mut m.bytes, PART + 34, 478),
            4 => word(&mut m.bytes, TOC + 32 + 7 * 48 + 16, 64),
            5 => {
                word(&mut m.bytes, TOC + 32 + 48, PART as u32);
                word(&mut m.bytes, TOC + 36 + 48, 4096);
            }
            6 => half(&mut m.bytes, TOC + 32 + 11 * 48 + 16, 497),
            _ => word(&mut m.bytes, PART + 16, 0x7b000),
        }
        drop(m);
        assert!(x.step().is_err());
        assert!(e.borrow().writes.is_empty());
        stopped(&mut x, &e);
    }
}
#[test]
fn mpss_smem_every_header_cursor_failure_and_payload_boundaries_are_consumed() {
    for index in (0..12).chain([12, 96, 97, 104, 105, 1128, 2152, 3176, 4200]) {
        for after in [false, true] {
            let (mut x, bus, e) = setup();
            e.borrow_mut().fail = Some((index, after));
            assert_eq!(
                loop {
                    if let Err(err) = x.step() {
                        break err;
                    }
                },
                ext::Error::Store
            );
            assert_eq!(e.borrow().writes.len(), index + 1);
            assert_eq!(x.report().submitted as usize, index + 1);
            assert_eq!(x.report().completed as usize, index);
            assert_eq!(bus.0.borrow().lock, 0);
            stopped(&mut x, &e);
        }
    }
}
#[test]
fn mpss_smem_corrupt_publication_and_late_payload_change_cannot_prepare() {
    for index in [0, 1, 2, 3, 7, 11, 12, 96, 97, 104, 105, 4200] {
        let (mut x, _, e) = setup();
        e.borrow_mut().corrupt = Some(index);
        assert_eq!(
            loop {
                if let Err(err) = x.step() {
                    break err;
                }
            },
            ext::Error::Readback
        );
        stopped(&mut x, &e);
    }
    let (mut x, bus, e) = setup();
    until(&mut x, Phase::Verify);
    word(&mut bus.0.borrow_mut().bytes, ext::ITEMS[0].1.offset, 1);
    assert_eq!(x.step(), Err(ext::Error::Readback));
    stopped(&mut x, &e);
}
#[test]
fn mpss_smem_no_foreign_unlock_or_store_when_lock_is_busy_or_lost() {
    let (mut x, bus, e) = setup();
    bus.0.borrow_mut().busy = true;
    assert_eq!(x.step(), Err(ext::Error::LockBusy));
    assert_eq!(x.report().lock_attempts, 65536);
    stopped(&mut x, &e);
    for at in [1, 2, 3, 4, 5, 6, 9, 12] {
        let (mut x, bus, e) = setup();
        x.step().unwrap();
        let n = bus.0.borrow().lock_reads;
        bus.0.borrow_mut().lose_at = Some(n + at);
        assert_eq!(
            x.step(),
            Err(if at == 1 {
                ext::Error::LockBusy
            } else {
                ext::Error::LockLost
            })
        );
        assert_eq!(bus.0.borrow().lock, 2);
        stopped(&mut x, &e);
    }
}
#[test]
fn mpss_smem_guard_loss_and_unstable_directory_stop_without_repair() {
    for at in [1, 2, 3, 4, 5, 6, 100, 2000, 4205] {
        let (mut x, _, e) = setup();
        e.borrow_mut().deny = Some(at);
        let result = loop {
            match x.step() {
                Err(err) => break err,
                Ok(Phase::Prepared) => panic!("missing guard"),
                _ => (),
            }
        };
        assert_eq!(result, ext::Error::Guard);
        stopped(&mut x, &e);
    }
    let (mut x, bus, e) = setup();
    let scan = bus.0.borrow().scans;
    bus.0.borrow_mut().change_at_scan = Some((scan + 6, PART + 12, 0x2038));
    assert!(x.step().is_err());
    assert!(e.borrow().writes.is_empty());
    stopped(&mut x, &e);
}
#[test]
fn mpss_smem_uncertain_publication_cannot_replay_and_both_providers_stay_retained() {
    for index in [0, 3, 11, 12, 4200] {
        let (mut x, bus, e) = setup();
        e.borrow_mut().panic = Some(index);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| loop {
                x.step().unwrap();
            }))
            .is_err()
        );
        assert_eq!(x.report().error, Some(ext::Error::Consumed));
        stopped(&mut x, &e);
        drop(x);
        assert_eq!(e.borrow().dropped, 0);
        assert_eq!(bus.0.borrow().lock, 1);
    }
}
#[test]
fn mpss_smem_receipt_is_one_use_and_never_available_before_complete_verification() {
    let (mut x, _, e) = setup();
    assert!(x.take_prepared().is_err());
    stopped(&mut x, &e);
    let (mut x, _, e) = setup();
    until(&mut x, Phase::Prepared);
    assert_eq!(x.take_prepared().unwrap().generation(), 91);
    assert!(x.take_prepared().is_err());
    stopped(&mut x, &e);
}

#[repr(align(4096))]
struct Page([u8; 4096]);
struct Gate {
    calls: usize,
    deny: Option<usize>,
}
impl native::Gate for Gate {
    fn check(&mut self, g: u64) -> Result<(), ext::Error> {
        assert_eq!(g, 91);
        self.calls += 1;
        if self.deny == Some(self.calls) {
            Err(ext::Error::Guard)
        } else {
            Ok(())
        }
    }
}
#[test]
fn mpss_smem_actual_volatile_parent_and_extension_preserve_every_other_byte() {
    let original = fixture();
    let mut memory: Vec<Page> = (0..512)
        .map(|i| Page(original[i * 4096..(i + 1) * 4096].try_into().unwrap()))
        .collect();
    let base = memory.as_mut_ptr().cast::<u8>();
    let meta = unsafe { base.add(PART) }.cast::<u32>();
    let mut lock = Page([0; 4096]);
    let lp = lock.0.as_mut_ptr().cast::<u32>();
    let window = unsafe {
        sunfish_ipa_smem_native::Window::new(base, 0x86000000, meta, 0x8611d000, lp, 0x1f43000)
    }
    .unwrap();
    let mut parent = Owner::new(window);
    parent.allocate_once().unwrap();
    let before = unsafe { std::slice::from_raw_parts(base, SIZE) }.to_vec();
    let pages = unsafe { base.add(native::FIRST_PAGE) }.cast::<u32>();
    let payload = unsafe {
        native::Native::new(
            base,
            0x86000000,
            meta,
            0x8611d000,
            pages,
            0x8611f000,
            Gate {
                calls: 0,
                deny: None,
            },
        )
    }
    .ok()
    .unwrap();
    let mut x = Extension::new(parent, payload, 91);
    for _ in 0..64 {
        if x.report().phase == Phase::Prepared {
            break;
        }
        x.step().unwrap();
    }
    assert_eq!(x.report().phase, Phase::Prepared);
    assert_eq!((x.report().submitted, x.report().completed), (4201, 4201));
    let after = unsafe { std::slice::from_raw_parts(base, SIZE) };
    let headers = [PART + 0x2030, PART + 0x2198, PART + 0x21c8];
    for i in 0..SIZE {
        if (PART + 12..PART + 16).contains(&i)
            || headers.iter().any(|h| (*h..*h + 12).contains(&i))
            || ext::ITEMS
                .iter()
                .any(|(_, it)| (it.offset..it.offset + it.size).contains(&i))
        {
            continue;
        }
        assert_eq!(after[i], before[i], "native overwrite at {i:x}");
    }
}
#[test]
fn mpss_smem_native_rejects_substituted_aliases_and_physical_pages() {
    let mut memory: Vec<Page> = (0..512).map(|_| Page([0; 4096])).collect();
    let b = memory.as_mut_ptr().cast::<u8>();
    let m = unsafe { b.add(PART) }.cast::<u32>();
    let p = unsafe { b.add(native::FIRST_PAGE) }.cast::<u32>();
    for (base, meta, pages, pa, mp, pp) in [
        (b, m, p, 0x86000000, 0x8611d000, 0x8611e000),
        (b, m, p, 0x86000001, 0x8611d000, 0x8611f000),
        (b, m, p, 0x86000000, 0x8611e000, 0x8611f000),
        (
            b,
            unsafe { m.add(1) },
            p,
            0x86000000,
            0x8611d000,
            0x8611f000,
        ),
        (
            b,
            m,
            unsafe { p.add(1024) },
            0x86000000,
            0x8611d000,
            0x8611f000,
        ),
    ] {
        assert!(
            unsafe {
                native::Native::new(
                    base,
                    pa,
                    meta,
                    mp,
                    pages,
                    pp,
                    Gate {
                        calls: 0,
                        deny: None,
                    },
                )
            }
            .is_err()
        );
    }
}
