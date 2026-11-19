#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
pub mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
pub mod sunfish_ipa_linux_gsi;
#[path = "../rust/src/sunfish_ipa_linux_receive.rs"]
pub mod sunfish_ipa_linux_receive;
#[path = "../rust/src/sunfish_ipa_linux_receive_native.rs"]
pub mod sunfish_ipa_linux_receive_native;
#[path = "../rust/src/sunfish_ipa_linux_receive_pool.rs"]
pub mod sunfish_ipa_linux_receive_pool;

use std::alloc::{Layout, alloc_zeroed, dealloc};
use sunfish_ipa_linux_buffers::{Memory, MemoryError, PAGE_BYTES};
use sunfish_ipa_linux_receive as rx;
use sunfish_ipa_linux_receive_native::*;
use sunfish_ipa_linux_receive_pool::{DataPool, HEADROOM_CANARY, TAIL_CANARY};

struct Allocation(*mut u32, Layout);
impl Allocation {
    fn new() -> Self {
        let layout = Layout::from_size_align(rx::DATA_RESERVED_BYTES, PAGE_BYTES).unwrap();
        let p = unsafe { alloc_zeroed(layout) }.cast::<u32>();
        assert!(!p.is_null());
        Self(p, layout)
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe { dealloc(self.0.cast(), self.1) };
    }
}
fn pages() -> Vec<u64> {
    (0..RESERVED_PAGES)
        .map(|n| 0x98000000 + (n * PAGE_BYTES) as u64)
        .collect()
}

#[test]
fn rxnative_geometry_and_dma_page_diagnostics_exclude_tail() {
    let a = Allocation::new();
    let physical = pages();
    assert_eq!(RESERVED_PAGES, 512);
    for n in [0, 1, 31, 501, 511, 513] {
        let mut bad = physical.clone();
        bad.resize(n, 0x98000000);
        assert!(matches!(
            unsafe { NativeReceiveRam::new(a.0, &bad) },
            Err(MemoryError::Layout)
        ));
    }
    for index in [0, 1, 255, 501, 511] {
        for value in [0, 0x7ffff000, 0x98000001, 0x98000000, 1u64 << 40, u64::MAX] {
            let mut bad = physical.clone();
            if bad[index] == value {
                continue;
            }
            bad[index] = value;
            assert!(
                matches!(
                    unsafe { NativeReceiveRam::new(a.0, &bad) },
                    Err(MemoryError::Layout)
                ),
                "{index}/{value:x}"
            );
        }
    }
    for p in [
        core::ptr::null_mut(),
        (a.0 as usize + 4) as *mut u32,
        usize::MAX as *mut u32,
        (usize::MAX - (PAGE_BYTES - 1)) as *mut u32,
    ] {
        assert!(matches!(
            unsafe { NativeReceiveRam::new(p, &physical) },
            Err(MemoryError::Layout)
        ));
    }
    let ram = unsafe { NativeReceiveRam::new(a.0, &physical) }.unwrap();
    assert_eq!(ram.physical_base(), physical[0]);
    for (index, &page) in physical.iter().take(rx::DATA_USED_PAGES).enumerate() {
        assert_eq!(ram.dma_page(index), Some(page));
    }
    for n in [502, 503, 511, 512, usize::MAX] {
        assert_eq!(ram.dma_page(n), None);
    }
}

#[test]
fn rxnative_actual_scalar_cpu_stores_reads_and_full_span_bounds() {
    let a = Allocation::new();
    let mut ram = unsafe { NativeReceiveRam::new(a.0, &pages()) }.unwrap();
    assert_eq!(ram.bytes(), rx::DATA_RESERVED_BYTES);
    for offset in (0..rx::DATA_RESERVED_BYTES).step_by(4) {
        ram.write32(offset, 0x01020304 ^ offset as u32).unwrap();
    }
    for offset in (0..rx::DATA_RESERVED_BYTES).step_by(4) {
        assert_eq!(ram.read32(offset), Ok(0x01020304 ^ offset as u32));
    }
    for offset in [
        1,
        2,
        3,
        rx::DATA_RESERVED_BYTES - 1,
        rx::DATA_RESERVED_BYTES,
        usize::MAX,
        usize::MAX - 3,
    ] {
        assert_eq!(ram.read32(offset), Err(MemoryError::Bounds));
        assert_eq!(ram.write32(offset, 0), Err(MemoryError::Bounds));
    }
    assert_eq!(ram.read32(0), Ok(0x01020304));
    assert_eq!(
        ram.read32(rx::DATA_RESERVED_BYTES - 4),
        Ok(0x01020304 ^ (rx::DATA_RESERVED_BYTES - 4) as u32)
    );
}

#[test]
fn rxnative_data_pool_prepares_same_cpu_mapping_without_raw_accessor() {
    let a = Allocation::new();
    let ram = unsafe { NativeReceiveRam::new(a.0, &pages()) }.unwrap();
    let mut pool = DataPool::new(ram).unwrap();
    let report = pool.prepare(|| Ok(()));
    assert_eq!(report.error, None);
    assert_eq!(report.writes_done, 524288);
    assert_eq!(report.checked_words, 1048576);
    assert_eq!(pool.verify_guards(|| Ok(())).checked_words, 14256);
    assert!(pool.prepared());
    // No DMA was ever configured. Inspect only after the backend owner is
    // dropped; this host allocation remains retained by the test allocator.
    drop(pool);
    for (offset, expected) in [
        (0, HEADROOM_CANARY),
        (60, HEADROOM_CANARY),
        (64, 0),
        (rx::DATA_USED_BYTES - 4, 0),
        (rx::DATA_USED_BYTES, TAIL_CANARY),
        (rx::DATA_RESERVED_BYTES - 4, TAIL_CANARY),
    ] {
        assert_eq!(unsafe { a.0.add(offset / 4).read() }, expected);
    }
}
