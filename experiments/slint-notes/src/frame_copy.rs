//! Bounded copies between ordinary RAM buffers used by the UTM frame path.

/// # Safety
/// Both regions must be valid for `len` bytes, non-overlapping and ordinary
/// RAM. The destination must be writable. This must never be used for MMIO.
pub unsafe fn copy_nonoverlapping_ram(destination: *mut u8, source: *const u8, len: usize) {
    let alignment = (destination as usize) ^ (source as usize);
    let width = if alignment & 7 == 0 { 8 } else if alignment & 3 == 0 { 4 } else { 1 };
    let mut offset = 0;
    while offset < len && ((destination as usize + offset) & (width - 1)) != 0 {
        unsafe { destination.add(offset).write_volatile(source.add(offset).read_volatile()) };
        offset += 1;
    }
    if width == 8 {
        while len - offset >= 8 {
            let word = unsafe { source.add(offset).cast::<u64>().read_volatile() };
            unsafe { destination.add(offset).cast::<u64>().write_volatile(word) };
            offset += 8;
        }
    } else if width == 4 {
        while len - offset >= 4 {
            let word = unsafe { source.add(offset).cast::<u32>().read_volatile() };
            unsafe { destination.add(offset).cast::<u32>().write_volatile(word) };
            offset += 4;
        }
    }
    while offset < len {
        unsafe { destination.add(offset).write_volatile(source.add(offset).read_volatile()) };
        offset += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_exact_bytes_for_aligned_and_unaligned_ranges() {
        let source: [u8; 320] = core::array::from_fn(|i| (i.wrapping_mul(37)) as u8);
        for src_offset in 0..8 {
            for dst_offset in 0..8 {
                for len in 0..=128 {
                    let mut actual = [0xa5; 320];
                    let mut expected = actual;
                    expected[dst_offset..dst_offset + len].copy_from_slice(&source[src_offset..src_offset + len]);
                    unsafe { copy_nonoverlapping_ram(actual.as_mut_ptr().add(dst_offset), source.as_ptr().add(src_offset), len) };
                    assert_eq!(actual, expected, "src={src_offset} dst={dst_offset} len={len}");
                }
            }
        }
    }
}
