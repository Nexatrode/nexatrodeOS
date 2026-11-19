//! Shared evidence interface between a WPA supplicant and radio key trackers.
#![forbid(unsafe_code)]

/// A generation-bound tracker may authorize the supplicant's protected-data
/// phase only after every required key is confirmed for that association.
pub trait KeyInstallConfirmation {
    fn confirms_keys_for_association(
        &self,
        generation: u32,
        peer_address: [u8; 6],
        pairwise_key_index: u8,
        group_key_index: Option<u8>,
    ) -> bool;
}
