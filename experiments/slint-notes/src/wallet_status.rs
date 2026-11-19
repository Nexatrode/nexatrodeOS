/// Accept the default HTTPS node in its user-facing forms.
pub fn wallet_node_authority(input: &str) -> &str {
    match input.trim() {
        "rpc.nexawal.com" | "https://rpc.nexawal.com" => "rpc.nexawal.com:443",
        other => other,
    }
}

/// The wallet uses `+ SCAN <height>` for work in progress. A completed
/// fixture scan uses `| SCAN <count>`; a general scan publishes `SCAN OK`.
pub fn scan_complete(status: &str) -> bool {
    status.contains("SCAN OK")
        || status.split("| SCAN ").nth(1).is_some_and(|rest| {
            rest.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit())
        })
}

/// An empty balance is only meaningful after the scan has completed.
pub fn waiting_for_deposit(status: &str) -> bool {
    status.contains("NEED DEPOSIT") && scan_complete(status)
}

/// The daemon height is the next block height; SCAN is the block being read.
pub fn scan_progress(status: &str) -> Option<(u64, u64)> {
    fn number_after(text: &str, marker: &str) -> Option<u64> {
        let value = text.split_once(marker)?.1;
        let digits = value.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 { return None; }
        value[..digits].parse().ok()
    }
    let tip = number_after(status, "| TIP ")?;
    let current = number_after(status, " + SCAN ")?;
    (tip > 0 && current <= tip).then_some((current, tip))
}

#[cfg(test)]
mod tests {
    use super::{scan_complete, scan_progress, waiting_for_deposit};

    #[test]
    fn nexawal_node_accepts_bare_host_or_https_url() {
        assert_eq!(super::wallet_node_authority("rpc.nexawal.com"), "rpc.nexawal.com:443");
        assert_eq!(super::wallet_node_authority("https://rpc.nexawal.com"), "rpc.nexawal.com:443");
        assert_eq!(super::wallet_node_authority("node.example:18081"), "node.example:18081");
    }

    #[test]
    fn extracts_scanning_heights_without_mistaking_completion() {
        assert_eq!(scan_progress("NEXAWAL | TIP 3805916 + SCAN 3805816 RX"), Some((3805816, 3805916)));
        assert_eq!(scan_progress("NEXAWAL | TIP 12 + SCAN 13"), None);
        assert_eq!(scan_progress("NEXAWAL | TIP x + SCAN 12"), None);
        assert_eq!(scan_progress("NEXAWAL | SCAN OK"), None);
    }

    #[test]
    fn progress_and_unscanned_balances_are_not_complete() {
        for status in [
            "NEXAWAL | TIP 2251375 + SCAN 0",
            "NEXAWAL | TIP 2251375 + SCAN 1200",
            "NEXAWAL | OPEN wallet | BAL 0",
            "NEXAWAL | SCAN FAIL | LEGACY OUTPUTS UNSUPPORTED",
        ] {
            assert!(!scan_complete(status), "{status}");
        }
        for status in [
            "NEXAWAL | OPEN wallet | BAL 0 | SCAN 0",
            "NEXAWAL | OPEN wallet | BAL 10 | SCAN 2",
            "NEXAWAL | SCAN OK | SEND NOT READY",
        ] {
            assert!(scan_complete(status), "{status}");
        }
    }

    #[test]
    fn deposit_prompt_requires_completed_scan() {
        assert!(!waiting_for_deposit("NEXAWAL | CONNECTING | NEED DEPOSIT | BAL 0"));
        assert!(!waiting_for_deposit("NEXAWAL | TIP 2251400 + SCAN 2251400 | NEED DEPOSIT"));
        assert!(waiting_for_deposit("NEXAWAL | SCAN OK | NEED DEPOSIT | BAL 0"));
    }
}
