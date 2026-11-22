//! Bounds and assembles a photo read from individually authenticated chunks.
pub const CHUNK: usize = 3072;
pub const MAX_PHOTO: usize = 1024 * 1024;

pub fn read_photo<F>(owner: u64, id: u64, output: &mut [u8], mut read: F) -> Option<usize>
where
    F: FnMut(u64, u64, usize, &mut [u8]) -> Option<(usize, usize)>,
{
    if owner == 0 || owner == u64::MAX || id == 0 || output.is_empty() {
        return None;
    }
    let mut total = None;
    for part in 0..MAX_PHOTO.div_ceil(CHUNK) {
        let offset = part.checked_mul(CHUNK)?;
        if offset >= output.len() {
            return None;
        }
        let end = output.len().min(offset + CHUNK);
        let (reported, count) = read(owner, id, part, &mut output[offset..end])?;
        if !(1..=MAX_PHOTO.min(output.len())).contains(&reported)
            || total.is_some_and(|value| value != reported)
            || offset >= reported
            || count != (reported - offset).min(CHUNK)
        {
            return None;
        }
        total = Some(reported);
        if offset + count == reported {
            return Some(reported);
        }
    }
    None
}
