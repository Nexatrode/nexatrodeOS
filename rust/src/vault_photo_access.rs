//! Authorization for the dedicated read-only preview channel.
pub fn permits(id: u64, expected_owner: u64, actual_owner: u64) -> bool {
    id != 0
        && expected_owner != 0
        && expected_owner != u64::MAX
        && (actual_owner == expected_owner || actual_owner == u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::permits;
    #[test]
    fn requires_a_real_photo_and_matching_chat_or_library_owner() {
        assert!(permits(12, 34, 34));
        assert!(permits(12, 34, u64::MAX));
        assert!(!permits(0, 34, 34));
        assert!(!permits(12, 0, 0));
        assert!(!permits(12, u64::MAX, u64::MAX));
        assert!(!permits(12, 34, 35));
    }
}
