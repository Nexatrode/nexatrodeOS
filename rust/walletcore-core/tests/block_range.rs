use walletcore_core::{
    MAX_RANGE_BLOCKS, MAX_RANGE_BODY, decode_block_range, encode_blocks_request,
};

const START: u64 = 3_806_161;
const MODERN: &[u8] = include_bytes!("fixtures/mainnet-3806161-range2.bin");
const EARLY: &[u8] = include_bytes!("fixtures/mainnet-1-range2.bin");
const V1: &[u8] = include_bytes!("fixtures/mainnet-100000-range1.bin");

fn offset(bytes: &[u8], needle: &[u8], occurrence: usize) -> usize {
    bytes
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle)
        .nth(occurrence)
        .unwrap()
        .0
}

fn streamed(
    bytes: &[u8],
    step: usize,
) -> (
    usize,
    Result<Vec<walletcore_core::RangeBlock>, walletcore_core::RangeError>,
) {
    let mut body = walletcore_core::RangeBody::new(START, 2);
    for fragment in bytes.chunks(step) {
        body.append(fragment).unwrap();
        while body.validate_next() {}
    }
    (body.validated_blocks(), body.finish())
}

#[test]
fn streamed_blocks_match_one_shot_validation_across_fragments_and_field_orders() {
    let after_blocks = offset(MODERN, b"\x0cstart_height", 0);
    let mut prefix = walletcore_core::RangeBody::new(START, 2);
    prefix.append(&MODERN[..after_blocks]).unwrap();
    assert!(prefix.validate_next());
    assert_eq!(prefix.validated_blocks(), 1);
    assert!(prefix.validate_next());
    assert!(!prefix.validate_next());
    assert!(prefix.finish().is_err());

    let mut reordered = MODERN.to_vec();
    reordered[10..].rotate_left(after_blocks - 10);
    for (data, expected_cached) in [(MODERN, 2), (&reordered[..], 0)] {
        for step in [1, 8192, data.len()] {
            let (cached, decode) = streamed(data, step);
            // Geometric incomplete-object backoff may defer the last object until EOF.
            if step == data.len() {
                assert_eq!(cached, expected_cached);
            }
            let blocks = decode.unwrap();
            let expected = decode_block_range(data, START, 2).unwrap();
            assert_eq!(blocks.len(), expected.len());
            for (b, e) in blocks.iter().zip(expected) {
                assert_eq!(b.block, e.block);
                assert_eq!(b.txs, e.txs);
                assert_eq!((b.first_rct, b.next_rct), (e.first_rct, e.next_rct));
            }
        }
    }
}

#[test]
fn streamed_prefix_cannot_bypass_final_envelope_indices_or_block_validation() {
    let mut malformed = Vec::new();
    for key in [
        &b"status\x0a\x08"[..],
        b"untrusted\x0b",
        b"prunable_hash\x0a\x80",
    ] {
        let mut bytes = MODERN.to_vec();
        bytes[offset(MODERN, key, 0) + key.len()] ^= 3;
        malformed.push(bytes);
    }
    let mut indices = MODERN.to_vec();
    indices[offset(MODERN, &167747776u64.to_le_bytes(), 0)] ^= 1;
    malformed.push(indices);
    let mut trailing = MODERN.to_vec();
    trailing.push(0);
    malformed.push(trailing);
    for bytes in malformed {
        assert_eq!(
            streamed(&bytes, 4000).1.unwrap_err(),
            decode_block_range(&bytes, START, 2).unwrap_err()
        );
    }
}

#[test]
fn streamed_body_bounds_all_fragment_reservations() {
    use walletcore_core::{RangeBody, RangeError};
    let mut body = RangeBody::new(START, 2);
    body.reserve(MAX_RANGE_BODY).unwrap();
    assert_eq!(body.capacity(), MAX_RANGE_BODY);
    body.append(&[0]).unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body.reserve(usize::MAX), Err(RangeError::Limit));
    assert_eq!(body.reserve(MAX_RANGE_BODY), Err(RangeError::Limit));
    assert_eq!(body.len(), 1);
}

#[test]
fn streamed_unknown_nested_field_must_end_before_the_next_block() {
    let marker = b"\x06pruned\x0b";
    let first = offset(MODERN, marker, 0) - 1;
    let second = offset(MODERN, marker, 1) - 1;
    let mut bytes = MODERN.to_vec();
    bytes[first] += 4; // One additional top-level field in block zero.
    let mut unknown = b"\x01x\x0c\x04\x01y\x0a".to_vec();
    let n = 6000usize;
    unknown.extend_from_slice(&(((n as u16) << 2) | 1).to_le_bytes());
    unknown.resize(unknown.len() + n, 0);
    bytes.splice(second..second, unknown.iter().copied());
    let end = second + unknown.len();
    for length in [second, second + 6, second + 7, end - 1] {
        let mut body = walletcore_core::RangeBody::new(START, 2);
        body.append(&bytes[..length]).unwrap();
        assert!(!body.validate_next());
        assert_eq!(body.validated_blocks(), 0);
        assert_eq!(
            body.finish().unwrap_err(),
            decode_block_range(&bytes[..length], START, 2).unwrap_err()
        );
    }
    let (cached, result) = streamed(&bytes, bytes.len());
    assert_eq!(cached, 2);
    assert_eq!(
        result.unwrap()[1].block,
        decode_block_range(MODERN, START, 2).unwrap()[1].block
    );
    // Let the unknown field consume the first byte of the second block.
    bytes[second + 6] += 4;
    assert_eq!(
        streamed(&bytes, 137).1.unwrap_err(),
        decode_block_range(&bytes, START, 2).unwrap_err()
    );
}

#[test]
fn streamed_oversized_block_takes_the_same_bounded_error_path() {
    use walletcore_core::RangeError;
    let (p, w, n) = string_at(MODERN, b"block", 0);
    let raw_len = 256 * 1024 + 1;
    let mut bytes = MODERN[..p].to_vec();
    bytes.extend_from_slice(&(((raw_len as u32) << 2) | 2).to_le_bytes());
    bytes.resize(bytes.len() + raw_len, 0);
    bytes.extend_from_slice(&MODERN[p + w + n..]);
    assert_eq!(
        decode_block_range(&bytes, START, 2).unwrap_err(),
        RangeError::Limit
    );
    assert_eq!(streamed(&bytes, 11).1.unwrap_err(), RangeError::Limit);
}

#[test]
fn request_matches_monero_portable_storage() {
    let mut expected =
        b"\x01\x11\x01\x01\x01\x01\x02\x01\x01\x0c\x05prune\x0b\x01\x0cstart_height\x05".to_vec();
    expected.extend(START.to_le_bytes());
    expected.extend(b"\x0fmax_block_count\x05");
    expected.extend((MAX_RANGE_BLOCKS as u64).to_le_bytes());
    assert_eq!(encode_blocks_request(START, MAX_RANGE_BLOCKS), expected);
    for (start, count) in [(0, 1), (1, 0), (1, MAX_RANGE_BLOCKS + 1), (u64::MAX, 1)] {
        assert!(encode_blocks_request(start, count).is_empty());
    }
}

#[test]
fn verified_mainnet_prefix_has_the_expected_hashes_and_ringct_indices() {
    let b = decode_block_range(MODERN, START, MAX_RANGE_BLOCKS).unwrap();
    assert_eq!(b.len(), 2);
    let hashes = [
        "59369313053e009c06f2130568d961ba466764ea11563626f1adb207a7521c4e",
        "5217cc745b061c1ae8a50a462b7ecd7d04c01c92531847f5c96fe9bf102c2a4d",
    ];
    for (blk, expected) in b.iter().zip(hashes) {
        let hex: String = blk
            .block
            .hash()
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect();
        assert_eq!(hex, expected);
        assert_eq!(blk.block.transactions.len(), blk.txs.len());
    }
    assert_eq!((b[0].txs.len(), b[1].txs.len()), (33, 16));
    assert_eq!(
        (b[0].first_rct, b[1].first_rct),
        (Some(167747696), Some(167747776))
    );
}

#[test]
fn permits_pre_ringct_amount_specific_indices() {
    assert!(
        decode_block_range(EARLY, 1, 2)
            .unwrap()
            .iter()
            .all(|b| b.first_rct.is_none())
    );
    let b = decode_block_range(V1, 100000, 1).unwrap();
    assert_eq!(b[0].txs.len(), 2);
    assert_eq!(b[0].first_rct, None);
}

#[test]
fn enforces_framing_heights_flags_and_limits() {
    assert!(decode_block_range(MODERN, START + 1, 2).is_err());
    assert!(decode_block_range(MODERN, START, 1).is_err());
    assert!(decode_block_range(MODERN, START, 0).is_err());
    assert!(decode_block_range(MODERN, START, MAX_RANGE_BLOCKS + 1).is_err());
    assert!(decode_block_range(&vec![0; MAX_RANGE_BODY + 1], START, 2).is_err());
    for len in [0, 9, 35, MODERN.len() - 1] {
        assert!(decode_block_range(&MODERN[..len], START, 2).is_err());
    }
    let mut bad = MODERN.to_vec();
    bad.push(0);
    assert!(decode_block_range(&bad, START, 2).is_err());
    for (key, n, replacement) in [
        (&b"pruned\x0b"[..], 0usize, 0u8),
        (&b"pruned\x0b"[..], 0, 2),
        (&b"status\x0a\x08"[..], 0, b'X'),
        (&b"prunable_hash\x0a\x80"[..], 0, 0),
    ] {
        let mut b = MODERN.to_vec();
        let at = offset(&b, key, n) + key.len();
        b[at] = replacement;
        assert!(decode_block_range(&b, START, 2).is_err());
    }
}

#[test]
fn rejects_discontinuous_indices_in_and_between_blocks() {
    for index in [167747698u64, 167747776u64] {
        let mut b = MODERN.to_vec();
        let off = offset(&b, &index.to_le_bytes(), 0);
        b[off] ^= 1;
        assert!(decode_block_range(&b, START, 2).is_err());
    }
}

fn string_at(data: &[u8], name: &[u8], occurrence: usize) -> (usize, usize, usize) {
    let mut tag = vec![name.len() as u8];
    tag.extend(name);
    tag.push(10);
    let prefix = offset(data, &tag, occurrence) + tag.len();
    let width = 1 << (data[prefix] & 3);
    let mut encoded = [0u8; 8];
    encoded[..width].copy_from_slice(&data[prefix..prefix + width]);
    (prefix, width, (u64::from_le_bytes(encoded) >> 2) as usize)
}

#[test]
fn checks_order_proofs_and_block_linkage_separately() {
    use walletcore_core::{Block, RangeError};
    let mut b = MODERN.to_vec();
    let (p, w, n) = string_at(&b, b"block", 0);
    let mut raw = &b[p + w..p + w + n];
    let block = Block::read(&mut raw).unwrap();
    assert!(raw.is_empty());
    let first = offset(&b, &block.transactions[0], 0);
    let second = offset(&b, &block.transactions[1], 0);
    b[first..first + 32].copy_from_slice(&block.transactions[1]);
    b[second..second + 32].copy_from_slice(&block.transactions[0]);
    assert_eq!(
        decode_block_range(&b, START, 2).unwrap_err(),
        RangeError::Hash
    );

    let mut b = MODERN.to_vec();
    let (p, w, n) = string_at(&b, b"prunable_hash", 0);
    assert_eq!(n, 32);
    b[p + w..p + w + n].fill(0);
    assert!(
        decode_block_range(&b, START, 2)
            .unwrap_err()
            .needs_full_transactions()
    );

    let mut b = MODERN.to_vec();
    let (p, w, n) = string_at(&b, b"block", 1);
    let off = offset(&b[p + w..p + w + n], &block.hash(), 0);
    b[p + w + off] ^= 1;
    assert_eq!(
        decode_block_range(&b, START, 2).unwrap_err(),
        RangeError::Link
    );
}

#[test]
fn v1_signed_bytes_are_mandatory_and_authenticated() {
    use walletcore_core::{Pruned, RangeError, Transaction};
    let mut b = V1.to_vec();
    let (p, w, n) = string_at(&b, b"blob", 0);
    let blob_start = p + w;
    let mut signatures = &b[blob_start..blob_start + n];
    Transaction::<Pruned>::read(&mut signatures).unwrap();
    let prefix_len = n - signatures.len();
    assert!(prefix_len < n);
    b[blob_start + n - 1] ^= 1;
    assert_eq!(
        decode_block_range(&b, 100000, 1).unwrap_err(),
        RangeError::Hash
    );
    let new_size = ((prefix_len << 2) | usize::from(b[p] & 3)).to_le_bytes();
    b[p..p + w].copy_from_slice(&new_size[..w]);
    b.drain(blob_start + prefix_len..blob_start + n);
    assert!(
        decode_block_range(&b, 100000, 1)
            .unwrap_err()
            .needs_full_transactions()
    );
}

#[test]
fn indices_cannot_be_reassigned_between_transactions_or_amount_pools() {
    use walletcore_core::RangeError;
    let mut b = MODERN.to_vec();
    // Exchange a single-element miner vector for two outputs, and the following two
    // for one. The flattened sequence of all three valid indices is unchanged.
    let key = [7, b'i', b'n', b'd', b'i', b'c', b'e', b's', 0x85];
    let pos = offset(&b, &key, 0) + key.len();
    assert_eq!(b[pos], 4);
    let next = offset(&b, &key, 1) + key.len();
    assert_eq!(b[next], 8);
    let index = b[next + 1..next + 9].to_vec();
    b.drain(next + 1..next + 9);
    b[next] = 4;
    b.splice(pos + 9..pos + 9, index);
    b[pos] = 8;
    assert_eq!(
        decode_block_range(&b, START, 2).unwrap_err(),
        RangeError::Indices
    );

    let mut b = EARLY.to_vec();
    // Amount buckets can both have index zero. For the same amount a second
    // appearance must increment its previous index.
    let p = offset(&b, &key, 1) + key.len() + 1;
    b[p + 3 * 8] ^= 1;
    assert_eq!(
        decode_block_range(&b, 1, 2).unwrap_err(),
        RangeError::Indices
    );
}

#[test]
fn rejects_ambiguous_fields_and_invalid_declared_counts() {
    use walletcore_core::RangeError;
    let mut b = MODERN.to_vec();
    assert_eq!(b[9] & 3, 0);
    b[9] += 4;
    b.extend(b"\x06status\x0a\x08OK");
    assert_eq!(
        decode_block_range(&b, START, 2).unwrap_err(),
        RangeError::Encoding
    );
    let mut b = MODERN.to_vec();
    let p = offset(&b, b"blocks\x8c", 0) + 7;
    b[p] = 0xff;
    b.splice(p + 1..p + 1, [0xff; 7]);
    // The claimed length is impossible for the bytes present. A complete but
    // oversized response is the case that can be retried with a smaller range.
    assert_eq!(
        decode_block_range(&b, START, 2).unwrap_err(),
        RangeError::Encoding
    );
    assert!(decode_block_range(MODERN, START, 1).unwrap_err().is_limit());
}

#[test]
fn binary_and_full_json_match_and_feed_the_strict_scanner() {
    use walletcore_core::{
        Network, WalletIdentity, parse_tx_batch_verified, scan_assembled_strict,
    };
    let blocks = decode_block_range(MODERN, START, 2).unwrap();
    assert_eq!(blocks[0].next_rct, blocks[1].first_rct);
    let expected = blocks[0].block.transactions[..2]
        .iter()
        .map(|hash| hash.iter().map(|b| format!("{b:02x}")).collect())
        .collect::<Vec<String>>();
    let full = parse_tx_batch_verified(
        include_bytes!("fixtures/mainnet-3806161-full.json"),
        &expected,
    )
    .unwrap();
    for ((full_tx, _), range_tx) in full.iter().zip(&blocks[0].txs) {
        assert_eq!(full_tx, range_tx);
    }

    let (identity, _) = WalletIdentity::generate_from_entropy(
        zeroize::Zeroizing::new([71; 64]),
        START,
        Network::Mainnet,
    )
    .unwrap();
    let wallet = identity.open().unwrap();
    for b in blocks {
        assert!(
            scan_assembled_strict(wallet.view_pair().unwrap(), b.block, b.txs, b.first_rct)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn busy_needs_an_unambiguous_trusted_error_reply() {
    let status = b"\x06status\x0a\x10BUSY";
    let trusted = b"\x09untrusted\x0b\x00";
    let error = |fields: &[&[u8]], tail: &[u8]| {
        let mut doc = b"\x01\x11\x01\x01\x01\x01\x02\x01\x01".to_vec();
        doc.push((fields.len() * 4) as u8);
        for f in fields {
            doc.extend(*f);
        }
        doc.extend(tail);
        decode_block_range(&doc, START, 1).unwrap_err()
    };
    assert!(error(&[status, trusted], &[]).is_busy());
    assert!(error(&[trusted, status], &[]).is_busy());
    for e in [
        error(&[status, b"\x09untrusted\x0b\x01"], &[]),
        error(&[b"\x09untrusted\x0b\x02", status], &[]),
        error(&[trusted], &[]),
        error(&[status, status], &[]),
        error(&[status], &[0]),
        error(&[b"\x06status\x0a\x10BUS"], &[]),
        error(&[status, b"\x0eoutput_indices\x8c\x04\x00"], &[]),
        error(&[status, b"\x06blocks\x8c\x04\x00"], &[]),
    ] {
        assert!(!e.is_busy() && !e.is_limit() && !e.needs_full_transactions());
    }
}
