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
// exercise the production path without relaxing the wallet's legacy-output policy.
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
        Err(OwnershipError::LegacyOutputs)
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
    let original = scan_assembled_strict(&view, old, replay.txs.clone(), Some(0)).unwrap();
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
        Err(OwnershipError::LegacyOutputs)
    );
    assert!(j[..4].iter().all(|b| *b == 0));
}
