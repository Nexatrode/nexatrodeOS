/// Prevent acknowledgement until the UI accepts the worker's saved event.
pub struct ReceiveHandoff { owner: Option<u64> }

impl ReceiveHandoff {
    pub const fn new() -> Self { Self { owner: None } }

    pub fn hold(&mut self, owner: u64) -> bool {
        if owner == 0 || self.owner.is_some() { return false; }
        self.owner = Some(owner);
        true
    }

    pub fn retry_owner(&self) -> Option<u64> { self.owner }
    pub fn may_auto_ack(&self) -> bool { self.owner.is_none() }

    pub fn accept(&mut self, owner: u64) -> bool {
        if owner == 0 || self.owner != Some(owner) { return false; }
        self.owner = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::ReceiveHandoff;

    #[test]
    fn holds_ack_until_matching_chat_accepts_and_allows_retry() {
        let mut handoff = ReceiveHandoff::new();
        assert!(handoff.may_auto_ack());
        assert!(!handoff.hold(0));
        assert!(handoff.hold(91));
        assert!(!handoff.hold(42));
        assert!(!handoff.may_auto_ack());
        assert_eq!(handoff.retry_owner(), Some(91));
        assert!(!handoff.accept(7));
        assert_eq!(handoff.retry_owner(), Some(91));
        assert!(handoff.accept(91));
        assert!(handoff.may_auto_ack());
        assert!(!handoff.accept(91));
    }
}
