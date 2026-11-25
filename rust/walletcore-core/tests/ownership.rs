extern crate alloc;
extern crate self as sel4_microkit;
pub use pool_host::Channel;
#[path = "ownership/pool_host.rs"]
mod pool_host;
#[path = "../../src/wallet_scan_mailbox.rs"]
mod wallet_scan_mailbox;
#[path = "../../src/wallet_scan_pool.rs"]
mod wallet_scan_pool;
use monero_wallet::ed25519::{CompressedPoint, Scalar};
use walletcore_core::{
    Block, MAX_SCAN_JOB, MAX_SCAN_RESULT, OwnershipError, Pruned, RangeBlock, Transaction,
    ViewPair, decode_block_range, decode_scan_result, encode_scan_job, execute_scan_job,
    scan_assembled_strict, scan_verified_block,
};
use zeroize::Zeroizing;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

// Upstream monero-oxide wallet/src/tests/scan.rs contains a published funded RingCT
// transaction with two long encrypted amounts and a public test view key. The original
// pre-RingCT miner has legacy outputs; replay the same transaction with a v2 miner to
// exercise the compact modern path separately from the indexed legacy path.
fn funded() -> (ViewPair, Block, RangeBlock) {
    let v: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/funded-long-amount.json")).unwrap();
    let decode = |k: &str| hex(v[k].as_str().unwrap());
    let spend: [u8; 32] = decode("public_spend").try_into().unwrap();
    let view = ViewPair::new(
        CompressedPoint::from(spend).decompress().unwrap(),
        Zeroizing::new(Scalar::read(&mut decode("public_test_view").as_slice()).unwrap()),
    )
    .unwrap();
    let tx = Transaction::<Pruned>::read(&mut decode("pruned_transaction").as_slice()).unwrap();
    let old = Block::read(&mut decode("block").as_slice()).unwrap();
    let template = decode_block_range(
        include_bytes!("fixtures/mainnet-3806161-range2.bin"),
        3806161,
        2,
    )
    .unwrap()
    .remove(0)
    .block;
    let num = tx.prefix().outputs.len() + template.miner_transaction().prefix().outputs.len();
    let block = Block::new(
        template.header.clone(),
        template.miner_transaction().clone(),
        old.transactions.clone(),
    )
    .unwrap();
    (
        view,
        old,
        RangeBlock {
            block,
            txs: vec![tx],
            first_rct: Some(0),
            next_rct: Some(num as u64),
            output_indices: vec![],
        },
    )
}

#[test]
fn funded_worker_round_trip_matches_every_ordered_receipt_and_commit_field() {
    let (v, _, b) = funded();
    let serial = scan_assembled_strict(&v, b.block.clone(), b.txs.clone(), b.first_rct).unwrap();
    assert_eq!(serial.len(), 2);
    assert_eq!(
        serial
            .iter()
            .map(|x| x.commitment().amount)
            .collect::<Vec<_>>(),
        [10000, 10000]
    );
    let mut j = vec![0; MAX_SCAN_JOB];
    let mut r = vec![0; MAX_SCAN_RESULT];
    let n = encode_scan_job(&v, &b, &mut j).unwrap();
    let m = execute_scan_job(&j[..n], &mut r).unwrap();
    let scanned = decode_scan_result(&r[..m]).unwrap();
    assert_eq!(scanned.height, b.block.number() as u64);
    assert_eq!(scanned.hash, b.block.hash());
    assert_eq!(scanned.previous, b.block.header.previous);
    assert_eq!(scanned.miner_hash, b.block.miner_transaction().hash());
    assert_eq!(scanned.transactions, b.block.transactions);
    assert_eq!(
        scanned
            .outputs
            .iter()
            .map(|o| o.serialize())
            .collect::<Vec<_>>(),
        serial.iter().map(|o| o.serialize()).collect::<Vec<_>>()
    );
    let direct = scan_verified_block(&v, b).unwrap();
    assert_eq!(
        direct
            .outputs
            .iter()
            .map(|o| o.serialize())
            .collect::<Vec<_>>(),
        scanned
            .outputs
            .iter()
            .map(|o| o.serialize())
            .collect::<Vec<_>>()
    );
}

#[test]
fn enforce_legacy_missing_shapes_capacity_integrity_and_eof() {
    let (v, old, mut b) = funded();
    let mut j = vec![0; MAX_SCAN_JOB];
    let mut r = vec![0; MAX_SCAN_RESULT];
    assert_eq!(
        encode_scan_job(&v, &b, &mut [0; 20]),
        Err(OwnershipError::Limit)
    );
    let n = encode_scan_job(&v, &b, &mut j).unwrap();
    assert_eq!(
        execute_scan_job(&j[..n], &mut [0; 20]),
        Err(OwnershipError::Limit)
    );
    let m = execute_scan_job(&j[..n], &mut r).unwrap();
    for len in [0, 2, n - 1, n + 1] {
        assert!(execute_scan_job(&j[..len], &mut r).is_err());
    }
    for len in [0, 2, m - 1, m + 1] {
        assert!(decode_scan_result(&r[..len]).is_err());
    }
    j[n - 1] ^= 1;
    assert!(execute_scan_job(&j[..n], &mut r).is_err());
    r[m - 1] ^= 1;
    assert!(decode_scan_result(&r[..m]).is_err());
    b.next_rct = Some(u64::MAX);
    assert_eq!(
        encode_scan_job(&v, &b, &mut j),
        Err(OwnershipError::InvalidBlock)
    );
    b.block = old;
    assert!(matches!(
        scan_verified_block(&v, b),
        Err(OwnershipError::InvalidBlock)
    ));
}

fn seal(bytes: &mut [u8]) {
    let sum = monero_wallet::primitives::keccak256(&bytes[36..]);
    bytes[4..36].copy_from_slice(&sum);
}

#[test]
fn rejects_malformed_well_checksummed_jobs_and_receipts() {
    let (v, _, b) = funded();
    let mut job = vec![0; MAX_SCAN_JOB];
    let mut result = vec![0; MAX_SCAN_RESULT];
    let n = encode_scan_job(&v, &b, &mut job).unwrap();
    job.truncate(n);
    let m = execute_scan_job(&job, &mut result).unwrap();
    result.truncate(m);
    let mut wrong_key = job.clone();
    wrong_key[68..100].fill(0xff);
    seal(&mut wrong_key);
    assert_eq!(
        execute_scan_job(&wrong_key, &mut vec![0; MAX_SCAN_RESULT]),
        Err(OwnershipError::Encoding)
    );
    let mut extra = job.clone();
    extra.push(0);
    seal(&mut extra);
    assert_eq!(
        execute_scan_job(&extra, &mut vec![0; MAX_SCAN_RESULT]),
        Err(OwnershipError::Encoding)
    );
    let mut extra = result.clone();
    extra.push(0);
    seal(&mut extra);
    assert!(matches!(
        decode_scan_result(&extra),
        Err(OwnershipError::Encoding)
    ));

    let (_, _, b) = funded();
    let receipts = 36 + 8 + 3 * 32 + 4 + b.block.transactions.len() * 32;
    assert_eq!(
        u32::from_le_bytes(result[receipts..receipts + 4].try_into().unwrap()),
        2
    );
    let first = receipts + 4;
    let size = u32::from_le_bytes(result[first..first + 4].try_into().unwrap()) as usize;
    let next = first + 4 + size;
    let mut reversed = result[..first].to_vec();
    reversed.extend(&result[next..]);
    reversed.extend(&result[first..next]);
    seal(&mut reversed);
    assert!(matches!(
        decode_scan_result(&reversed),
        Err(OwnershipError::InvalidBlock)
    ));
    let mut missing = result;
    missing.truncate(next);
    seal(&mut missing);
    assert!(matches!(
        decode_scan_result(&missing),
        Err(OwnershipError::Encoding)
    ));
}

#[test]
fn original_public_transfer_and_supported_replay_both_find_two_receipts() {
    let (view, old, replay) = funded();
    assert!(scan_assembled_strict(&view, old.clone(), replay.txs.clone(), Some(0)).is_err());
    let indices = vec![vec![0; old.miner_transaction().prefix().outputs.len()], vec![0, 1]];
    let original = scan_verified_block(&view, RangeBlock { block: old, txs: replay.txs.clone(), first_rct: Some(0), next_rct: Some(2), output_indices: indices }).unwrap().outputs;
    assert_eq!(original.len(), 2);
    for (index, o) in original.iter().enumerate() {
        assert_eq!(o.transaction(), replay.block.transactions[0]);
        assert_eq!(o.index_in_transaction(), index as u64);
        assert_eq!(o.index_on_blockchain(), index as u64);
        assert_eq!(o.commitment().amount, 10000);
    }
}

fn wave(base: &RangeBlock, offset: usize) -> Vec<RangeBlock> {
    use monero_wallet::transaction::Input;
    let mut previous = base.block.header.previous;
    (0..4)
        .map(|i| {
            let mut header = base.block.header.clone();
            header.previous = previous;
            let mut miner = base.block.miner_transaction().clone();
            miner.prefix_mut().inputs = vec![Input::Gen(base.block.number() + offset + i)];
            let (txs, hashes) = if i == 1 {
                (base.txs.clone(), base.block.transactions.clone())
            } else {
                (vec![], vec![])
            };
            let n = miner.prefix().outputs.len()
                + txs.iter().map(|t| t.prefix().outputs.len()).sum::<usize>();
            let first = if i < 2 { i as u64 } else { i as u64 + 2 };
            let block = Block::new(header, miner, hashes).unwrap();
            previous = block.hash();
            RangeBlock {
                block,
                txs,
                first_rct: Some(first),
                next_rct: Some(first + n as u64),
                output_indices: vec![],
            }
        })
        .collect::<Vec<_>>()
}

#[test]
fn failed_reuse_invalidates_old_wire_objects() {
    let (v, old, mut b) = funded();
    let mut j = vec![0; MAX_SCAN_JOB];
    let mut r = vec![0; MAX_SCAN_RESULT];
    let n = encode_scan_job(&v, &b, &mut j).unwrap();
    let m = execute_scan_job(&j[..n], &mut r).unwrap();
    assert!(decode_scan_result(&r[..m]).is_ok());
    j[n - 1] ^= 1;
    assert_eq!(
        execute_scan_job(&j[..n], &mut r),
        Err(OwnershipError::Encoding)
    );
    assert!(r[..4].iter().all(|b| *b == 0));
    b.block = old;
    assert_eq!(
        encode_scan_job(&v, &b, &mut j),
        Err(OwnershipError::InvalidBlock)
    );
    assert!(j[..4].iter().all(|b| *b == 0));
}

// Use published test keys and output keys from the owned RingCT fixture, with
// transparent amounts. The ECDH ownership derivation is shared by V1 and V2.
// These are synthetic scan fixtures, not consensus-valid spend transactions.
fn legacy_owned(miner_only: bool, same_amount: bool) -> (ViewPair, RangeBlock) {
    use monero_wallet::transaction::{Input, Timelock};
    let (view, _, mut modern) = funded();
    let mut prefix = modern.txs[0].prefix().clone();
    for (n, output) in prefix.outputs.iter_mut().enumerate() {
        output.amount = Some(if n == 0 || same_amount { 10_000 } else { 20_000 });
        output.view_tag = None;
    }
    prefix.additional_timelock = Timelock::Block(4_000_000);
    if miner_only {
        prefix.inputs = modern.block.miner_transaction().prefix().inputs.clone();
        let miner = Transaction::V1 { prefix, signatures: vec![] };
        let block = Block::new(modern.block.header.clone(), miner, vec![]).unwrap();
        (view, RangeBlock { block, txs: vec![], first_rct: None, next_rct: None, output_indices: vec![if same_amount { vec![90, 91] } else { vec![90, 3] }] })
    } else {
        // The authenticated block's hash list is the transaction evidence. A
        // worker does not receive/need full V1 signatures a second time.
        assert!(!matches!(prefix.inputs[0], Input::Gen(_)));
        modern.txs = vec![Transaction::V1 { prefix, signatures: () }];
        modern.next_rct = Some(modern.block.miner_transaction().prefix().outputs.len() as u64);
        modern.output_indices = vec![(0..modern.next_rct.unwrap()).collect(), if same_amount { vec![90, 91] } else { vec![90, 3] }];
        (view, modern)
    }
}

#[test]
fn owned_legacy_outputs_keep_amount_bucket_indices_through_workers_and_storage() {
    use walletcore_core::{validate_scanned_block, WalletOutput};
    use monero_wallet::transaction::Timelock;
    for miner_only in [false, true] {
        for same_amount in [false, true] {
            let (view, b) = legacy_owned(miner_only, same_amount);
            let mut job = vec![0; MAX_SCAN_JOB]; let mut receipt = vec![0; MAX_SCAN_RESULT];
            let n = encode_scan_job(&view, &b, &mut job).unwrap();
            assert_eq!(&job[..4], b"NXJ2");
            let m = execute_scan_job(&job[..n], &mut receipt).unwrap();
            let scanned = decode_scan_result(&receipt[..m]).unwrap();
            assert!(validate_scanned_block(&scanned, &b));
            assert_eq!(scanned.outputs.len(), 2);
            for (i, output) in scanned.outputs.iter().enumerate() {
                let amount = if i == 0 || same_amount { 10_000 } else { 20_000 };
                assert!(output.is_legacy());
                assert_eq!(output.amount_bucket(), amount);
                assert_eq!(output.commitment().amount, amount);
                assert_eq!(output.index_on_blockchain(), if i == 0 { 90 } else if same_amount { 91 } else { 3 });
                assert_eq!(output.additional_timelock(), Timelock::Block(4_000_000));
                let bytes = output.serialize();
                let restored = WalletOutput::read(&mut bytes.as_slice()).unwrap();
                assert_eq!(output, &restored);
                assert_eq!(restored.serialize(), bytes);
                let points = [restored.key(), restored.commitment().commit()];
                let members = (0..16).map(|n| (if n == 0 { restored.index_on_blockchain() } else { restored.index_on_blockchain() + n }, points[0].compress().to_bytes(), points[1].compress().to_bytes())).collect::<Vec<_>>();
                assert!(walletcore_core::build_decoys_for_output(&restored, &members).is_none());
                let decoys = monero_wallet::ringct::clsag::Decoys::new(vec![90, 1], 0, vec![points; 2]).unwrap();
                assert!(monero_wallet::OutputWithDecoys::from_wallet_output(&restored, decoys).is_none());
            }
            let mut forged = scanned;
            let mut bytes = forged.outputs[0].serialize();
            bytes[40..48].copy_from_slice(&901u64.to_le_bytes());
            forged.outputs[0] = WalletOutput::read(&mut bytes.as_slice()).unwrap();
            assert!(!validate_scanned_block(&forged, &b));
            job[..4].copy_from_slice(b"NXJ1");
            assert!(execute_scan_job(&job[..n], &mut receipt).is_err());
        }
    }
}

#[test]
fn legacy_indices_cannot_be_omitted_duplicated_or_reordered_inside_an_amount_bucket() {
    for bad in [vec![], vec![vec![90]], vec![vec![90, 90]], vec![vec![91, 90]], vec![vec![u64::MAX, 0]]] {
        let (view, mut b) = legacy_owned(true, true);
        b.output_indices = bad;
        assert!(encode_scan_job(&view, &b, &mut vec![0; MAX_SCAN_JOB]).is_err());
    }
    let (view, mut b) = legacy_owned(false, false);
    // Exact amount-zero span remains mandatory in a mixed block.
    b.output_indices[0][0] += 1;
    assert!(scan_verified_block(&view, b).is_err());
}

#[test]
fn legacy_receipt_cannot_deserialize_with_a_different_amount_than_its_bucket() {
    let (view, block) = legacy_owned(true, false);
    let output = scan_verified_block(&view, block).unwrap().outputs.remove(0);
    let mut bytes = output.serialize();
    // Absolute ID 40, bucket index 8, key/offset/mask 32 each, then amount.
    bytes[144..152].copy_from_slice(&10_001u64.to_le_bytes());
    assert!(walletcore_core::WalletOutput::read(&mut bytes.as_slice()).is_err());
}
