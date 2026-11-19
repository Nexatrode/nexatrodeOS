use crate::sunfish_mpss_linux_loader::{BASE, BYTES, Error, METADATA_BYTES, Memory};
const META_PA: u64 = 0x81000000;
#[test]
fn mpss_native_window_checks_complete_scalars_and_keeps_adjacent_bytes() {
    use crate::sunfish_mpss_linux_memory::Window;
    // Owned host allocations simulate the two exclusive views. No physical
    // mapping or firmware ownership is inferred from these numerical values.
    let ml = std::alloc::Layout::from_size_align(METADATA_BYTES + 8192, 4096).unwrap();
    let tl = std::alloc::Layout::from_size_align(BYTES + 8192, 4096).unwrap();
    unsafe {
        let m = std::alloc::alloc_zeroed(ml);
        let t = std::alloc::alloc_zeroed(tl);
        assert!(!m.is_null() && !t.is_null());
        m.write_bytes(0xa5, 4096);
        m.add(METADATA_BYTES + 4096).write_bytes(0xa5, 4096);
        t.write_bytes(0xa5, 4096);
        t.add(BYTES + 4096).write_bytes(0xa5, 4096);
        let mut w = Window::new(
            m.add(4096),
            [META_PA, META_PA + 4096, META_PA + 8192],
            t.add(4096),
            BASE,
            BYTES,
        )
        .ok()
        .unwrap();
        w.metadata_write(METADATA_BYTES - 1, 0x12).unwrap();
        assert_eq!(w.metadata_read(METADATA_BYTES - 1), Ok(0x12));
        w.target_write32(BYTES - 4, 0x13579bdf).unwrap();
        assert_eq!(w.target_read32(BYTES - 4), Ok(0x13579bdf));
        w.target_write8(3, 0x34).unwrap();
        assert_eq!(w.target_read8(3), Ok(0x34));
        for at in [1, 3, BYTES - 3, BYTES, usize::MAX] {
            assert_eq!(w.target_write32(at, 0), Err(Error::Geometry));
            assert_eq!(w.target_read32(at), Err(Error::Geometry));
        }
        for at in [BYTES, usize::MAX] {
            assert_eq!(w.target_write8(at, 0), Err(Error::Geometry));
            assert_eq!(w.target_read8(at), Err(Error::Geometry));
        }
        assert_eq!(w.metadata_write(METADATA_BYTES, 0), Err(Error::Geometry));
        assert_eq!(w.metadata_read(METADATA_BYTES), Err(Error::Geometry));
        for pa in [
            0x86000000,
            BASE,
            0x98f80000,
            0xfde01000,
            META_PA + 1,
            (1 << 40) - 4096,
        ] {
            assert!(
                Window::new(
                    m.add(4096),
                    [pa, pa + 4096, pa + 8192],
                    t.add(4096),
                    BASE,
                    BYTES
                )
                .is_err()
            );
        }
        assert!(
            Window::new(
                m.add(4096),
                [META_PA, META_PA + 4096, META_PA + 12288],
                t.add(4096),
                BASE,
                BYTES
            )
            .is_err()
        );
        assert!(
            Window::new(
                m.add(4096),
                [META_PA, META_PA + 4096, META_PA + 8192],
                t.add(4096),
                BASE + 4096,
                BYTES
            )
            .is_err()
        );
        for (p, n) in [
            (m, 0),
            (m, METADATA_BYTES + 4096),
            (t, 0),
            (t, BYTES + 4096),
        ] {
            assert!(
                std::slice::from_raw_parts(p.add(n), 4096)
                    .iter()
                    .all(|b| *b == 0xa5)
            );
        }
        // All host operations are complete; this test never submitted PAS.
        std::alloc::dealloc(m, ml);
        std::alloc::dealloc(t, tl);
    }
}
