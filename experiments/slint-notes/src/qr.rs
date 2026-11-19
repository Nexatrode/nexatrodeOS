//! White-on-black QR bitmaps for Slint. Used by the UTM guest and the host preview.
extern crate alloc;

use alloc::vec;

/// Encode UTF-8 as a QR image. Empty text and strings over 2500 bytes are rejected.
pub fn qr_image(text: &str) -> Option<slint::Image> {
    use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
    if text.is_empty() || text.len() > 2500 {
        return None;
    }
    // Heap buffers — Version::MAX needs ~4 KiB each; keep them off the guest stack.
    let mut outbuffer = vec![0u8; Version::MAX.buffer_len()];
    let mut tempbuffer = vec![0u8; Version::MAX.buffer_len()];
    let qr = QrCode::encode_text(
        text,
        &mut tempbuffer,
        &mut outbuffer,
        QrCodeEcc::Low,
        Version::MIN,
        Version::MAX,
        None,
        true,
    )
    .ok()?;
    let modules = qr.size() as usize;
    // Quiet zone of 2 modules on each side; scale so the bitmap stays ~152px.
    let quiet = 2usize;
    let total = modules + quiet * 2;
    let scale = (152 / total).max(2);
    let pixels_side = total * scale;
    let mut buffer =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(pixels_side as u32, pixels_side as u32);
    let bytes = buffer.make_mut_bytes();
    for y in 0..pixels_side {
        for x in 0..pixels_side {
            let mx = x / scale;
            let my = y / scale;
            let dark = mx >= quiet
                && my >= quiet
                && mx < quiet + modules
                && my < quiet + modules
                && qr.get_module((mx - quiet) as i32, (my - quiet) as i32);
            let offset = (y * pixels_side + x) * 4;
            let value = if dark { 0u8 } else { 255u8 };
            bytes[offset] = value;
            bytes[offset + 1] = value;
            bytes[offset + 2] = value;
            bytes[offset + 3] = 255;
        }
    }
    Some(slint::Image::from_rgba8(buffer))
}
