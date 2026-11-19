//! Exact parsers for the small GLINK control records used by Sunfish probes.
pub const RX_DONE_W_REUSE: u16 = 14;
pub const SERVREG_CHANNEL: u16 = 2;
pub const PDR_REPLY_INTENT: u32 = 2;
pub const RX_DONE_BYTES: usize = 8;

/// Accept only the captured RX_DONE_W_REUSE for the PDR reply's intent.
/// Additional bytes belong to the next record and must be checked separately.
pub fn parse_pdr_reply_done(bytes: &[u8]) -> Option<u32> {
    if bytes.len() != RX_DONE_BYTES
        || u16::from_le_bytes([bytes[0], bytes[1]]) != RX_DONE_W_REUSE
        || u16::from_le_bytes([bytes[2], bytes[3]]) != SERVREG_CHANNEL
    {
        return None;
    }
    let intent = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    (intent == PDR_REPLY_INTENT).then_some(intent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_exact_pdr_reply_done_record() {
        let exact = [14, 0, 2, 0, 2, 0, 0, 0];
        assert_eq!(parse_pdr_reply_done(&exact), Some(2));

        let mut wrong = exact;
        wrong[0] = 6;
        assert_eq!(parse_pdr_reply_done(&wrong), None);
        let mut wrong = exact;
        wrong[2] = 1;
        assert_eq!(parse_pdr_reply_done(&wrong), None);
        let mut wrong = exact;
        wrong[4] = 3;
        assert_eq!(parse_pdr_reply_done(&wrong), None);
        assert_eq!(parse_pdr_reply_done(&exact[..7]), None);

        let mut extra = [0; 9];
        extra[..8].copy_from_slice(&exact);
        assert_eq!(parse_pdr_reply_done(&extra), None);
    }
}
