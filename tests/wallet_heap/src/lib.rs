#[cfg(test)]
mod tests {
    use core::alloc::{GlobalAlloc, Layout};
    use one_shot_mutex::sync::RawOneShotMutex;
    use sel4_dlmalloc::{DeferredStaticDlmalloc, StaticHeapBounds};

    #[test]
    fn reuses_scan_allocations_without_overwriting_live_state() {
        const HEAP: usize = 32 * 1024 * 1024;
        let backing_layout = Layout::from_size_align(HEAP, 4096).unwrap();
        let backing = unsafe { std::alloc::alloc_zeroed(backing_layout) };
        assert!(!backing.is_null());
        let allocator = DeferredStaticDlmalloc::<RawOneShotMutex>::new();
        allocator.set_bounds(StaticHeapBounds::new(backing, HEAP)).unwrap();
        let live_layout = Layout::from_size_align(8192, 64).unwrap();
        let live = unsafe { allocator.alloc(live_layout) };
        assert!(!live.is_null());
        unsafe { core::ptr::write_bytes(live, 0xa5, live_layout.size()); }
        for round in 0..4000 {
            let layout = Layout::from_size_align(2048 + (round % 32) * 1024, 16).unwrap();
            let ptr = unsafe { allocator.alloc(layout) };
            assert!(!ptr.is_null(), "heap exhausted on scan allocation {round}");
            unsafe {
                core::ptr::write_bytes(ptr, (round % 251) as u8, layout.size());
                allocator.dealloc(ptr, layout);
            }
        }
        assert!(unsafe { core::slice::from_raw_parts(live, live_layout.size()) }.iter().all(|b| *b == 0xa5));
        unsafe {
            allocator.dealloc(live, live_layout);
            std::alloc::dealloc(backing, backing_layout);
        }
    }
}
