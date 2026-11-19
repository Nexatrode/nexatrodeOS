#![allow(missing_docs)]

use monero_ed25519::CompressedPoint;

mod hex;

#[test]
fn canonical_y_and_sign_boundaries_match_round_trip() {
  use curve25519_dalek::edwards::CompressedEdwardsY;

  // Exercise both signs of x at y = 0, 1, p - 1 and all 19 values above p - 1,
  // along with nearby canonical encodings on both sides of the byte carry.
  for sign in [0, 0x80] {
    for low in 0..=255 {
      for mut bytes in [[0; 32], [0xff; 32]] {
        bytes[0] = low;
        bytes[31] = (bytes[31] & 0x7f) | sign;
        let reference = CompressedEdwardsY(bytes)
          .decompress()
          .filter(|point| point.compress().to_bytes() == bytes);
        assert_eq!(
          CompressedPoint::from(bytes).decompress().map(|point| point.into()),
          reference,
          "encoding {bytes:?}"
        );
      }
    }
  }
}

#[test]
fn decompress() {
  let reader = include_str!("./tests.txt");

  for line in reader.lines() {
    let mut words = line.split_whitespace();

    let command = words.next().unwrap();
    match command {
      "check_key" => {
        let key = hex::decode(words.next().unwrap());
        let expected = match words.next().unwrap() {
          "true" => true,
          "false" => false,
          _ => unreachable!("invalid result"),
        };

        let actual = CompressedPoint::from(key).decompress();
        assert_eq!(actual.is_some(), expected);
      }
      "hash_to_ec" => {}
      _ => unreachable!("unknown command"),
    }
  }
}
