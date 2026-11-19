#![allow(missing_docs)]

use monero_ed25519::Point;
use sha3::{Digest, Keccak256};

mod hex;

#[test]
fn biased_hash() {
  let reader = include_str!("./tests.txt");

  for line in reader.lines() {
    let mut words = line.split_whitespace();

    let command = words.next().unwrap();
    match command {
      "check_key" => {}
      "hash_to_ec" => {
        let preimage = hex::decode(words.next().unwrap());
        let actual = Point::biased_hash(preimage);
        let expected = hex::decode(words.next().unwrap());
        assert_eq!(actual.compress().to_bytes(), expected);
      }
      _ => unreachable!("unknown command"),
    }
  }
}

#[test]
fn biased_hash_legacy_corpus() {
  // This corpus digest is fixed from revision 34201e5 with crypto-bigint 0.5.5.
  // Retain it across arithmetic dependency upgrades: key images and generators
  // must remain byte-for-byte compatible, not merely valid curve points.
  let mut digest = Keccak256::new();
  let mut check = |input| {
    digest.update(Point::biased_hash(input).compress().to_bytes());
  };
  check([0; 32]);
  check([0xff; 32]);
  for byte in 0..=u8::MAX {
    check([byte; 32]);
  }
  for bit in 0..256 {
    let mut input = [0; 32];
    input[bit / 8] = 1 << (bit % 8);
    check(input);
  }
  for index in 0..4096_u32 {
    let mut input = Keccak256::new();
    input.update(b"monero-ed25519 hash-to-point compatibility corpus v1");
    input.update(index.to_le_bytes());
    check(input.finalize().into());
  }
  let actual: [u8; 32] = digest.finalize().into();
  assert_eq!(
    actual,
    [
      0xcf, 0xb8, 0x25, 0x36, 0xa1, 0xfc, 0x3f, 0x76, 0x30, 0x3c, 0x6c, 0x5c, 0xbd, 0xc7, 0x76,
      0xfb, 0x53, 0x6e, 0x1b, 0x22, 0x21, 0x6d, 0x80, 0xed, 0xff, 0xdc, 0xb6, 0x56, 0x64, 0xfb,
      0x77, 0x24,
    ]
  );
}
