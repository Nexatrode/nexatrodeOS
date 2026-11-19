//! Opaque identity for verified WLFW board data.
//!
//! This module deliberately has no production constructor yet. The observed
//! Sunfish board-file hash identifies bytes, but the active WLFW board ID has
//! not been captured and tied to those bytes. Only tests can mint fixtures;
//! production construction must come from the later device/source verifier.
#![forbid(unsafe_code)]
#![allow(dead_code)]

pub const MAX_BDF_IMAGE_BYTES: usize = 1024 * 1024;

/// A board-data source matched to an exact WLFW capability and device identity.
/// Private fields prevent transport code from accepting arbitrary byte slices.
pub struct VerifiedBdfImage<'a> {
    bytes: &'a [u8],
    board_id: u32,
    generation: u64,
    identity: u64,
    asset_id: u64,
}

impl VerifiedBdfImage<'_> {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self.bytes
    }

    pub(crate) fn board_id(&self) -> u32 {
        self.board_id
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) fn asset_id(&self) -> u64 {
        self.asset_id
    }
}

#[cfg(test)]
impl<'a> VerifiedBdfImage<'a> {
    pub(crate) fn test_fixture(
        bytes: &'a [u8],
        board_id: u32,
        generation: u64,
        identity: u64,
        asset_id: u64,
    ) -> Self {
        Self {
            bytes,
            board_id,
            generation,
            identity,
            asset_id,
        }
    }
}
