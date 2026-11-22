//! Versioned system-value/lock-settings payload. The vault must authenticate
//! this payload and commit it atomically; this codec does neither by itself.
use crate::device_lock::{LockSettings, SETTINGS_SIZE};

const MAGIC: &[u8; 8] = b"NXSYS001";
pub const SYSTEM_RECORD_SIZE: usize = 16 + SETTINGS_SIZE;

pub fn encode_system(value: u64, settings: &[u8; SETTINGS_SIZE]) -> Result<[u8; SYSTEM_RECORD_SIZE], ()> {
    LockSettings::decode(settings).map_err(|_| ())?;
    let mut result = [0; SYSTEM_RECORD_SIZE];
    result[..8].copy_from_slice(MAGIC);
    result[8..16].copy_from_slice(&value.to_le_bytes());
    result[16..].copy_from_slice(settings);
    Ok(result)
}

pub fn decode_system(bytes: &[u8]) -> Result<(u64, [u8; SETTINGS_SIZE]), ()> {
    if bytes.len() != SYSTEM_RECORD_SIZE || &bytes[..8] != MAGIC { return Err(()); }
    let value = u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| ())?);
    let mut settings = [0; SETTINGS_SIZE];
    settings.copy_from_slice(&bytes[16..]);
    LockSettings::decode(&settings).map_err(|_| ())?;
    Ok((value, settings))
}
