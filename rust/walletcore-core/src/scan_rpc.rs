//! Tip-block ownership scan via monerod JSON HTTP (get_block + get_transactions).
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use monero_wallet::interface::ScannableBlock;
use monero_wallet::{
    Scanner, ViewPair, WalletOutput,
    block::Block,
    transaction::{NotPruned, Pruned, Transaction},
};

/// Max tx hashes fetched per get_transactions call (keeps TCP RX under dhcp 32KiB).
/// Cake tip batches of 4 were ~32 KB and wedged `RX_CAPACITY`; 2 stays safely under.
pub const TX_BATCH: usize = 2;
pub const STRICT_COMPACT_BATCH: usize = 8;
/// Compact range, or one member for the complete-transaction fallback.
pub fn strict_tx_request_range(batch: usize, total: usize, fallback: Option<usize>) -> Option<(usize, usize)> {
    let start = batch.checked_mul(STRICT_COMPACT_BATCH)?;
    if start >= total { return None; }
    let end = start.saturating_add(STRICT_COMPACT_BATCH).min(total);
    match fallback {
        None => Some((start, end)),
        Some(offset) => {
            let index = start.checked_add(offset)?;
            (index < end).then_some((index, index + 1))
        }
    }
}
/// Pace read-only sync requests; back off after rate limiting. Never retry writes.
pub fn scan_request_delay_ns(read_only: bool, rate_retries: u8) -> Option<u64> {
    if !read_only { return None; }
    Some(match rate_retries {
        0 => 250_000_000,
        1 => 60_000_000_000,
        2 => 120_000_000_000,
        _ => 240_000_000_000,
    })
}
/// Only transport failures are safe to retry without changing scan evidence.
pub fn transient_scan_network_error(message: &[u8]) -> bool {
    matches!(message, b"NEXAWAL | SYNC CONNECT FAIL" | b"NEXAWAL | SYNC RECV FAIL" | b"NEXAWAL | TLS TIMEOUT")
}
/// Temporary daemon or gateway failures for which a read-only request may be retried.
pub fn transient_daemon_status(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}
/// Wait for authenticated HTTP headers. Successful replies also need the complete
/// daemon body; error replies are classified from the status without parsing it.
pub fn https_rpc_response_complete(response: &[u8], success_complete: impl FnOnce(&[u8]) -> bool) -> bool {
    let headers_complete = response.windows(4).any(|bytes| bytes == b"\r\n\r\n")
        || response.windows(2).any(|bytes| bytes == b"\n\n");
    headers_complete && (http_response_status(response) != Some(200) || success_complete(response))
}
/// Count consecutive stalled TLS polls, resetting whenever ciphertext moves.
pub fn next_tls_stall_polls(current: u32, before: u64, after: u64) -> u32 {
    if before != after { 0 } else { current.saturating_add(1) }
}
/// Read the status line before interpreting a daemon response as JSON.
pub fn http_response_status(response: &[u8]) -> Option<u16> {
    if !(response.starts_with(b"HTTP/1.0 ") || response.starts_with(b"HTTP/1.1 ")) { return None; }
    let digits = response.get(9..12)?;
    if !digits.iter().all(u8::is_ascii_digit) || !matches!(response.get(12), Some(b' ' | b'\r' | b'\n')) { return None; }
    let status = u16::from(digits[0] - b'0') * 100 + u16::from(digits[1] - b'0') * 10 + u16::from(digits[2] - b'0');
    (100..=599).contains(&status).then_some(status)
}
/// Bound verified batches to the same size as the existing scan path.
pub fn strict_tx_batch_range(batch: usize, total: usize) -> Option<(usize, usize)> {
    let start = batch.checked_mul(TX_BATCH)?;
    if start >= total { return None; }
    Some((start, start.saturating_add(TX_BATCH).min(total)))
}
/// Tip-window size for single-PD path. Dense tip blocks are slow over 32 KiB
/// DHCP TCP; keep small until scan_worker owns parallel fetches.
pub const SCAN_BLOCKS: u64 = 1;

pub fn build_fee_estimate_body(out: &mut [u8]) -> Option<usize> {
    let body = br#"{"jsonrpc":"2.0","id":"0","method":"get_fee_estimate","params":{"grace_blocks":10}}"#;
    if out.len() < body.len() { return None; }
    out[..body.len()].copy_from_slice(body);
    Some(body.len())
}

/// Parse Monero's normal-priority estimate, including the legacy scalar form.
pub fn parse_normal_fee_rate(json: &[u8]) -> Option<monero_wallet::interface::FeeRate> {
    let response: serde_json::Value = serde_json::from_slice(json).ok()?;
    let result = response.get("result")?;
    if result.get("status")?.as_str()? != "OK" { return None; }
    let mask = result.get("quantization_mask")?.as_u64()?;
    let rate = match result.get("fees") {
        Some(serde_json::Value::Null) | None => result.get("fee")?.as_u64()?.checked_mul(5)?,
        Some(value) => value.as_array()?.get(1)?.as_u64()?,
    };
    monero_wallet::interface::FeeRate::new(rate, mask)
}

pub fn build_http_post(host: &str, path: &str, body: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut n = 0usize;
    macro_rules! push {
        ($bytes:expr) => {{
            let b: &[u8] = $bytes;
            if n + b.len() > out.len() {
                return None;
            }
            out[n..n + b.len()].copy_from_slice(b);
            n += b.len();
        }};
    }
    push!(b"POST ");
    push!(path.as_bytes());
    push!(b" HTTP/1.0\r\nHost: ");
    push!(host.as_bytes());
    push!(b"\r\nContent-Type: application/json\r\nContent-Length: ");
    let mut len_buf = [0u8; 10];
    let mut len = body.len();
    let mut i = len_buf.len();
    if len == 0 {
        i -= 1;
        len_buf[i] = b'0';
    } else {
        while len > 0 {
            i -= 1;
            len_buf[i] = b'0' + (len % 10) as u8;
            len /= 10;
        }
    }
    push!(&len_buf[i..]);
    push!(b"\r\nConnection: close\r\n\r\n");
    push!(body);
    Some(n)
}

/// JSON-RPC body for `get_output_histogram` (rct amount 0 pool size).
pub fn build_get_output_histogram_body(out: &mut [u8]) -> Option<usize> {
    const BODY: &[u8] = br#"{"jsonrpc":"2.0","id":"0","method":"get_output_histogram","params":{"amounts":[0],"min_count":1,"max_count":0,"unlocked":true,"recent_cutoff":0}}"#;
    if BODY.len() > out.len() {
        return None;
    }
    out[..BODY.len()].copy_from_slice(BODY);
    Some(BODY.len())
}

/// A bounded cumulative RingCT distribution request. Chunking keeps replies
/// within the guest's network and heap limits.
pub fn build_output_distribution_body(from: u64, to: u64, out: &mut [u8]) -> Option<usize> {
    if to == 0 || to < from || to.checked_sub(from)? >= 1024 { return None; }
    let mut n = 0usize;
    let prefix = br#"{"jsonrpc":"2.0","id":"0","method":"get_output_distribution","params":{"amounts":[0],"cumulative":true,"binary":false,"from_height":"#;
    if out.len() < prefix.len() { return None; }
    out[..prefix.len()].copy_from_slice(prefix); n += prefix.len();
    n += write_u64(&mut out[n..], from)?;
    let middle = br#", "to_height":"#;
    if out.len().saturating_sub(n) < middle.len() { return None; }
    out[n..n + middle.len()].copy_from_slice(middle); n += middle.len();
    n += write_u64(&mut out[n..], to)?;
    let suffix = b"}}";
    if out.len().saturating_sub(n) < suffix.len() { return None; }
    out[n..n + suffix.len()].copy_from_slice(suffix); n += suffix.len();
    Some(n)
}

/// Validate one complete cumulative distribution chunk and its boundary with
/// the prior chunk. This validates shape and monotonicity, not node honesty.
pub fn parse_output_distribution(json: &[u8], from: u64, to: u64, previous: Option<u64>) -> Option<Vec<u64>> {
    let (start, _, values) = parse_output_distribution_chunk(json, from, to, previous)?;
    if start != from { return None; }
    Some(values)
}

/// The first chunk may begin after the requested height when the node has no
/// earlier RingCT outputs. Subsequent chunks must start exactly at the request.
pub fn parse_output_distribution_chunk(json: &[u8], from: u64, to: u64, previous: Option<u64>) -> Option<(u64, u64, Vec<u64>)> {
    let requested = usize::try_from(to.checked_sub(from)?.checked_add(1)?).ok()?;
    if requested == 0 || requested > 1024 { return None; }
    let response: serde_json::Value = serde_json::from_slice(json).ok()?;
    let result = response.get("result")?;
    if result.get("status")?.as_str()? != "OK" { return None; }
    let distributions = result.get("distributions")?.as_array()?;
    if distributions.len() != 1 { return None; }
    let entry = &distributions[0];
    let start = entry.get("start_height")?.as_u64()?;
    if entry.get("amount")?.as_u64()? != 0 || start < from || (previous.is_some() && start != from)
        || entry.get("binary")?.as_bool()? || entry.get("compress")?.as_bool()? { return None; }
    let base = entry.get("base")?.as_u64()?;
    if previous.is_some_and(|value| value != base) { return None; }
    if previous.is_none() && start > from && base != 0 { return None; }
    let values = entry.get("distribution")?.as_array()?;
    let len = if start > to { 0 } else { usize::try_from(to.checked_sub(start)?.checked_add(1)?).ok()? };
    if values.len() != len { return None; }
    let mut last = base;
    let mut parsed = Vec::with_capacity(len);
    for value in values {
        let current = value.as_u64()?;
        if current < last { return None; }
        parsed.push(current); last = current;
    }
    Some((start, base, parsed))
}

/// HTTP body for `/get_outs` (absolute global index into the rct pool).
pub fn build_get_outs_body(index: u64, out: &mut [u8]) -> Option<usize> {
    build_get_outs_batch_body(core::slice::from_ref(&index), out)
}

/// Request real and decoy indices together so their request positions do not
/// disclose the real input to the node. Callers must shuffle or sort the set.
pub fn build_get_outs_batch_body(indices: &[u64], out: &mut [u8]) -> Option<usize> {
    if indices.is_empty() || indices.len() > 64 { return None; }
    for (i, index) in indices.iter().enumerate() {
        if indices[..i].contains(index) { return None; }
    }
    let mut n = 0usize;
    macro_rules! push {
        ($b:expr) => {{
            let b: &[u8] = $b;
            if n + b.len() > out.len() {
                return None;
            }
            out[n..n + b.len()].copy_from_slice(b);
            n += b.len();
        }};
    }
    push!(br#"{"outputs":["#);
    for (i, index) in indices.iter().enumerate() {
        if i != 0 { push!(b","); }
        push!(br#"{"amount":0,"index":"#);
        n += write_u64(&mut out[n..], *index)?;
        push!(b"}");
    }
    push!(br#"],"get_txid":false}"#);
    Some(n)
}

/// Parse `total_instances` from get_output_histogram result.
pub fn parse_histogram_total(json: &[u8]) -> Option<u64> {
    let key = b"\"total_instances\":";
    let rel = find_bytes(json, key)?;
    let mut i = skip_ws(json, rel + key.len());
    let start = i;
    while i < json.len() && json[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return None;
    }
    let mut v = 0u64;
    for &d in &json[start..i] {
        v = v.saturating_mul(10).saturating_add((d - b'0') as u64);
    }
    Some(v)
}

pub fn build_spent_status_body(images: &[[u8; 32]], out: &mut [u8]) -> Option<usize> {
    if images.is_empty() || images.len() > 16 { return None; }
    for (index, image) in images.iter().enumerate() {
        if images[..index].contains(image) { return None; }
    }
    let needed = 17usize.checked_add(images.len().checked_mul(66)?)?.checked_add(images.len() - 1)?;
    if out.len() < needed { return None; }
    let prefix = b"{\"key_images\":[";
    let mut n = prefix.len(); out[..n].copy_from_slice(prefix);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (index, image) in images.iter().enumerate() {
        if index != 0 { out[n] = b','; n += 1; }
        out[n] = b'"'; n += 1;
        for byte in image { out[n] = HEX[(byte >> 4) as usize]; out[n + 1] = HEX[(byte & 15) as usize]; n += 2; }
        out[n] = b'"'; n += 1;
    }
    out[n..n + 2].copy_from_slice(b"]}"); n += 2;
    Some(n)
}

/// Decode one spent status per requested key image. A daemon's pool-spent
/// status (2) must be kept distinct from a confirmed chain spend (1).
pub fn parse_spent_statuses(json: &[u8], expected: usize) -> Option<Vec<u8>> {
    if expected == 0 || expected > 16 { return None; }
    let response: serde_json::Value = serde_json::from_slice(json).ok()?;
    if response.get("status")?.as_str()? != "OK" { return None; }
    let statuses = response.get("spent_status")?.as_array()?;
    if statuses.len() != expected { return None; }
    statuses.iter().map(|status| match status.as_u64()? {
        value @ 0..=2 => Some(value as u8),
        _ => None,
    }).collect()
}

/// True when `/get_outs` returned at least one output key.
pub fn parse_get_outs_ok(json: &[u8]) -> bool {
    find_bytes(json, b"\"key\":").is_some() && find_bytes(json, b"\"status\":").is_some()
}

/// True when `/send_raw_transaction` reports acceptance.
pub fn parse_send_raw_ok(json: &[u8]) -> bool {
    let Ok(response) = serde_json::from_slice::<serde_json::Value>(json) else { return false; };
    if response.get("status").and_then(|value| value.as_str()) != Some("OK") { return false; }
    match response.get("not_relayed") {
        None => true,
        Some(value) => value.as_bool() == Some(false),
    }
}

#[cfg(test)]
mod send_response_tests {
    use super::parse_send_raw_ok;

    #[test]
    fn read_only_scan_pacing_and_rate_limit_backoff() {
        use super::scan_request_delay_ns;
        assert_eq!(scan_request_delay_ns(true, 0), Some(250_000_000));
        assert_eq!(scan_request_delay_ns(true, 1), Some(60_000_000_000));
        assert_eq!(scan_request_delay_ns(true, 2), Some(120_000_000_000));
        assert_eq!(scan_request_delay_ns(true, 20), Some(240_000_000_000));
        assert_eq!(scan_request_delay_ns(false, 1), None);
    }

    #[test]
    fn error_status_does_not_depend_on_a_json_body_or_connection_close() {
        use super::https_rpc_response_complete;
        let no_body = |_: &[u8]| panic!("incomplete or unsuccessful headers don't need a body check");
        assert!(!https_rpc_response_complete(b"HTTP/1.1 503 Busy\r\n{}", no_body));
        assert!(https_rpc_response_complete(b"HTTP/1.1 503 Busy\r\n\r\n<html>", no_body));
        assert!(https_rpc_response_complete(b"HTTP/1.1 403 Denied\r\n\r\n", no_body));
        assert!(!https_rpc_response_complete(b"HTTP/1.1 200 OK\r\n{}", no_body));
        let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert!(https_rpc_response_complete(ok, |response| response.ends_with(b"{}")));
        assert!(!https_rpc_response_complete(&ok[..ok.len()-1], |response| response.ends_with(b"{}")));
    }

    #[test]
    fn retry_only_temporary_daemon_server_statuses() {
        for code in [500, 502, 503, 504] {
            assert!(super::transient_daemon_status(code), "{code}");
        }
        for code in [200, 400, 401, 403, 404, 501, 505] {
            assert!(!super::transient_daemon_status(code), "{code}");
        }
    }

    #[test]
    fn retries_only_transient_network_failures() {
        use super::transient_scan_network_error;
        assert!(transient_scan_network_error(b"NEXAWAL | SYNC CONNECT FAIL"));
        assert!(transient_scan_network_error(b"NEXAWAL | SYNC RECV FAIL"));
        assert!(transient_scan_network_error(b"NEXAWAL | TLS TIMEOUT"));
        assert!(!transient_scan_network_error(b"NEXAWAL | TLS FAILED"));
        assert!(!transient_scan_network_error(b"NEXAWAL | SYNC OPEN FAIL | NODE NETWORK MISMATCH"));
        assert!(!transient_scan_network_error(b"NEXAWAL | SCAN BAD TXS"));
    }

    #[test]
    fn validates_pruned_mainnet_transactions_against_block_hashes() {
        let data = include_bytes!("../tests/fixtures/mainnet-3806161-pruned.json");
        let hashes = ["17ca31d9ac20cbde5f679d4ab61b9e883471f1c75c111796f2964ca1af722e51".into(), "51ed92c96e9915ae9ae504461ef78523b819bd0d17b2a8965ba70fb4f7267edb".into()];
        let compact = super::parse_tx_batch_verified(data, &hashes).unwrap();
        let full = super::parse_tx_batch_verified(include_bytes!("../tests/fixtures/mainnet-3806161-full.json"), &hashes).unwrap();
        assert_eq!(compact.len(), 2);
        for ((compact_tx, compact_indices), (full_tx, full_indices)) in compact.iter().zip(full.iter()) {
            assert_eq!(compact_tx.serialize(), full_tx.serialize());
            assert_eq!(compact_indices, full_indices);
        }
        let mut wrong = hashes.clone(); wrong[0] = "00".repeat(32);
        assert!(super::parse_tx_batch_verified(data, &wrong).is_none());
        let text = core::str::from_utf8(data).unwrap();
        let tampered = text.replace("8be855af3b5ae18b05d2d74888ba821402e2ae81053a4bbfd6a4262ffd803c36", &"00".repeat(32));
        assert!(super::parse_tx_batch_verified(tampered.as_bytes(), &hashes).is_none());
        let mut parsed: serde_json::Value = serde_json::from_slice(data).unwrap();
        parsed["txs"][0]["output_indices"] = serde_json::json!([]);
        assert!(super::parse_tx_batch_verified(&serde_json::to_vec(&parsed).unwrap(), &hashes).is_none());
        let mut parsed: serde_json::Value = serde_json::from_slice(data).unwrap();
        let pruned = parsed["txs"][0]["pruned_as_hex"].as_str().unwrap();
        let mut changed = pruned.as_bytes().to_vec();
        let last = changed.len() - 1;
        changed[last] = if changed[last] == b'0' { b'1' } else { b'0' };
        parsed["txs"][0]["pruned_as_hex"] = core::str::from_utf8(&changed).unwrap().into();
        assert!(super::parse_tx_batch_verified(&serde_json::to_vec(&parsed).unwrap(), &hashes).is_none());
        let mut body = [0; 256];
        let len = super::build_get_pruned_transactions_body(&["abc"], &mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body[..len]).unwrap();
        assert_eq!(request["prune"], true);
        assert_eq!(request["txs_hashes"][0], "abc");
    }

    #[test]
    fn tls_timeout_counts_only_consecutive_stalled_polls() {
        use super::next_tls_stall_polls;
        assert_eq!(next_tls_stall_polls(65_535, 1_000, 1_001), 0);
        assert_eq!(next_tls_stall_polls(65_535, 1_000, 1_000), 65_536);
        assert_eq!(next_tls_stall_polls(u32::MAX, 1_000, 1_000), u32::MAX);
    }

    #[test]
    fn recognizes_http_rate_limiting() {
        use super::http_response_status;
        assert_eq!(http_response_status(b"HTTP/1.1 429 Too Many Requests\r\n\r\n"), Some(429));
        assert_eq!(http_response_status(b"HTTP/1.0 200 OK\r\n\r\n"), Some(200));
        assert_eq!(http_response_status(b"HTTP/1.1 42x bad\r\n\r\n"), None);
        assert_eq!(http_response_status(b"not an HTTP response"), None);
    }

    #[test]
    fn strict_batch_ranges_cover_every_transaction() {
        use super::strict_tx_batch_range;
        assert_eq!(strict_tx_batch_range(0, 5), Some((0, 2)));
        assert_eq!(strict_tx_batch_range(1, 5), Some((2, 4)));
        assert_eq!(strict_tx_batch_range(2, 5), Some((4, 5)));
        assert_eq!(strict_tx_batch_range(3, 5), None);
        assert_eq!(strict_tx_batch_range(usize::MAX, 9), None);
    }

    #[test]
    fn compact_batches_and_fallback_cover_every_hash() {
        for total in 1..40 {
            let mut compact = alloc::vec::Vec::new();
            let mut fallback = alloc::vec::Vec::new();
            for batch in 0..(total + 7) / 8 {
                let (start, end) = super::strict_tx_request_range(batch, total, None).unwrap();
                compact.extend(start..end);
                for offset in 0..(end - start) {
                    let (a, b) = super::strict_tx_request_range(batch, total, Some(offset)).unwrap();
                    fallback.extend(a..b);
                }
                assert!(super::strict_tx_request_range(batch, total, Some(end-start)).is_none());
            }
            assert_eq!(compact, (0..total).collect::<alloc::vec::Vec<_>>());
            assert_eq!(fallback, compact);
        }
        assert!(super::strict_tx_request_range(usize::MAX, 10, None).is_none());
    }

    #[test]
    fn compact_eight_matches_verified_individual_full_transactions() {
        let hashes: alloc::vec::Vec<alloc::string::String> = serde_json::from_slice(include_bytes!("../tests/fixtures/mainnet-3806204-hashes8.json")).unwrap();
        let compact = super::parse_tx_batch_verified(include_bytes!("../tests/fixtures/mainnet-3806204-pruned8.json"), &hashes).unwrap();
        let full = super::parse_tx_batch_verified(include_bytes!("../tests/fixtures/mainnet-3806204-full8.json"), &hashes).unwrap();
        assert_eq!(compact.len(), 8);
        for ((a, ai), (b, bi)) in compact.iter().zip(full.iter()) {
            assert_eq!(a.serialize(), b.serialize());
            assert_eq!(ai, bi);
        }
        let response: serde_json::Value = serde_json::from_slice(include_bytes!("../tests/fixtures/mainnet-3806204-full8.json")).unwrap();
        for i in 0..8 {
            let single = serde_json::json!({"status":"OK", "txs":[response["txs"][i].clone()], "txs_as_hex":[response["txs_as_hex"][i].clone()]});
            let verified = super::parse_tx_batch_verified(&serde_json::to_vec(&single).unwrap(), &hashes[i..i+1]).unwrap();
            assert_eq!(verified[0].0.serialize(), compact[i].0.serialize());
            assert_eq!(verified[0].1, compact[i].1);
        }
        let mut wrong = hashes.clone(); wrong[7] = "00".repeat(32);
        assert!(super::parse_tx_batch_verified(include_bytes!("../tests/fixtures/mainnet-3806204-pruned8.json"), &wrong).is_none());
    }


    #[test]
    fn spent_status_must_match_every_requested_image() {
        use super::parse_spent_statuses;
        assert_eq!(parse_spent_statuses(br#"{"status":"OK","spent_status":[0, 1, 2]}"#, 3), Some(alloc::vec![0, 1, 2]));
        for response in [
            br#"{"status":"OK","spent_status":[0,1]}"#.as_slice(),
            br#"{"status":"OK","spent_status":[0,1,3]}"#.as_slice(),
            br#"{"status":"OK","spent_status":[0,1,2,]}"#.as_slice(),
            br#"{"status":"OK","spent_status":[0,1,2"#.as_slice(),
            br#"{"status":"Failed","spent_status":[0,1,2]}"#.as_slice(),
            br#"{"status":"OK","nested":{"spent_status":[0,1,2]}}"#.as_slice(),
            br#"{"status":"OK","spent_status":[0,1,2]} garbage"#.as_slice(),
            br#"{"status":"OK","spent_status":[0,1,"2"]}"#.as_slice(),
        ] { assert!(parse_spent_statuses(response, 3).is_none()); }
    }

    #[test]
    fn rejects_missing_wrong_or_misaligned_transactions() {
        use super::parse_tx_batch_verified;
        let tx = "02f78dae0101ffbb8dae0101e0b2d2b9c21103e6854544fbb66d55fc3546f4d3e69f8234257b69fa2237712af3b058a5f01ba14a340173f263b8a4bbc46dfb6f29e0584adbfffdf7a47c929d77c2d0c142afea2b05300211000000f7eeeb3f0e00000000000000000000";
        let hash = "373a2ace627debaf8bfd493155fd3c00c5c2fc164400ec22e79ee79a1ac487c4";
        let good = alloc::format!(r#"{{"status":"OK","txs_as_hex":["{}"],"txs":[{{"output_indices":[123]}}]}}"#, tx);
        assert!(parse_tx_batch_verified(good.as_bytes(), &[hash.into()]).is_some());
        assert!(parse_tx_batch_verified(good.as_bytes(), &["00".repeat(32)]).is_none());
        assert!(parse_tx_batch_verified(good.as_bytes(), &[hash.into(), hash.into()]).is_none());
        let bad_indices = good.replace("[123]", "[]");
        assert!(parse_tx_batch_verified(bad_indices.as_bytes(), &[hash.into()]).is_none());
        let bad_status = good.replace("\"OK\"", "\"Failed\"");
        assert!(parse_tx_batch_verified(bad_status.as_bytes(), &[hash.into()]).is_none());
        let embedded = alloc::format!(r#"{{"status":"OK","txs":[{{"as_hex":"{}","output_indices":[123]}}],"missed_tx":[]}}"#, tx);
        assert!(parse_tx_batch_verified(embedded.as_bytes(), &[hash.into()]).is_some());
        let split = alloc::format!(r#"{{"status":"OK","txs_as_hex":[""],"txs":[{{"as_hex":"","pruned_as_hex":"{}","prunable_as_hex":"{}","output_indices":[123]}}]}}"#, &tx[..100], &tx[100..]);
        assert!(parse_tx_batch_verified(split.as_bytes(), &[hash.into()]).is_some());
        let miner = alloc::format!(r#"{{"status":"OK","txs_as_hex":[""],"txs":[{{"as_hex":"","pruned_as_hex":"{}","prunable_as_hex":"","output_indices":[123]}}]}}"#, tx);
        assert!(parse_tx_batch_verified(miner.as_bytes(), &[hash.into()]).is_some());
        let missing_part = split.replace(&alloc::format!(",\"prunable_as_hex\":\"{}\"", &tx[100..]), "");
        assert!(parse_tx_batch_verified(missing_part.as_bytes(), &[hash.into()]).is_none());
        assert!(parse_tx_batch_verified(split.as_bytes(), &["00".repeat(32)]).is_none());
        let missed = embedded.replace("\"missed_tx\":[]", "\"missed_tx\":[\"missing\"]");
        assert!(parse_tx_batch_verified(missed.as_bytes(), &[hash.into()]).is_none());
        let trailing = alloc::format!("{} garbage", good);
        assert!(parse_tx_batch_verified(trailing.as_bytes(), &[hash.into()]).is_none());
    }

    #[test]
    fn accepts_split_stagenet_miner_response_and_checks_its_hash() {
        // Stagenet block 2251380: the node supplies a full miner transaction
        // as pruned_as_hex with an empty prunable part and empty legacy fields.
        let response = br#"{"status":"OK","txs_as_hex":[""],"txs":[{"as_hex":"","pruned_as_hex":"02b0b5890101fff4b489010180e0a596bb1103e6f3a20fcf822d271ab5fc5678c52c610e293fdfd6ab6723fdb4b333dbf45db4eb2101642ee03f381819ac3a93e6ae0023d8ad456a6d9dce56799a9b3130a077ac5a0a00","prunable_as_hex":"","output_indices":[10175122]}]}"#;
        let hash = "dbf369b8153efaa2319ffce5d898f58cef8020e11cde862ab90c21b6035b0887";
        assert!(super::parse_tx_batch_verified(response, &[hash.into()]).is_some());
        assert!(super::parse_tx_batch_verified(response, &["00".repeat(32)]).is_none());
    }

    #[test]
    fn accepts_only_relayed_ok_status() {
        assert!(parse_send_raw_ok(br#"{"status":"OK","not_relayed":false}"#));
        assert!(parse_send_raw_ok(br#"{"status":"OK"}"#));
        assert!(!parse_send_raw_ok(br#"{"status":"Failed","reason":"OK"}"#));
        assert!(!parse_send_raw_ok(br#"{"status":"OK","not_relayed":true}"#));
        assert!(!parse_send_raw_ok(br#"{"status":"OK","not_relayed":null}"#));
        assert!(!parse_send_raw_ok(br#"{"status":"BUSY"}"#));
        assert!(!parse_send_raw_ok(br#"{"status":"OK","not_relayed":falsehood}"#));
        assert!(!parse_send_raw_ok(br#"{"status":"OK","not_relayed":false} garbage"#));
        assert!(!parse_send_raw_ok(br#"{"nested":{"status":"OK"},"not_relayed":false}"#));
    }
}

/// Parse one unlocked output from the daemon response to a single-index request.
pub fn parse_get_outs_key_mask(json: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    parse_get_outs_batch(json, 1)?.pop().flatten()
}

/// Preserve request order and report locked outputs as unavailable. A malformed
/// entry invalidates the full response rather than shifting ring positions.
pub fn parse_get_outs_batch(json: &[u8], expected: usize) -> Option<Vec<Option<([u8; 32], [u8; 32])>>> {
    if expected == 0 || expected > 64 { return None; }
    let response: serde_json::Value = serde_json::from_slice(json).ok()?;
    if response.get("status")?.as_str()? != "OK" { return None; }
    let outs = response.get("outs")?.as_array()?;
    if outs.len() != expected { return None; }
    let mut result = Vec::with_capacity(expected);
    for output in outs {
        let output = output.as_object()?;
        let unlocked = output.get("unlocked")?.as_bool()?;
        let key_hex = output.get("key")?.as_str()?.as_bytes();
        let mask_hex = output.get("mask")?.as_str()?.as_bytes();
        let mut key = [0u8; 32];
        let mut mask = [0u8; 32];
        if hex_decode(key_hex, &mut key)? != 32 || hex_decode(mask_hex, &mut mask)? != 32 { return None; }
        result.push(unlocked.then_some((key, mask)));
    }
    Some(result)
}

#[cfg(test)]
mod output_response_tests {
    use super::{build_get_outs_batch_body, parse_get_outs_batch, parse_get_outs_key_mask, build_output_distribution_body, parse_output_distribution, parse_output_distribution_chunk, build_fee_estimate_body, parse_normal_fee_rate, build_spent_status_body};

    #[test]
    fn accepts_only_one_unlocked_output_from_successful_response() {
        let key = "11".repeat(32);
        let mask = "22".repeat(32);
        let output = alloc::format!(r#"{{"key":"{key}","mask":"{mask}","unlocked":true}}"#);
        let response = |status: &str, outs: &str| alloc::format!(r#"{{"status":"{status}","outs":[{outs}]}}"#);
        assert_eq!(parse_get_outs_key_mask(response("OK", &output).as_bytes()), Some(([0x11; 32], [0x22; 32])));
        assert!(parse_get_outs_key_mask(response("BUSY", &output).as_bytes()).is_none());
        assert!(parse_get_outs_key_mask(response("OK", &output.replace("true", "false")).as_bytes()).is_none());
        assert!(parse_get_outs_key_mask(response("OK", &output.replace(",\"unlocked\":true", "")).as_bytes()).is_none());
        assert!(parse_get_outs_key_mask(response("OK", &alloc::format!("{output},{output}")).as_bytes()).is_none());
        assert!(parse_get_outs_key_mask(response("OK", "").as_bytes()).is_none());
        assert!(parse_get_outs_key_mask(response("OK", &output.replace(&key, "12")).as_bytes()).is_none());
    }

    #[test]
    fn batch_keeps_indices_and_results_aligned() {
        let mut body = [0u8; 256];
        let n = build_get_outs_batch_body(&[4, 9], &mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body[..n]).unwrap();
        assert_eq!(request["outputs"][0]["index"], 4);
        assert_eq!(request["outputs"][1]["index"], 9);
        assert!(build_get_outs_batch_body(&[4, 4], &mut body).is_none());
        assert!(build_get_outs_batch_body(&[], &mut body).is_none());
        let key = "11".repeat(32);
        let mask = "22".repeat(32);
        let response = alloc::format!(r#"{{"status":"OK","outs":[{{"key":"{key}","mask":"{mask}","unlocked":false}},{{"key":"{key}","mask":"{mask}","unlocked":true}}]}}"#);
        assert_eq!(parse_get_outs_batch(response.as_bytes(), 2), Some(alloc::vec![None, Some(([0x11; 32], [0x22; 32]))]));
        assert!(parse_get_outs_batch(response.as_bytes(), 1).is_none());
    }

    #[test]
    fn distribution_checks_range_shape_and_chunk_boundary() {
        let mut body = [0u8; 256];
        let n = build_output_distribution_body(10, 12, &mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body[..n]).unwrap();
        assert_eq!(request["params"]["from_height"], 10);
        assert_eq!(request["params"]["to_height"], 12);
        assert!(build_output_distribution_body(0, 0, &mut body).is_none());
        assert!(build_output_distribution_body(0, 1024, &mut body).is_none());
        assert_eq!(request["params"]["binary"], false);
        let response = br#"{"result":{"status":"OK","distributions":[{"amount":0,"base":4,"binary":false,"compress":false,"start_height":10,"distribution":[5,5,8]}]}}"#;
        assert_eq!(parse_output_distribution(response, 10, 12, Some(4)), Some(alloc::vec![5, 5, 8]));
        assert!(parse_output_distribution(response, 10, 12, Some(6)).is_none());
        assert!(parse_output_distribution(response, 11, 13, None).is_none());
        assert!(parse_output_distribution(response, 10, 11, None).is_none());
        let decreasing = br#"{"result":{"status":"OK","distributions":[{"amount":0,"base":4,"binary":false,"compress":false,"start_height":10,"distribution":[5,4,8]}]}}"#;
        assert!(parse_output_distribution(decreasing, 10, 12, None).is_none());
        let delayed = br#"{"result":{"status":"OK","distributions":[{"amount":0,"base":0,"binary":false,"compress":false,"start_height":11,"distribution":[1,2]}]}}"#;
        assert_eq!(parse_output_distribution_chunk(delayed, 10, 12, None), Some((11, 0, alloc::vec![1, 2])));
        assert!(parse_output_distribution_chunk(delayed, 10, 12, Some(0)).is_none());
        assert!(parse_output_distribution(delayed, 10, 12, None).is_none());
        let empty = br#"{"result":{"status":"OK","distributions":[{"amount":0,"base":0,"binary":false,"compress":false,"start_height":20,"distribution":[]}]}}"#;
        assert_eq!(parse_output_distribution_chunk(empty, 10, 12, None), Some((20, 0, alloc::vec![])));
    }

    #[test]
    fn fee_estimate_requires_valid_normal_rate_and_mask() {
        let mut request = [0u8; 128];
        let n = build_fee_estimate_body(&mut request).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&request[..n]).unwrap();
        assert_eq!(parsed["params"]["grace_blocks"], 10);
        let modern = br#"{"result":{"status":"OK","fee":1200000,"fees":[1200000,4700000,19000000,240000000],"quantization_mask":10000}}"#;
        assert_eq!(parse_normal_fee_rate(modern).unwrap().per_weight(), 4_700_000);
        let legacy = br#"{"result":{"status":"OK","fee":12,"quantization_mask":10000}}"#;
        assert_eq!(parse_normal_fee_rate(legacy).unwrap().per_weight(), 60);
        for bad in [
            br#"{"result":{"status":"BUSY","fee":12,"quantization_mask":1}}"#.as_slice(),
            br#"{"result":{"status":"OK","fees":[1],"quantization_mask":1}}"#.as_slice(),
            br#"{"result":{"status":"OK","fee":0,"quantization_mask":1}}"#.as_slice(),
            br#"{"result":{"status":"OK","fee":1,"quantization_mask":0}}"#.as_slice(),
            br#"{"result":{"status":"OK","fee":18446744073709551615,"quantization_mask":1}}"#.as_slice(),
        ] { assert!(parse_normal_fee_rate(bad).is_none()); }
    }

    #[test]
    fn spent_request_keeps_key_image_order_and_rejects_duplicates() {
        let mut body = [0u8; 1200];
        let n = build_spent_status_body(&[[1; 32], [2; 32]], &mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body[..n]).unwrap();
        assert_eq!(request["key_images"][0], "01".repeat(32));
        assert_eq!(request["key_images"][1], "02".repeat(32));
        assert!(build_spent_status_body(&[[1; 32], [1; 32]], &mut body).is_none());
        assert!(build_spent_status_body(&[], &mut body).is_none());
        assert!(build_spent_status_body(&[[3; 32]], &mut body[..20]).is_none());
    }
}

pub fn build_get_block_body(height: u64, out: &mut [u8]) -> Option<usize> {
    // {"jsonrpc":"2.0","id":"0","method":"get_block","params":{"height":N}}
    let mut n = 0usize;
    macro_rules! push {
        ($b:expr) => {{
            let b: &[u8] = $b;
            if n + b.len() > out.len() {
                return None;
            }
            out[n..n + b.len()].copy_from_slice(b);
            n += b.len();
        }};
    }
    push!(br#"{"jsonrpc":"2.0","id":"0","method":"get_block","params":{"height":"#);
    n += write_u64(&mut out[n..], height)?;
    push!(br#"}}"#);
    Some(n)
}

pub fn build_get_transactions_body(hashes_hex: &[&str], out: &mut [u8]) -> Option<usize> {
    let mut n = 0usize;
    macro_rules! push {
        ($b:expr) => {{
            let b: &[u8] = $b;
            if n + b.len() > out.len() {
                return None;
            }
            out[n..n + b.len()].copy_from_slice(b);
            n += b.len();
        }};
    }
    push!(br#"{"txs_hashes":["#);
    for (i, h) in hashes_hex.iter().enumerate() {
        if i > 0 {
            push!(b",");
        }
        push!(b"\"");
        push!(h.as_bytes());
        push!(b"\"");
    }
    push!(br#"],"decode_as_json":false}"#);
    Some(n)
}

/// Request compact transactions together with their prunable hashes.
pub fn build_get_pruned_transactions_body(hashes_hex: &[&str], out: &mut [u8]) -> Option<usize> {
    let n = build_get_transactions_body(hashes_hex, out)?;
    let suffix = br#", "prune":true}"#;
    let start = n.checked_sub(1)?;
    if start.checked_add(suffix.len())? > out.len() { return None; }
    out[start..start + suffix.len()].copy_from_slice(suffix);
    Some(start + suffix.len())
}

fn write_u64(out: &mut [u8], mut value: u64) -> Option<usize> {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    if value == 0 {
        i -= 1;
        digits[i] = b'0';
    } else {
        while value > 0 {
            i -= 1;
            digits[i] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    let dig = &digits[i..];
    if dig.len() > out.len() {
        return None;
    }
    out[..dig.len()].copy_from_slice(dig);
    Some(dig.len())
}

pub fn hex_decode(hex: &[u8], out: &mut [u8]) -> Option<usize> {
    if hex.len() % 2 != 0 || hex.len() / 2 > out.len() {
        return None;
    }
    let mut n = 0;
    let mut i = 0;
    while i < hex.len() {
        let hi = from_hex(hex[i])?;
        let lo = from_hex(hex[i + 1])?;
        out[n] = (hi << 4) | lo;
        n += 1;
        i += 2;
    }
    Some(n)
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn skip_ws(json: &[u8], mut i: usize) -> usize {
    while i < json.len() && matches!(json[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Find `"key": "` / `"key":"` and return the string value bytes (hex fields; no escapes).
pub fn json_string_value<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut pat = Vec::with_capacity(key.len() + 2);
    pat.extend_from_slice(b"\"");
    pat.extend_from_slice(key);
    pat.extend_from_slice(b"\":");
    let mut i = find_bytes(json, &pat)? + pat.len();
    i = skip_ws(json, i);
    if i >= json.len() || json[i] != b'"' {
        return None;
    }
    i += 1;
    let start = i;
    while i < json.len() && json[i] != b'"' {
        i += 1;
    }
    if i >= json.len() {
        return None;
    }
    Some(&json[start..i])
}

pub fn json_string_array(json: &[u8], key: &[u8]) -> Option<Vec<String>> {
    let mut pat = Vec::with_capacity(key.len() + 2);
    pat.extend_from_slice(b"\"");
    pat.extend_from_slice(key);
    pat.extend_from_slice(b"\":");
    let mut i = find_bytes(json, &pat)? + pat.len();
    i = skip_ws(json, i);
    if i >= json.len() || json[i] != b'[' {
        return None;
    }
    i += 1;
    let mut out = Vec::new();
    loop {
        i = skip_ws(json, i);
        while i < json.len() && json[i] == b',' {
            i = skip_ws(json, i + 1);
        }
        if i >= json.len() {
            return None;
        }
        if json[i] == b']' {
            return Some(out);
        }
        if json[i] != b'"' {
            return None;
        }
        i += 1;
        let start = i;
        while i < json.len() && json[i] != b'"' {
            i += 1;
        }
        if i >= json.len() {
            return None;
        }
        out.push(String::from_utf8_lossy(&json[start..i]).into_owned());
        i += 1;
    }
}

/// Collect `"output_indices": [n,...]` / `"output_indices":[n,...]` arrays in order.
pub fn json_output_index_lists(json: &[u8]) -> Vec<Vec<u64>> {
    let mut lists = Vec::new();
    let key = b"\"output_indices\":";
    let mut search_from = 0usize;
    while let Some(rel) = find_bytes(&json[search_from..], key) {
        let abs = search_from + rel;
        let mut i = skip_ws(json, abs + key.len());
        if i >= json.len() || json[i] != b'[' {
            search_from = abs + key.len();
            continue;
        }
        i += 1;
        let mut nums = Vec::new();
        let mut closed = false;
        loop {
            i = skip_ws(json, i);
            while i < json.len() && json[i] == b',' {
                i = skip_ws(json, i + 1);
            }
            if i >= json.len() {
                break;
            }
            if json[i] == b']' {
                lists.push(nums);
                search_from = i + 1;
                closed = true;
                break;
            }
            let start = i;
            while i < json.len() && json[i].is_ascii_digit() {
                i += 1;
            }
            if i == start {
                break;
            }
            let mut v = 0u64;
            for &d in &json[start..i] {
                v = v.saturating_mul(10).saturating_add((d - b'0') as u64);
            }
            nums.push(v);
        }
        if !closed {
            break;
        }
    }
    lists
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    for i in 0..=(haystack.len() - needle.len()) {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

pub fn decode_block_blob(blob_hex: &[u8]) -> Option<Block> {
    let mut raw = vec![0u8; blob_hex.len() / 2];
    let n = hex_decode(blob_hex, &mut raw)?;
    let mut slice: &[u8] = &raw[..n];
    let block = Block::read(&mut slice).ok()?;
    slice.is_empty().then_some(block)
}

pub fn decode_tx_hex(tx_hex: &[u8]) -> Option<Transaction<Pruned>> {
    let mut raw = vec![0u8; tx_hex.len() / 2];
    let n = hex_decode(tx_hex, &mut raw)?;
    let mut slice: &[u8] = &raw[..n];
    let full: Transaction<NotPruned> = Transaction::read(&mut slice).ok()?;
    Some(Transaction::<Pruned>::from(full))
}

/// Pair as_hex blobs with output_indices lists from a get_transactions body.
pub fn parse_tx_batch(json: &[u8]) -> Option<Vec<(Transaction<Pruned>, Vec<u64>)>> {
    // Prefer txs[].as_hex; also accept top-level txs_as_hex array.
    // Cake / monerod often emit `"as_hex": "` (space after colon).
    let mut hexes: Vec<String> = Vec::new();
    let mut search = 0usize;
    let key = b"\"as_hex\":";
    while let Some(rel) = find_bytes(&json[search..], key) {
        let mut i = search + rel + key.len();
        i = skip_ws(json, i);
        if i >= json.len() || json[i] != b'"' {
            search = search + rel + key.len();
            continue;
        }
        i += 1;
        let start = i;
        while i < json.len() && json[i] != b'"' {
            i += 1;
        }
        if i >= json.len() {
            break;
        }
        // Skip empty as_hex (pruned-only entries use pruned_as_hex).
        if i > start {
            hexes.push(String::from_utf8_lossy(&json[start..i]).into_owned());
        }
        search = i + 1;
    }
    // Prefer aligned top-level txs_as_hex (Cake emits spaces; as_hex may be empty for miner).
    if let Some(arr) = json_string_array(json, b"txs_as_hex") {
        if !arr.is_empty() {
            hexes = arr;
        }
    }
    let indices = json_output_index_lists(json);
    if hexes.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::with_capacity(hexes.len());
    for (i, hx) in hexes.iter().enumerate() {
        let tx = decode_tx_hex(hx.as_bytes())?;
        let idx = indices.get(i).cloned().unwrap_or_default();
        out.push((tx, idx));
    }
    Some(out)
}

/// Decode a complete daemon batch and bind each transaction to the hash
/// requested from the block. Never advance past missing or reordered entries.
pub fn parse_tx_batch_verified(json: &[u8], expected: &[String]) -> Option<Vec<(Transaction<Pruned>, Vec<u64>)>> {
    let response: serde_json::Value = serde_json::from_slice(json).ok()?;
    if response.get("status")?.as_str()? != "OK" { return None; }
    if let Some(missed) = response.get("missed_tx") {
        if !missed.as_array()?.is_empty() { return None; }
    }
    let txs = response.get("txs")?.as_array()?;
    if txs.len() != expected.len() { return None; }
    let hexes = match response.get("txs_as_hex") {
        Some(value) if !value.as_array()?.is_empty() => {
            let items = value.as_array()?;
            if items.len() != expected.len() { return None; }
            Some(items)
        }
        _ => None,
    };
    let mut out = Vec::with_capacity(expected.len());
    for (index, (entry, hash)) in txs.iter().zip(expected).enumerate() {
        let indices: Vec<u64> = entry.get("output_indices")?.as_array()?.iter()
            .map(|value| value.as_u64()).collect::<Option<Vec<_>>>()?;
        // Some nodes return an empty legacy `txs_as_hex`/`as_hex` while
        // providing the complete transaction in the two split fields. Never
        // accept a pruned transaction by itself: concatenate both fields and
        // still verify the decoded full transaction against the requested hash.
        let top_hex = match hexes { Some(items) => Some(items[index].as_str()?), None => None };
        let entry_hex = match entry.get("as_hex") { Some(value) => Some(value.as_str()?), None => None };
        if top_hex.unwrap_or("").is_empty() && entry_hex.unwrap_or("").is_empty()
            && entry.get("prunable_as_hex").and_then(|v| v.as_str()) == Some("")
        {
            if let (Some(pruned), Some(prunable_hash)) = (
                entry.get("pruned_as_hex").and_then(|v| v.as_str()),
                entry.get("prunable_hash").and_then(|v| v.as_str()).and_then(|s| crate::decode_hex32(s.as_bytes())),
            ) {
                let mut bytes = vec![0u8; pruned.len() / 2];
                let len = hex_decode(pruned.as_bytes(), &mut bytes)?;
                let mut input = &bytes[..len];
                let tx = Transaction::<Pruned>::read(&mut input).ok()?;
                let expected_hash = crate::decode_hex32(hash.as_bytes())?;
                if !input.is_empty() || tx.hash_with_prunable_hash(prunable_hash) != Some(expected_hash)
                    || indices.len() != tx.prefix().outputs.len()
                    || indices.windows(2).any(|pair| pair[0].checked_add(1) != Some(pair[1])) {
                    return None;
                }
                out.push((tx, indices));
                continue;
            }
        }
        let mut split_hex = String::new();
        let hex = if let Some(hex) = top_hex.filter(|s| !s.is_empty()).or(entry_hex.filter(|s| !s.is_empty())) {
            hex
        } else {
            let pruned = entry.get("pruned_as_hex")?.as_str()?;
            let prunable = entry.get("prunable_as_hex")?.as_str()?;
            if pruned.is_empty() { return None; }
            split_hex.push_str(pruned);
            split_hex.push_str(prunable);
            split_hex.as_str()
        };
        let mut bytes = vec![0u8; hex.len() / 2];
        let len = hex_decode(hex.as_bytes(), &mut bytes)?;
        let mut input = &bytes[..len];
        let full = Transaction::<NotPruned>::read(&mut input).ok()?;
        if !input.is_empty() || Some(full.hash()) != crate::decode_hex32(hash.as_bytes())
            || indices.len() != full.prefix().outputs.len()
            || (matches!(full, Transaction::V2 { .. }) && indices.windows(2).any(|pair| pair[0].checked_add(1) != Some(pair[1]))) {
            return None;
        }
        out.push((Transaction::<Pruned>::from(full), indices));
    }
    Some(out)
}

/// Require a complete block; the permissive fixture scanner intentionally
/// tolerates missing transactions, but real wallet recovery must never do so.
pub fn scan_assembled_strict(
    view: &ViewPair, block: Block, txs: Vec<Transaction<Pruned>>, first_rct: Option<u64>,
) -> Result<Vec<WalletOutput>, ()> {
    if block.transactions.len() != txs.len() { return Err(()); }
    let has_ringct = matches!(block.miner_transaction(), Transaction::V2 { .. }) && !block.miner_transaction().prefix().outputs.is_empty()
        || txs.iter().any(|tx| matches!(tx, Transaction::V2 { .. }) && !tx.prefix().outputs.is_empty());
    if has_ringct != first_rct.is_some() { return Err(()); }
    scan_assembled(view, block, txs, first_rct)
}

pub fn scan_assembled(
    view: &ViewPair,
    mut block: Block,
    mut txs: Vec<Transaction<Pruned>>,
    first_rct: Option<u64>,
) -> Result<Vec<WalletOutput>, ()> {
    // tip768: tip hang-skip can leave fewer fetched peers than tx_hashes.
    // Scan what we have (truncate the longer side) instead of Err→TIP_SKIP
    // which dropped H3803776 after 17/21 TX_OK.
    if block.transactions.len() != txs.len() {
        let n = block.transactions.len().min(txs.len());
        block.transactions.truncate(n);
        txs.truncate(n);
    }
    let mut scanner = Scanner::new(view.clone());
    let scannable = ScannableBlock {
        block,
        transactions: txs,
        output_index_for_first_ringct_output: first_rct,
    };
    let locked = scanner.scan(scannable).map_err(|_| ())?;
    Ok(locked.ignore_additional_timelock())
}

pub fn outputs_balance(outputs: &[WalletOutput]) -> u64 {
    outputs.iter().fold(0u64, |acc, o| acc.saturating_add(o.commitment().amount))
}
