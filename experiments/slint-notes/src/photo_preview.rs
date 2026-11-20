//! Bounded decoding for image previews.
extern crate alloc;
use alloc::{vec, vec::Vec};

pub fn decode_jpeg_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let options = zune_jpeg::zune_core::options::DecoderOptions::default()
        .jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::RGBA)
        .set_max_width(2560)
        .set_max_height(2560);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(
        zune_jpeg::zune_core::bytestream::ZCursor::new(bytes),
        options,
    );
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    let width = usize::from(info.width);
    let height = usize::from(info.height);
    if width == 0 || height == 0 || width.checked_mul(height)? > 3_500_000 {
        return None;
    }
    let pixels = decoder.decode().ok()?;
    if pixels.len() != width * height * 4 {
        return None;
    }
    let longest = width.max(height);
    if longest <= 480 {
        return Some((width, height, pixels));
    }
    let out_width = (width * 480 / longest).max(1);
    let out_height = (height * 480 / longest).max(1);
    let mut scaled = vec![0u8; out_width * out_height * 4];
    for y in 0..out_height {
        for x in 0..out_width {
            let from = ((y * height / out_height) * width + x * width / out_width) * 4;
            let to = (y * out_width + x) * 4;
            scaled[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
        }
    }
    Some((out_width, out_height, scaled))
}
