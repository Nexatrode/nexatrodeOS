//! Bounded mailbox format shared by the UI and the photo worker.
use crate::photo_mailbox::Mailbox;

pub const MAX_JPEG: usize = 1024 * 1024;
pub const MAX_RGBA: usize = 480 * 480 * 4;
pub const HEADER: usize = 8;
pub type PhotoMailbox = Mailbox<MAX_JPEG, { HEADER + MAX_RGBA }>;
const _: () = assert!(core::mem::size_of::<PhotoMailbox>() <= 0x200000);

pub fn encode_preview(jpeg: &[u8], output: &mut [u8]) -> Option<usize> {
    let (width, height, rgba) = crate::photo_preview::decode_jpeg_rgba(jpeg)?;
    let total = HEADER.checked_add(rgba.len())?;
    if total > output.len() {
        return None;
    }
    output[..4].copy_from_slice(&(width as u32).to_le_bytes());
    output[4..8].copy_from_slice(&(height as u32).to_le_bytes());
    output[8..total].copy_from_slice(&rgba);
    Some(total)
}

pub fn preview_parts(data: &[u8]) -> Option<(usize, usize, &[u8])> {
    let width = u32::from_le_bytes(data.get(..4)?.try_into().ok()?) as usize;
    let height = u32::from_le_bytes(data.get(4..8)?.try_into().ok()?) as usize;
    if width == 0 || height == 0 || width > 480 || height > 480 {
        return None;
    }
    let bytes = width.checked_mul(height)?.checked_mul(4)?;
    let rgba = data.get(8..)?;
    (rgba.len() == bytes).then_some((width, height, rgba))
}

/// A completion may update its model after leaving the screen, but never a
/// different chat or a replaced transcript.
pub fn preview_matches(
    stamp: u64,
    chat: u64,
    active_stamp: u64,
    active_chat: u64,
    loader_matches: bool,
) -> bool {
    loader_matches && stamp == active_stamp && chat == active_chat
}
