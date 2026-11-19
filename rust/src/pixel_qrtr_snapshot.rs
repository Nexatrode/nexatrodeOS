//! Byte boundaries for one bounded QRTR RX diagnostic snapshot.
//! The known 72-byte frame is separated from, but does not interpret, the
//! following 16 bytes.

pub const FRAME_PREFIX_BYTES: usize = 72;
pub const OPAQUE_SUFFIX_BYTES: usize = 16;
pub const SNAPSHOT_BYTES: usize = FRAME_PREFIX_BYTES + OPAQUE_SUFFIX_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Split {
    pub prefix_bytes: usize,
    pub opaque_suffix_bytes: usize,
}

pub fn split_snapshot(bytes: &[u8], captured: usize) -> Split {
    let bounded = captured.min(bytes.len()).min(SNAPSHOT_BYTES);
    Split {
        prefix_bytes: bounded.min(FRAME_PREFIX_BYTES),
        opaque_suffix_bytes: bounded
            .saturating_sub(FRAME_PREFIX_BYTES)
            .min(OPAQUE_SUFFIX_BYTES),
    }
}
