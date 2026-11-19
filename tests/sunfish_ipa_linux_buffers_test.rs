#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_buffers_native.rs"]
mod sunfish_ipa_linux_buffers_native;
use std::{cell::RefCell, rc::Rc};
use sunfish_ipa_linux_buffers::*;
use sunfish_ipa_linux_buffers_native::NativeRam;

#[derive(Clone)]
struct Fake(Rc<RefCell<State>>);
struct State {
    words: Vec<u32>,
    reads: usize,
    writes: usize,
    fail_read: Option<usize>,
    fail_write: Option<usize>,
    suppress_write: Option<usize>,
    corrupt_read: Option<usize>,
}
impl Fake {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(State {
            words: (0..POOL_BYTES / 4).map(|i| 0xa5000000 | i as u32).collect(),
            reads: 0,
            writes: 0,
            fail_read: None,
            fail_write: None,
            suppress_write: None,
            corrupt_read: None,
        })))
    }
    fn counts(&self) -> (usize, usize) {
        let s = self.0.borrow();
        (s.reads, s.writes)
    }
}
impl Memory for Fake {
    fn bytes(&self) -> usize {
        self.0.borrow().words.len() * 4
    }
    fn read32(&mut self, offset: usize) -> Result<u32, MemoryError> {
        let mut s = self.0.borrow_mut();
        let n = s.reads;
        s.reads += 1;
        if s.corrupt_read == Some(n) {
            s.words[0] ^= 1;
        }
        if s.fail_read == Some(n) {
            Err(MemoryError::Io)
        } else {
            Ok(s.words[offset / 4])
        }
    }
    fn write32(&mut self, offset: usize, value: u32) -> Result<(), MemoryError> {
        let mut s = self.0.borrow_mut();
        let n = s.writes;
        s.writes += 1;
        if s.suppress_write != Some(n) {
            s.words[offset / 4] = value;
        }
        if s.fail_write == Some(n) {
            Err(MemoryError::Io)
        } else {
            Ok(())
        }
    }
}
fn no_retry(pool: &mut Pool<Fake>, io: &Fake, expected: Error) {
    let counts = io.counts();
    assert_eq!(
        pool.prepare(
            || panic!("guard on retry"),
            |_, _, _| panic!("progress on retry")
        )
        .error,
        Some(expected)
    );
    assert_eq!(io.counts(), counts);
    assert!(pool.buffer(Kind::Zero).is_none());
}
fn word_edges() -> Vec<usize> {
    let plan = Plan::new().unwrap();
    let mut n = 0;
    let mut indices = vec![];
    for b in plan.buffers() {
        indices.push(n);
        n += b.reserved() / 4;
        indices.push(n - 1);
    }
    indices
}

#[test]
fn linux_requirements_fit_without_overlap_and_ring_iovas_have_size_alignment() {
    let p = Plan::new().unwrap();
    let expected = [
        (Kind::Zero, 0, 0x2000, 0x2000),
        (Kind::Table, 0x2000, 0x88, 0x1000),
        (Kind::Event(1), 0x3000, 0x1000, 0x1000),
        (Kind::Tre(1), 0x4000, 0x1000, 0x1000),
        (Kind::CommandPayload, 0x6000, 0x2000, 0x2000),
        (Kind::Event(2), 0x8000, 0x1000, 0x1000),
        (Kind::Tre(2), 0x9000, 0x1000, 0x1000),
        (Kind::Event(0), 0xa000, 0x2000, 0x2000),
        (Kind::Tre(0), 0xc000, 0x2000, 0x2000),
        (Kind::Event(3), 0xe000, 0x1000, 0x1000),
        (Kind::Tre(3), 0xf000, 0x1000, 0x1000),
    ];
    for (i, (kind, offset, requested, reserved)) in expected.into_iter().enumerate() {
        let b = p.buffers()[i];
        assert_eq!(
            (b.kind(), b.offset(), b.requested(), b.reserved(), b.iova()),
            (
                kind,
                offset,
                requested,
                reserved,
                0x80000000 + offset as u64
            )
        );
        assert_eq!(b.iova() % reserved as u64, 0);
        for earlier in &p.buffers()[..i] {
            assert!(earlier.offset() + earlier.reserved() <= b.offset());
        }
    }
    assert_eq!(
        (p.used_bytes(), p.reserved_bytes(), p.remaining_bytes()),
        (0x10000, 0xf000, 0x10000)
    );
    assert_eq!(
        CHANNELS
            .iter()
            .map(|c| (c.name, c.endpoint_id, c.channel_id))
            .collect::<Vec<_>>(),
        [
            ("AP_COMMAND_TX", 6, 1),
            ("AP_LAN_RX", 8, 2),
            ("AP_MODEM_TX", 1, 0),
            ("AP_MODEM_RX", 9, 3)
        ]
    );
}

#[test]
fn prepares_real_bytes_including_empty_table_and_preserves_padding_and_tail() {
    let io = Fake::new();
    let before = io.0.borrow().words.clone();
    let mut p = Pool::new(io.clone()).unwrap();
    assert!(p.buffer(Kind::Zero).is_none());
    let mut guards = 0;
    let r = p.prepare(
        || {
            guards += 1;
            Ok(())
        },
        |_, _, _| {},
    );
    assert_eq!(r.error, None);
    assert_eq!(
        (r.submitted, r.initialized, r.verified),
        (COMPLETE, COMPLETE, COMPLETE)
    );
    assert_eq!((r.checked_bytes, guards), (0xf000, 34));
    assert_eq!(io.counts(), (30720, 15360));
    let state = io.0.borrow();
    for (i, &value) in state.words.iter().enumerate() {
        let offset = i * 4;
        let allocated = p
            .plan()
            .buffers()
            .iter()
            .any(|b| offset >= b.offset() && offset < b.offset() + b.reserved());
        let expected = if (0x2000..0x2088).contains(&offset) {
            match (offset - 0x2000) / 4 {
                2 => 0x24,
                n if n >= 4 && n % 2 == 0 => 0x80002000,
                _ => 0,
            }
        } else if allocated {
            0
        } else {
            before[i]
        };
        assert_eq!(value, expected, "offset {offset:x}");
    }
    drop(state);
    assert_eq!(p.buffer(Kind::Table).unwrap().iova(), 0x80002000);
    assert_eq!(p.buffer(Kind::CommandPayload).unwrap().reserved(), 8192);
    let counts = io.counts();
    assert_eq!(
        p.prepare(|| panic!(), |_, _, _| panic!()).error,
        Some(Error::AlreadyStarted)
    );
    assert_eq!(io.counts(), counts);
    p.quarantine();
    no_retry(&mut p, &io, Error::Quarantined);
}

#[test]
fn uncertain_writes_at_every_buffer_edge_stop_and_quarantine() {
    for n in word_edges() {
        let io = Fake::new();
        io.0.borrow_mut().fail_write = Some(n);
        let mut p = Pool::new(io.clone()).unwrap();
        let r = p.prepare(|| Ok(()), |_, _, _| {});
        assert_eq!(r.error, Some(Error::Memory(MemoryError::Io)));
        assert_eq!(io.counts(), (0, n + 1));
        assert!(r.submitted & (1 << r.failed_buffer.unwrap()) != 0);
        assert!(r.initialized & (1 << r.failed_buffer.unwrap()) == 0);
        no_retry(&mut p, &io, Error::Quarantined);
    }
}

#[test]
fn both_readback_passes_propagate_each_buffer_edge_failure_without_replay() {
    for pass in 0..2 {
        for edge in word_edges() {
            let n = pass * 15360 + edge;
            let io = Fake::new();
            io.0.borrow_mut().fail_read = Some(n);
            let mut p = Pool::new(io.clone()).unwrap();
            let r = p.prepare(|| Ok(()), |_, _, _| {});
            assert_eq!(r.error, Some(Error::Memory(MemoryError::Io)));
            assert_eq!(io.counts(), (n + 1, 15360));
            assert_eq!(r.initialized, COMPLETE);
            no_retry(&mut p, &io, Error::Quarantined);
        }
    }
}

#[test]
fn non_sticking_words_and_between_pass_corruption_cannot_publish_buffers() {
    for n in word_edges() {
        let io = Fake::new();
        io.0.borrow_mut().suppress_write = Some(n);
        let mut p = Pool::new(io.clone()).unwrap();
        assert_eq!(
            p.prepare(|| Ok(()), |_, _, _| {}).error,
            Some(Error::Readback)
        );
        no_retry(&mut p, &io, Error::Quarantined);
    }
    let io = Fake::new();
    io.0.borrow_mut().corrupt_read = Some(15360);
    let mut p = Pool::new(io.clone()).unwrap();
    let r = p.prepare(|| Ok(()), |_, _, _| {});
    assert_eq!(r.error, Some(Error::Readback));
    assert_eq!(r.verified, 0);
    no_retry(&mut p, &io, Error::Quarantined);
}

#[test]
fn every_domain_or_configuration_guard_failure_consumes_attempt_without_repair() {
    for failure in [Error::Domain, Error::Config] {
        for stop in 0..34 {
            let io = Fake::new();
            let mut p = Pool::new(io.clone()).unwrap();
            let mut n = 0;
            let r = p.prepare(
                || {
                    let at = n;
                    n += 1;
                    if at == stop { Err(failure) } else { Ok(()) }
                },
                |_, _, _| {},
            );
            assert_eq!(r.error, Some(failure));
            if stop == 0 {
                assert_eq!(io.counts(), (0, 0));
            }
            if stop == 33 {
                assert_eq!(r.verified, COMPLETE);
            }
            no_retry(&mut p, &io, Error::Quarantined);
        }
    }
    assert_eq!(Error::Readback.code(), 7);
}

#[test]
fn insufficient_backing_never_reaches_memory_io() {
    let io = Fake::new();
    io.0.borrow_mut().words.pop();
    assert!(matches!(Pool::new(io.clone()), Err(Error::Layout)));
    assert_eq!(io.counts(), (0, 0));
}

#[repr(align(4096))]
struct Aligned([u32; POOL_BYTES / 4]);
fn pages() -> [u64; 32] {
    core::array::from_fn(|i| 0x88001000 + i as u64 * 4096)
}

#[test]
fn concrete_scalar_ram_backend_prepares_pool_even_with_non_ring_aligned_physical_base() {
    let mut backing = Box::new(Aligned([0xa5a5a5a5; POOL_BYTES / 4]));
    // Host-owned test memory stands in for the audited retained native span;
    // no DMA or hardware authority is asserted by this test.
    let ram = unsafe { NativeRam::new(backing.0.as_mut_ptr(), &pages()) }.unwrap();
    assert_eq!(ram.physical_base(), 0x88001000);
    let mut pool = Pool::new(ram).unwrap();
    let r = pool.prepare(|| Ok(()), |_, _, _| {});
    assert_eq!(r.error, None);
    assert_eq!(r.verified, COMPLETE);
    assert_eq!(pool.buffer(Kind::Event(0)).unwrap().iova() % 8192, 0);
    drop(pool);
    assert_eq!(backing.0[0x2008 / 4], 0x24);
    assert_eq!(backing.0[0x2010 / 4], 0x80002000);
    assert_eq!(backing.0[0x5000 / 4], 0xa5a5a5a5);
    assert_eq!(backing.0[0x1fffc / 4], 0xa5a5a5a5);
}

#[test]
fn concrete_backend_rejects_bad_frame_geometry_and_all_out_of_range_word_accesses() {
    let mut backing = Box::new(Aligned([0; POOL_BYTES / 4]));
    let pointer = backing.0.as_mut_ptr();
    for kind in 0..5 {
        let mut bad = pages();
        match kind {
            0 => bad[4] = bad[3],
            1 => bad[0] += 1,
            2 => bad[31] = 1 << 40,
            3 => bad[0] = 0,
            _ => bad[2] += 4096,
        }
        assert!(matches!(
            unsafe { NativeRam::new(pointer, &bad) },
            Err(MemoryError::Layout)
        ));
    }
    assert!(matches!(
        unsafe { NativeRam::new(pointer, &pages()[..31]) },
        Err(MemoryError::Layout)
    ));
    assert!(matches!(
        unsafe { NativeRam::new(core::ptr::null_mut(), &pages()) },
        Err(MemoryError::Layout)
    ));
    assert!(matches!(
        unsafe { NativeRam::new(pointer.wrapping_add(1), &pages()) },
        Err(MemoryError::Layout)
    ));
    let mut ram = unsafe { NativeRam::new(pointer, &pages()) }.unwrap();
    for offset in [1, POOL_BYTES, POOL_BYTES + 4, usize::MAX] {
        assert_eq!(ram.read32(offset), Err(MemoryError::Bounds));
        assert_eq!(ram.write32(offset, 0x12345678), Err(MemoryError::Bounds));
    }
    ram.write32(POOL_BYTES - 4, 0x12345678).unwrap();
    assert_eq!(ram.read32(POOL_BYTES - 4), Ok(0x12345678));
}
