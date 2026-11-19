//! Minimal C ABI memory routines required by the seL4 Rust runtime.
//!
//! These implementations use volatile byte accesses so LLVM does not lower
//! their loops back into calls to the same symbols. They run on ordinary RAM
//! buffers only; device MMIO must use explicit volatile register accessors.

use core::ffi::c_void;

/// Copies `count` bytes from `source` to `destination`.
///
/// # Safety
/// The caller must provide valid, non-overlapping regions of `count` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(
    destination: *mut c_void,
    source: *const c_void,
    count: usize,
) -> *mut c_void {
    let destination = destination.cast::<u8>();
    let source = source.cast::<u8>();
    for offset in 0..count {
        let byte = unsafe { source.add(offset).read_volatile() };
        unsafe { destination.add(offset).write_volatile(byte) };
    }
    destination.cast::<c_void>()
}

/// Copies `count` ordinary RAM bytes, allowing source/destination overlap.
///
/// # Safety
/// Both regions must be valid for `count` bytes, with destination writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(
    destination: *mut c_void,
    source: *const c_void,
    count: usize,
) -> *mut c_void {
    let destination = destination.cast::<u8>();
    let source = source.cast::<u8>();
    if destination as usize > source as usize {
        for offset in (0..count).rev() {
            let byte = unsafe { source.add(offset).read_volatile() };
            unsafe { destination.add(offset).write_volatile(byte) };
        }
    } else if destination.cast_const() != source {
        for offset in 0..count {
            let byte = unsafe { source.add(offset).read_volatile() };
            unsafe { destination.add(offset).write_volatile(byte) };
        }
    }
    destination.cast::<c_void>()
}

/// Fills `count` bytes at `destination` with the low byte of `value`.
///
/// # Safety
/// The caller must provide a valid writable region of `count` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(destination: *mut c_void, value: i32, count: usize) -> *mut c_void {
    let destination = destination.cast::<u8>();
    for offset in 0..count {
        unsafe { destination.add(offset).write_volatile(value as u8) };
    }
    destination.cast::<c_void>()
}

/// Compares two ordinary RAM buffers as unsigned bytes (C ABI).
/// This is not a constant-time comparison for secret values.
///
/// # Safety
/// Both pointers must reference readable regions of `count` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(left: *const c_void, right: *const c_void, count: usize) -> i32 {
    let left = left.cast::<u8>();
    let right = right.cast::<u8>();
    for offset in 0..count {
        let a = unsafe { left.add(offset).read_volatile() };
        let b = unsafe { right.add(offset).read_volatile() };
        if a != b {
            return i32::from(a) - i32::from(b);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{memcmp, memmove};

    #[test]
    fn overlapping_moves_match_snapshot_in_both_directions_and_keep_canaries() {
        for source in 1..9 {
            for destination in 1..9 {
                for count in 0..=8 {
                    let original = core::array::from_fn::<_, 18, _>(|i| i as u8);
                    let mut actual = original;
                    let mut expected = original;
                    expected[destination..destination + count]
                        .copy_from_slice(&original[source..source + count]);
                    let p = actual.as_mut_ptr();
                    let returned =
                        unsafe { memmove(p.add(destination).cast(), p.add(source).cast(), count) };
                    assert_eq!(returned, unsafe { p.add(destination).cast() });
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    fn compares_unsigned_bytes_with_first_difference_and_exact_bound() {
        let a = [0u8, 0xff, 1];
        let b = [0u8, 0x7f, 255];
        unsafe {
            assert_eq!(memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 0), 0);
            assert_eq!(memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 1), 0);
            assert!(memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 3) > 0);
            assert!(memcmp(b.as_ptr().cast(), a.as_ptr().cast(), 3) < 0);
            assert_eq!(memcmp(a.as_ptr().cast(), a.as_ptr().cast(), 3), 0);
        }
    }
}
