extern crate alloc;
#[path = "../src/frame.rs"]
mod frame;

#[test]
fn repainting_a_title_or_badge_band_requires_restoring_its_overlay() {
    assert!(frame::bands_intersect(0, 2340, 133, 100));
    assert!(frame::bands_intersect(120, 134, 133, 100));
    assert!(!frame::bands_intersect(0, 133, 133, 100));
    assert!(!frame::bands_intersect(233, 400, 133, 100));
    assert!(!frame::bands_intersect(0, 0, 133, 100));
}
