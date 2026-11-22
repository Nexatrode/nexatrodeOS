//! Counting incoming lines while retaining the chat's read position.

pub fn pending_before_baseline(optimistic: &str, reading: bool) -> usize {
    if reading { 0 } else { unread_incoming(optimistic, 0) }
}

pub fn unread_incoming(transcript: &str, seen: usize) -> usize {
    let bytes = transcript.as_bytes();
    let seen = seen.min(bytes.len());
    let mut start = if seen == 0 || bytes.get(seen.saturating_sub(1)) == Some(&b'\n') {
        seen
    } else {
        match bytes[seen..].iter().position(|byte| *byte == b'\n') {
            Some(offset) => seen + offset + 1,
            None => bytes.len(),
        }
    };
    let mut count = 0usize;
    while start < bytes.len() {
        let end = bytes[start..].iter().position(|byte| *byte == b'\n')
            .map(|offset| start + offset).unwrap_or(bytes.len());
        let line = &bytes[start..end];
        if !line.is_empty() && !line.starts_with(b"NexatrodeOS: ") { count += 1; }
        if end == bytes.len() { break; }
        start = end + 1;
    }
    count
}
