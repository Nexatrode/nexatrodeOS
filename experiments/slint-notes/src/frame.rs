//! Wire contract for a bounded Slint RGB frame shared with the trusted GPU client.

use core::sync::atomic::{AtomicU32, Ordering};

pub const FRAME_WIDTH: u32 = 300;
pub const FRAME_HEIGHT: u32 = 400;
pub const FRAME_BYTES_PER_PIXEL: u32 = 3;
pub const FRAME_STRIDE_BYTES: u32 = FRAME_WIDTH * FRAME_BYTES_PER_PIXEL;
pub const FRAME_PIXEL_BYTES: usize = FRAME_STRIDE_BYTES as usize * FRAME_HEIGHT as usize;
pub const FRAME_FORMAT_RGB8: u32 = 1;

/// Header followed immediately by `FRAME_PIXEL_BYTES` RGB bytes.
///
/// The producer uses an odd revision while updating the pixels, then publishes
/// the next even revision with Release ordering. The GPU client accepts a frame
/// only when the revision is even and unchanged before and after its copy.
#[repr(C)]
pub struct FrameSurfaceHeader {
    pub revision: AtomicU32,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub format: u32,
}

impl FrameSurfaceHeader {
    pub const fn new() -> Self {
        Self {
            revision: AtomicU32::new(0),
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            stride_bytes: FRAME_STRIDE_BYTES,
            format: FRAME_FORMAT_RGB8,
        }
    }

    pub fn publish(&self, pixels: &mut [u8], source: &[u8]) -> bool {
        if pixels.len() != FRAME_PIXEL_BYTES || source.len() != FRAME_PIXEL_BYTES {
            return false;
        }
        let odd = self.revision.load(Ordering::Relaxed).wrapping_add(1) | 1;
        self.revision.store(odd, Ordering::Release);
        pixels.copy_from_slice(source);
        self.revision.store(odd.wrapping_add(1), Ordering::Release);
        true
    }

    pub fn is_valid(&self, mapped_bytes: usize) -> bool {
        let Some(required) = core::mem::size_of::<Self>().checked_add(FRAME_PIXEL_BYTES) else {
            return false;
        };
        let revision = self.revision.load(Ordering::Acquire);
        self.width == FRAME_WIDTH
            && self.height == FRAME_HEIGHT
            && self.stride_bytes == FRAME_STRIDE_BYTES
            && self.format == FRAME_FORMAT_RGB8
            && mapped_bytes >= required
            && revision != 0
            && revision & 1 == 0
    }
}

impl Default for FrameSurfaceHeader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_expected_bounded_rgb_surface() {
        let header = FrameSurfaceHeader::new();
        let required = core::mem::size_of::<FrameSurfaceHeader>() + FRAME_PIXEL_BYTES;
        assert!(!header.is_valid(required)); // unpublished
        header.revision.store(2, Ordering::Release);
        assert!(header.is_valid(required));
        assert!(!header.is_valid(required - 1));
    }

    #[test]
    fn rejects_wrong_dimensions_stride_and_format() {
        let mut header = FrameSurfaceHeader::new();
        header.revision.store(2, Ordering::Release);
        header.width = FRAME_WIDTH + 1;
        assert!(!header.is_valid(usize::MAX));
        header.width = FRAME_WIDTH;
        header.stride_bytes -= 1;
        assert!(!header.is_valid(usize::MAX));
        header.stride_bytes = FRAME_STRIDE_BYTES;
        header.format = 0xffff;
        assert!(!header.is_valid(usize::MAX));
    }

    #[test]
    fn publish_checks_both_buffers_before_writing() {
        let header = FrameSurfaceHeader::new();
        let mut surface = [0; 6];
        assert!(!header.publish(&mut surface, &[1, 2, 3]));
        assert_eq!(surface, [0; 6]);
    }

    #[test]
    fn publish_copies_a_full_frame_then_marks_it_stable() {
        let header = FrameSurfaceHeader::new();
        let source = alloc::vec![0x5a; FRAME_PIXEL_BYTES];
        let mut surface = alloc::vec![0; FRAME_PIXEL_BYTES];
        assert!(header.publish(&mut surface, &source));
        assert_eq!(header.revision.load(Ordering::Acquire), 2);
        assert!(header.is_valid(core::mem::size_of::<FrameSurfaceHeader>() + FRAME_PIXEL_BYTES));
        assert_eq!(surface, source);
    }
}
