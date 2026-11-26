//! Wire contract for a bounded Slint RGB frame shared with the trusted GPU client.

use core::sync::atomic::{AtomicU32, Ordering};

pub const FRAME_WIDTH: u32 = if cfg!(feature = "desktop-real-app") { 1120 } else { 1080 };
pub const FRAME_HEIGHT: u32 = if cfg!(feature = "desktop-real-app") { 1040 } else { 2340 };
pub const FRAME_BYTES_PER_PIXEL: u32 = 3;
pub const FRAME_STRIDE_BYTES: u32 = FRAME_WIDTH * FRAME_BYTES_PER_PIXEL;
pub const FRAME_PIXEL_BYTES: usize = FRAME_STRIDE_BYTES as usize * FRAME_HEIGHT as usize;
pub const FRAME_FORMAT_RGB8: u32 = 1;

/// Whether a Slint damage interval overlaps an independently drawn overlay.
pub fn bands_intersect(top: i32, bottom: i32, band_y: u32, band_height: u32) -> bool {
    let start = i64::from(band_y);
    let end = start + i64::from(band_height);
    bottom > top && band_height > 0 && i64::from(top) < end && i64::from(bottom) > start
}

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
    /// First changed row in the RGB buffer. The GPU copies only this band.
    pub damage_y: AtomicU32,
    pub damage_height: AtomicU32,
}

impl FrameSurfaceHeader {
    /// Desktop viewport publication within the existing maximum-size mapping.
    /// The sole producer holds a mutable header while readers use the seqlock.
    pub fn publish_viewport(&mut self,pixels:&mut [u8],source:&[u8],height:u32,y:u32,rows:u32) -> bool {
        if height==0||height>FRAME_HEIGHT||pixels.len()!=FRAME_PIXEL_BYTES||source.len()!=FRAME_PIXEL_BYTES{return false;}
        let changed=self.height!=height;
        let y=if changed {0} else {y};
        let rows=if changed {height} else {rows.min(height.saturating_sub(y))};
        if rows==0{return true;}
        let odd=self.revision.load(Ordering::Relaxed).wrapping_add(1)|1;
        self.revision.store(odd,Ordering::Release);core::sync::atomic::fence(Ordering::SeqCst);
        self.width=FRAME_WIDTH;self.height=height;self.stride_bytes=FRAME_STRIDE_BYTES;self.format=FRAME_FORMAT_RGB8;
        self.damage_y.store(y,Ordering::Relaxed);self.damage_height.store(rows,Ordering::Relaxed);
        let start=y as usize*FRAME_STRIDE_BYTES as usize;let bytes=rows as usize*FRAME_STRIDE_BYTES as usize;
        unsafe {crate::frame_copy::copy_nonoverlapping_ram(pixels.as_mut_ptr().add(start),source.as_ptr().add(start),bytes);}
        self.revision.store(odd.wrapping_add(1),Ordering::Release);true
    }
    pub const fn new() -> Self {
        Self {
            revision: AtomicU32::new(0),
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            stride_bytes: FRAME_STRIDE_BYTES,
            format: FRAME_FORMAT_RGB8,
            damage_y: AtomicU32::new(0),
            damage_height: AtomicU32::new(0),
        }
    }

    pub fn publish(&self, pixels: &mut [u8], source: &[u8]) -> bool {
        self.publish_rows(pixels, source, 0, FRAME_HEIGHT)
    }

    /// Copy one horizontal band. Rows outside it stay as the previous frame.
    pub fn publish_rows(&self, pixels: &mut [u8], source: &[u8], y: u32, height: u32) -> bool {
        if pixels.len() != FRAME_PIXEL_BYTES || source.len() != FRAME_PIXEL_BYTES {
            return false;
        }
        if height == 0 || y >= FRAME_HEIGHT || height > FRAME_HEIGHT - y {
            return false;
        }
        let odd = self.revision.load(Ordering::Relaxed).wrapping_add(1) | 1;
        self.revision.store(odd, Ordering::Release);
        // Publish the in-progress marker before either damage metadata or
        // pixels can become visible to a reader on another CPU.
        core::sync::atomic::fence(Ordering::SeqCst);
        self.damage_y.store(y, Ordering::Relaxed);
        self.damage_height.store(height, Ordering::Relaxed);
        let stride = FRAME_STRIDE_BYTES as usize;
        let start = y as usize * stride;
        let bytes = height as usize * stride;
        // These slices are distinct, bounded ordinary RAM mappings. The
        // freestanding libc memcpy copies one byte at a time; use aligned
        // words for the large frame handoff instead.
        unsafe {
            crate::frame_copy::copy_nonoverlapping_ram(
                pixels.as_mut_ptr().add(start), source.as_ptr().add(start), bytes,
            );
        }
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
    fn viewport_shrink_grow_and_damage_stay_inside_the_mapping() {
        let mut header=FrameSurfaceHeader::new();
        let mut pixels=alloc::vec![0xa5;FRAME_PIXEL_BYTES];
        let mut source=alloc::vec![0x31;FRAME_PIXEL_BYTES];
        let short=FRAME_HEIGHT/2;
        assert!(header.publish_viewport(&mut pixels,&source,short,FRAME_HEIGHT,1));
        assert_eq!(header.height,short);
        assert_eq!(header.damage_y.load(Ordering::Acquire),0);
        let boundary=(short*FRAME_STRIDE_BYTES) as usize;
        assert!(pixels[..boundary].iter().all(|b|*b==0x31));
        assert!(pixels[boundary..].iter().all(|b|*b==0xa5));
        source.fill(0x72);
        assert!(header.publish_viewport(&mut pixels,&source,FRAME_HEIGHT,0,1));
        assert!(pixels.iter().all(|b|*b==0x72));
        assert_eq!(header.damage_height.load(Ordering::Acquire),FRAME_HEIGHT);
        let revision=header.revision.load(Ordering::Acquire);
        assert!(!header.publish_viewport(&mut pixels,&source,FRAME_HEIGHT+1,0,1));
        assert!(!header.publish_viewport(&mut pixels,&source,0,0,1));
        assert_eq!(header.revision.load(Ordering::Acquire),revision);
        source.fill(0x19);
        assert!(header.publish_viewport(&mut pixels,&source,FRAME_HEIGHT,FRAME_HEIGHT-1,100));
        let last=((FRAME_HEIGHT-1)*FRAME_STRIDE_BYTES) as usize;
        assert!(pixels[..last].iter().all(|b|*b==0x72));
        assert!(pixels[last..].iter().all(|b|*b==0x19));
        assert_eq!(header.damage_height.load(Ordering::Acquire),1);
    }

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
