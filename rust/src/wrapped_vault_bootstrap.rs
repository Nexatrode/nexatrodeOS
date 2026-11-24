//! Disposable-only lock bootstrap payload; storage journal is implemented by
//! the experimental UTM vault, and this record is not a hardware trust anchor.
use crate::{
    device_lock::{LockSettings, SETTINGS_SIZE},
    vault_key_prototype::ENVELOPE_SIZE,
};

const MAGIC: &[u8; 8] = b"NXWBOOT1";
pub const BOOTSTRAP_SIZE: usize = 8 + SETTINGS_SIZE + ENVELOPE_SIZE + 32;

pub fn encode(
    settings: &[u8; SETTINGS_SIZE],
    envelope: &[u8; ENVELOPE_SIZE],
    key_hash: &[u8; 32],
) -> Option<[u8; BOOTSTRAP_SIZE]> {
    if !matches!(
        LockSettings::decode(settings),
        Ok(LockSettings::Enabled { .. })
    ) || !crate::vault_key_prototype::supported_envelope(envelope)
    {
        return None;
    }
    let mut result = [0; BOOTSTRAP_SIZE];
    result[..8].copy_from_slice(MAGIC);
    result[8..8 + SETTINGS_SIZE].copy_from_slice(settings);
    result[8 + SETTINGS_SIZE..8 + SETTINGS_SIZE + ENVELOPE_SIZE].copy_from_slice(envelope);
    result[8 + SETTINGS_SIZE + ENVELOPE_SIZE..].copy_from_slice(key_hash);
    Some(result)
}

pub fn decode(bytes: &[u8]) -> Option<([u8; SETTINGS_SIZE], [u8; ENVELOPE_SIZE], [u8; 32])> {
    if bytes.len() != BOOTSTRAP_SIZE || &bytes[..8] != MAGIC {
        return None;
    }
    let settings: [u8; SETTINGS_SIZE] = bytes[8..8 + SETTINGS_SIZE].try_into().ok()?;
    let envelope: [u8; ENVELOPE_SIZE] = bytes[8 + SETTINGS_SIZE..8 + SETTINGS_SIZE + ENVELOPE_SIZE]
        .try_into()
        .ok()?;
    let hash: [u8; 32] = bytes[8 + SETTINGS_SIZE + ENVELOPE_SIZE..].try_into().ok()?;
    encode(&settings, &envelope, &hash)?;
    Some((settings, envelope, hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_lock::Credential;

    #[test]
    fn valid_lock_and_envelope_round_trip_and_invalid_records_fail_closed() {
        let credential = Credential::create(b"disposable password", [3; 16]).unwrap();
        let settings = LockSettings::enabled(credential, 60_000).unwrap().encode();
        let envelope =
            crate::vault_key_prototype::seal(b"disposable password", &[4; 28], &[5; 32]).unwrap();
        let encoded = encode(&settings, &envelope, &[6; 32]).unwrap();
        assert_eq!(decode(&encoded), Some((settings, envelope, [6; 32])));
        let mut bad = encoded;
        bad[0] ^= 1;
        assert!(decode(&bad).is_none());
        assert!(decode(&encoded[..encoded.len() - 1]).is_none());
        assert!(encode(&LockSettings::disabled().encode(), &envelope, &[6; 32]).is_none());
        let mut bad = envelope;
        bad[8] ^= 1;
        assert!(encode(&settings, &bad, &[6; 32]).is_none());
    }
}
