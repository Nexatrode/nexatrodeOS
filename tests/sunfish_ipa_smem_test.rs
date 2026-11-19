//! Independent metadata fixtures and failure injection; no device access.
#![allow(dead_code)]

#[path = "../rust/src/qcom_modem_status.rs"]
mod qcom_modem_status;
#[path = "../rust/src/sunfish_ipa_smem.rs"]
mod sunfish_ipa_smem;
#[path = "../rust/src/sunfish_ipa_smem_native.rs"]
mod sunfish_ipa_smem_native;

// The composed extension reuses this exact real predecessor and lock model.
// Default predecessor suites keep their original scope and dependencies.
#[cfg(mpss_smem_tests)]
#[path = "sunfish_mpss_linux_smem_test.rs"]
mod mpss;
#[cfg(mpss_smp2p_tests)]
#[path = "sunfish_mpss_linux_smp2p_test.rs"]
mod smp2p;
#[cfg(any(mpss_smem_tests, mpss_smp2p_tests))]
#[path = "../rust/src/sunfish_mpss_linux_smem.rs"]
mod sunfish_mpss_linux_smem;
#[cfg(any(mpss_smem_tests, mpss_smp2p_tests))]
#[path = "../rust/src/sunfish_mpss_linux_smem_native.rs"]
mod sunfish_mpss_linux_smem_native;
#[cfg(any(mpss_smem_tests, mpss_smp2p_tests))]
#[path = "../rust/src/sunfish_mpss_linux_smp2p.rs"]
mod sunfish_mpss_linux_smp2p;
#[cfg(mpss_smp2p_tests)]
#[path = "../rust/src/sunfish_mpss_linux_smp2p_native.rs"]
mod sunfish_mpss_linux_smp2p_native;

use qcom_modem_status::Read;
use std::{cell::RefCell, rc::Rc};
use sunfish_ipa_smem::{Error, Io, Owner, StoreError};

const PART: usize = 0x11d000;
const TOC: usize = 0x1ff000;
const SIZE: usize = 0x200000;
const EXPECTED: [(usize, u32); 4] = [
    (0x11d020, 0x01f1a5a5),
    (0x11d024, 0x2000),
    (0x11d028, 0),
    (0x11d00c, 0x2030),
];
fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn half(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn fixture() -> Vec<u8> {
    let mut b = vec![0x5a; SIZE];
    word(&mut b, 92, 12 << 16);
    word(&mut b, 192, 1);
    word(&mut b, 204, 0);
    b[TOC..].fill(0);
    word(&mut b, TOC, 0x434f5424);
    word(&mut b, TOC + 4, 1);
    word(&mut b, TOC + 8, 11);
    word(&mut b, TOC + 32, TOC as u32);
    word(&mut b, TOC + 36, 4096);
    half(&mut b, TOC + 44, 0xffff);
    half(&mut b, TOC + 46, 0xffff);
    let target = TOC + 32 + 7 * 48;
    word(&mut b, target, PART as u32);
    word(&mut b, target + 4, 0x7c000);
    half(&mut b, target + 12, 0);
    half(&mut b, target + 14, 1);
    word(&mut b, target + 16, 32);
    word(&mut b, PART, 0x54525024);
    half(&mut b, PART + 4, 0);
    half(&mut b, PART + 6, 1);
    word(&mut b, PART + 8, 0x7c000);
    word(&mut b, PART + 12, 32);
    word(&mut b, PART + 16, 0x7c000);
    let info = TOC + 32 + 11 * 48;
    b[info..info + 4].copy_from_slice(b"SIII");
    half(&mut b, info + 16, 621);
    b
}

struct Model {
    bytes: Vec<u8>,
    lock: u32,
    busy: bool,
    lock_reads: usize,
    lose_at: Option<usize>,
    lock_writes: Vec<u32>,
    stores: Vec<(usize, u32)>,
    fail_store: Option<(usize, bool)>,
    corrupt_store: Option<usize>,
    scans: usize,
    change_at_scan: Option<(usize, usize, u32)>,
    barriers: usize,
    barriers_at_store: Vec<usize>,
    watch: usize,
    watched_reads: usize,
    change_at_read: Option<(usize, usize, u8)>,
}
#[derive(Clone)]
struct Bus(Rc<RefCell<Model>>);
impl Bus {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(Model {
            bytes: fixture(),
            lock: 0,
            busy: false,
            lock_reads: 0,
            lose_at: None,
            lock_writes: vec![],
            stores: vec![],
            fail_store: None,
            corrupt_store: None,
            scans: 0,
            change_at_scan: None,
            barriers: 0,
            barriers_at_store: vec![],
            watch: usize::MAX,
            watched_reads: 0,
            change_at_read: None,
        })))
    }
}
impl Read for Bus {
    fn read8(&self, offset: usize) -> u8 {
        assert!(offset < SIZE);
        assert!(
            !(PART + 0x30..PART + 0x2030).contains(&offset),
            "payload read"
        );
        let mut m = self.0.borrow_mut();
        if offset == m.watch {
            m.watched_reads += 1;
            if let Some((count, at, value)) = m.change_at_read {
                if m.watched_reads == count {
                    m.bytes[at] = value;
                }
            }
        }
        if offset == 192 {
            m.scans += 1;
            if let Some((scan, at, value)) = m.change_at_scan {
                if scan == m.scans {
                    word(&mut m.bytes, at, value);
                }
            }
        }
        m.bytes[offset]
    }
}
// SAFETY: this model implements hardware arbitration/uncertainty explicitly.
unsafe impl Io for Bus {
    fn barrier(&mut self) {
        self.0.borrow_mut().barriers += 1;
    }
    fn lock_read(&mut self) -> u32 {
        let mut m = self.0.borrow_mut();
        m.lock_reads += 1;
        if m.lose_at == Some(m.lock_reads) {
            m.lock = 2;
        }
        m.lock
    }
    fn lock_write(&mut self, value: u32) {
        let mut m = self.0.borrow_mut();
        m.lock_writes.push(value);
        match value {
            1 if !m.busy && m.lock == 0 => m.lock = 1,
            0 => {
                assert_eq!(m.lock, 1, "foreign lock released");
                m.lock = 0;
            }
            1 => (),
            _ => panic!("unexpected hardware-lock value"),
        }
    }
    fn store_metadata(&mut self, offset: usize, value: u32) -> Result<(), StoreError> {
        let mut m = self.0.borrow_mut();
        assert_eq!(m.lock, 1);
        let step = m.stores.len();
        assert_eq!((offset, value), EXPECTED[step]);
        let barriers = m.barriers;
        m.barriers_at_store.push(barriers);
        m.stores.push((offset, value));
        if m.fail_store == Some((step, false)) {
            return Err(StoreError::Uncertain);
        }
        word(&mut m.bytes, offset, value);
        if m.fail_store == Some((step, true)) {
            return Err(StoreError::Uncertain);
        }
        if m.corrupt_store == Some(step) {
            word(&mut m.bytes, offset, value ^ 1);
        }
        Ok(())
    }
}

fn assert_no_other_byte_changed(before: &[u8], after: &[u8]) {
    for (i, (&a, &b)) in before.iter().zip(after).enumerate() {
        if !EXPECTED.iter().any(|(at, _)| (*at..*at + 4).contains(&i)) {
            assert_eq!(a, b, "unplanned mutation at {i:x}");
        }
    }
}

#[test]
fn allocation_matches_measured_publication_without_touching_payload_or_reserved_word() {
    let bus = Bus::new();
    let original = bus.0.borrow().bytes.clone();
    let mut owner = Owner::new(bus.clone());
    let item = *owner.allocate_once().unwrap();
    assert_eq!((item.physical(), item.bytes()), (0x8611d030, 8192));
    assert_eq!(item.page_span(), (0x8611d000, 3 * 4096));
    assert_eq!(owner.retained_item(), Some(&item));
    let r = owner.report();
    assert_eq!((r.pre_scans, r.post_scans, r.lock_attempts), (2, 2, 1));
    assert_eq!((r.submitted, r.completed, r.item_limit), (15, 15, 621));
    assert!(r.consumed && r.published && r.release_sent);
    let m = bus.0.borrow();
    assert_eq!(m.stores, EXPECTED);
    assert_eq!(m.lock_writes, [1, 0]);
    assert_no_other_byte_changed(&original, &m.bytes);
    // The explicit publication barrier adds one over the ordinary write gap.
    assert!(
        m.barriers_at_store[3] - m.barriers_at_store[2]
            > m.barriers_at_store[2] - m.barriers_at_store[1]
    );
}

#[test]
fn both_host_orientations_and_linux_empty_slot_rule_are_accepted() {
    for reverse in [false, true] {
        let bus = Bus::new();
        {
            let mut m = bus.0.borrow_mut();
            if reverse {
                for at in [PART + 4, TOC + 32 + 7 * 48 + 12] {
                    half(&mut m.bytes, at, 1);
                    half(&mut m.bytes, at + 2, 0);
                }
            }
            // These slots are skipped by Linux if either field is zero.
            word(&mut m.bytes, TOC + 32 + 48 + 4, 0xffff_ffff);
            word(&mut m.bytes, TOC + 32 + 2 * 48, 0xffff_ffff);
            // A real unrelated nonoverlapping span also remains untouched.
            word(&mut m.bytes, TOC + 32 + 3 * 48, 0x10000);
            word(&mut m.bytes, TOC + 32 + 3 * 48 + 4, 0x1000);
        }
        Owner::new(bus).allocate_once().unwrap();
    }
}

#[test]
fn item_limit_falls_back_to_512_and_rejects_id_outside_declared_limit() {
    for (magic, limit, pass) in [(0, 0, true), (0x49494953, 497, false)] {
        let bus = Bus::new();
        {
            let mut m = bus.0.borrow_mut();
            let at = TOC + 32 + 11 * 48;
            word(&mut m.bytes, at, magic);
            half(&mut m.bytes, at + 16, limit);
        }
        let mut owner = Owner::new(bus.clone());
        if pass {
            owner.allocate_once().unwrap();
            assert_eq!(owner.report().item_limit, 512);
        } else {
            assert_eq!(owner.allocate_once(), Err(Error::ItemLimit));
            assert!(bus.0.borrow().stores.is_empty());
            assert!(owner.report().release_sent);
        }
    }
}

#[test]
fn changed_toc_descriptor_count_target_and_foreign_spans_stop_before_stores() {
    let cases = [
        (TOC + 8, 10),
        (TOC + 32, TOC as u32 - 4096),
        (TOC + 36, 8192),
        (TOC + 44, 0xffff0001),
        (TOC + 32 + 7 * 48 + 16, 64),
        (PART + 16, 0x7b000),
        (TOC + 32 + 48, 0x11d000),
        (TOC + 32 + 2 * 48, 0x1ff000),
    ];
    for (at, value) in cases {
        let bus = Bus::new();
        {
            let mut m = bus.0.borrow_mut();
            if at == TOC + 32 + 48 || at == TOC + 32 + 2 * 48 {
                word(&mut m.bytes, at + 4, 0x1000);
            }
            word(&mut m.bytes, at, value);
        }
        let before = bus.0.borrow().bytes.clone();
        let mut owner = Owner::new(bus.clone());
        assert!(owner.allocate_once().is_err(), "accepted mutation {at:x}");
        assert!(owner.retained_item().is_none());
        assert!(bus.0.borrow().stores.is_empty());
        assert_eq!(bus.0.borrow().bytes, before);
        assert!(owner.report().release_sent);
        assert_eq!(owner.allocate_once(), Err(Error::Consumed));
    }
}

#[test]
fn already_present_item_and_changed_two_scan_snapshot_are_not_reused() {
    for unstable in [false, true] {
        let bus = Bus::new();
        {
            let mut m = bus.0.borrow_mut();
            if unstable {
                m.change_at_scan = Some((2, TOC + 32 + 7 * 48 + 16, 64));
            } else {
                for (at, value) in EXPECTED {
                    word(&mut m.bytes, at, value);
                }
            }
        }
        let mut owner = Owner::new(bus.clone());
        assert_eq!(
            owner.allocate_once(),
            Err(if unstable {
                Error::Unstable
            } else {
                Error::Target
            })
        );
        assert!(owner.retained_item().is_none());
        assert!(bus.0.borrow().stores.is_empty());
    }
}

#[test]
fn all_eight_uncertain_store_outcomes_are_retained_and_cannot_retry() {
    for step in 0..4 {
        for applied in [false, true] {
            let bus = Bus::new();
            let before = bus.0.borrow().bytes.clone();
            bus.0.borrow_mut().fail_store = Some((step, applied));
            let mut owner = Owner::new(bus.clone());
            assert_eq!(
                owner.allocate_once(),
                Err(Error::Store {
                    step: step as u8,
                    source: StoreError::Uncertain
                })
            );
            let r = owner.report();
            assert_eq!(r.submitted, (1 << (step + 1)) - 1);
            assert_eq!(r.completed, (1 << step) - 1);
            assert!(!r.published && r.release_sent);
            assert!(owner.retained_item().is_none());
            let m = bus.0.borrow();
            assert_eq!(m.stores.len(), step + 1);
            assert_no_other_byte_changed(&before, &m.bytes);
            let retained = m.bytes.clone();
            let lock_writes = m.lock_writes.len();
            drop(m);
            assert_eq!(owner.allocate_once(), Err(Error::Consumed));
            let m = bus.0.borrow();
            assert_eq!(m.bytes, retained);
            assert_eq!(m.lock_writes.len(), lock_writes);
            assert_eq!(m.stores.len(), step + 1);
        }
    }
}

#[test]
fn readback_failure_at_each_word_never_rolls_back_or_publishes_a_lease() {
    for step in 0..4 {
        let bus = Bus::new();
        bus.0.borrow_mut().corrupt_store = Some(step);
        let mut owner = Owner::new(bus.clone());
        assert_eq!(
            owner.allocate_once(),
            Err(Error::Readback { step: step as u8 })
        );
        assert_eq!(bus.0.borrow().stores.len(), step + 1);
        assert!(owner.retained_item().is_none());
        assert_eq!(owner.report().completed, (1 << step) - 1);
        assert!(owner.report().release_sent);
    }
}

#[test]
fn loss_at_every_owned_boundary_prevents_foreign_release_and_later_stores() {
    let reference = Bus::new();
    Owner::new(reference.clone()).allocate_once().unwrap();
    let boundaries = reference.0.borrow().lock_reads;
    for boundary in 2..=boundaries {
        let bus = Bus::new();
        bus.0.borrow_mut().lose_at = Some(boundary);
        let mut owner = Owner::new(bus.clone());
        assert_eq!(
            owner.allocate_once(),
            Err(Error::LockLost),
            "boundary {boundary}"
        );
        assert!(!owner.report().release_sent);
        assert!(owner.retained_item().is_none());
        assert_eq!(bus.0.borrow().lock_writes, [1]);
        assert_eq!(bus.0.borrow().lock, 2);
        let submitted = bus.0.borrow().stores.len();
        assert_eq!(owner.allocate_once(), Err(Error::Consumed));
        assert_eq!(bus.0.borrow().stores.len(), submitted);
    }
}

#[test]
fn busy_hardware_lock_is_bounded_and_is_never_released() {
    let bus = Bus::new();
    {
        let mut m = bus.0.borrow_mut();
        m.busy = true;
        m.lock = 2;
    }
    let mut owner = Owner::new(bus.clone());
    assert_eq!(owner.allocate_once(), Err(Error::LockBusy));
    assert_eq!(owner.report().lock_attempts, 65536);
    assert!(!owner.report().release_sent);
    assert!(bus.0.borrow().stores.is_empty());
    assert_eq!(bus.0.borrow().scans, 0);
    assert!(bus.0.borrow().lock_writes.iter().all(|v| *v == 1));
}

#[test]
fn failure_during_each_post_lookup_does_not_return_item_info() {
    for scan in [3, 4] {
        let bus = Bus::new();
        bus.0.borrow_mut().change_at_scan = Some((scan, PART + 8, 0x7b000));
        let mut owner = Owner::new(bus.clone());
        assert!(owner.allocate_once().is_err());
        assert_eq!(
            (owner.report().submitted, owner.report().completed),
            (15, 15)
        );
        assert!(owner.report().published && owner.report().release_sent);
        assert!(owner.retained_item().is_none());
        assert_eq!(bus.0.borrow().stores.len(), 4);
        assert_eq!(owner.allocate_once(), Err(Error::Consumed));
    }
}

#[repr(C, align(4096))]
#[derive(Clone)]
struct Page([u8; 4096]);

#[test]
fn native_window_uses_only_the_one_rw_metadata_page_and_exact_lock() {
    let mut memory = vec![Page([0; 4096]); 512];
    let mut lock = Box::new(Page([0; 4096]));
    let base = memory.as_mut_ptr().cast::<u8>();
    let bytes = fixture();
    // SAFETY: the Vec consists of exactly 512 contiguous byte-only pages.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), base, SIZE) };
    let meta = unsafe { base.add(PART) }.cast::<u32>();
    let lock_pointer = lock.0.as_mut_ptr().cast::<u32>();
    let window = unsafe {
        sunfish_ipa_smem_native::Window::new(
            base,
            0x86000000,
            meta,
            0x8611d000,
            lock_pointer,
            0x01f43000,
        )
    }
    .unwrap();
    let targets = [435, 428, 478, 479, 497];
    let batch = qcom_modem_status::allocation_directories(&window, targets).unwrap();
    for (index, id) in targets.into_iter().enumerate() {
        assert_eq!(batch[index], qcom_modem_status::allocation_directory(&window, id).unwrap());
    }
    let mut owner = Owner::new(window);
    assert_eq!(owner.allocate_once().unwrap().physical(), 0x8611d030);
    assert_eq!(
        (owner.report().submitted, owner.report().completed),
        (15, 15)
    );
    drop(owner);
    let after = unsafe { std::slice::from_raw_parts(base, SIZE) };
    assert_no_other_byte_changed(&bytes, after);
    assert_eq!(lock.0, [0; 4096]);
}

#[test]
fn native_metadata_word_reads_match_bytes_at_aligned_unaligned_and_end_offsets() {
    use qcom_modem_status::Read;
    let mut memory = vec![Page([0; 4096]); 512];
    let mut lock = Box::new(Page([0; 4096]));
    let base = memory.as_mut_ptr().cast::<u8>();
    let bytes = unsafe { std::slice::from_raw_parts_mut(base, SIZE) };
    for (at, byte) in bytes.iter_mut().enumerate() { *byte = (at % 251) as u8; }
    let offsets = [0, 1, 4, PART, PART + 3, SIZE - 4];
    let expected: [u32; 6] = offsets.map(|at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()));
    let window = unsafe {
        sunfish_ipa_smem_native::Window::new(base, 0x86000000,
            base.add(PART).cast::<u32>(), 0x8611d000,
            lock.0.as_mut_ptr().cast::<u32>(), 0x01f43000)
    }.unwrap();
    for (at, value) in offsets.into_iter().zip(expected) {
        assert_eq!(window.read_le32(at), value);
    }
}

#[test]
fn native_window_rejects_wrong_identity_alias_alignment_order_and_payload_writes() {
    let mut memory = vec![Page([0; 4096]); 512];
    let mut lock = Box::new(Page([0; 4096]));
    let base = memory.as_mut_ptr().cast::<u8>();
    let meta = unsafe { base.add(PART) }.cast::<u32>();
    let lp = lock.0.as_mut_ptr().cast::<u32>();
    for args in [
        (base, 0x86001000, meta, 0x8611d000, lp, 0x01f43000),
        (base, 0x86000000, meta, 0x8611e000, lp, 0x01f43000),
        (base, 0x86000000, meta, 0x8611d000, lp, 0x01f44000),
        (
            unsafe { base.add(1) },
            0x86000000,
            meta,
            0x8611d000,
            lp,
            0x01f43000,
        ),
        (
            base,
            0x86000000,
            unsafe { meta.add(1024) },
            0x8611d000,
            lp,
            0x01f43000,
        ),
        (base, 0x86000000, meta, 0x8611d000, base.cast(), 0x01f43000),
    ] {
        assert!(
            unsafe {
                sunfish_ipa_smem_native::Window::new(args.0, args.1, args.2, args.3, args.4, args.5)
            }
            .is_err()
        );
    }
    let mut window = unsafe {
        sunfish_ipa_smem_native::Window::new(base, 0x86000000, meta, 0x8611d000, lp, 0x01f43000)
    }
    .unwrap();
    for (at, value) in [
        (PART + 0x30, 1),
        EXPECTED[1],
        (PART, 0),
        (PART + 0x20, 0),
        (PART + 4096, 0),
    ] {
        assert_eq!(window.store_metadata(at, value), Err(StoreError::Rejected));
    }
    assert_eq!(
        window.store_metadata(EXPECTED[0].0, EXPECTED[0].1),
        Err(StoreError::Rejected)
    );
    window.lock_write(1);
    for (at, value) in EXPECTED {
        window.store_metadata(at, value).unwrap();
    }
    assert_eq!(
        window.store_metadata(EXPECTED[0].0, EXPECTED[0].1),
        Err(StoreError::Rejected)
    );
    for (i, page) in memory.iter().enumerate() {
        if i != PART / 4096 {
            assert_eq!(page.0, [0; 4096]);
        }
    }
}
