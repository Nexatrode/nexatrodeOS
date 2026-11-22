//! Experimental software lock policy, integrated into the UTM shell.
//! It does not protect vault keys or provide rollback-resistant rate limiting.

use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"NXLOCK01";
const ROUNDS: u32 = 600_000;
const SALT_SIZE: usize = 16;
const HASH_SIZE: usize = 32;
pub const RECORD_SIZE: usize = MAGIC.len() + SALT_SIZE + HASH_SIZE;
const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 128;
const SETTINGS_MAGIC: &[u8; 8] = b"NXLS0001";
pub const SETTINGS_SIZE: usize = 24 + RECORD_SIZE;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidCredential,
    InvalidRecord,
    InvalidTimeout,
}

/// The caller must obtain a fresh, unpredictable salt from a secure RNG.
pub struct Credential {
    salt: [u8; SALT_SIZE],
    verifier: [u8; HASH_SIZE],
}

impl Credential {
    pub fn create(password: &[u8], salt: [u8; SALT_SIZE]) -> Result<Self, Error> {
        if !(MIN_PASSWORD..=MAX_PASSWORD).contains(&password.len()) || salt == [0; SALT_SIZE] {
            return Err(Error::InvalidCredential);
        }
        let mut verifier = [0; HASH_SIZE];
        pbkdf2_hmac::<Sha256>(password, &salt, ROUNDS, &mut verifier);
        Ok(Self { salt, verifier })
    }

    pub fn encode(&self) -> [u8; RECORD_SIZE] {
        let mut out = [0; RECORD_SIZE];
        out[..8].copy_from_slice(MAGIC);
        out[8..24].copy_from_slice(&self.salt);
        out[24..].copy_from_slice(&self.verifier);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != RECORD_SIZE || &bytes[..8] != MAGIC || bytes[8..24] == [0; SALT_SIZE] {
            return Err(Error::InvalidRecord);
        }
        let mut salt = [0; SALT_SIZE];
        let mut verifier = [0; HASH_SIZE];
        salt.copy_from_slice(&bytes[8..24]);
        verifier.copy_from_slice(&bytes[24..]);
        Ok(Self { salt, verifier })
    }

    pub fn verify(&self, password: &[u8]) -> bool {
        let valid_length = (MIN_PASSWORD..=MAX_PASSWORD).contains(&password.len());
        // Do the same KDF for invalid input instead of returning immediately.
        // Bound attacker-controlled work for overlong inputs.
        let input = if password.len() <= MAX_PASSWORD { password } else { &[] };
        let mut candidate = Zeroizing::new([0; HASH_SIZE]);
        pbkdf2_hmac::<Sha256>(input, &self.salt, ROUNDS, &mut *candidate);
        bool::from(candidate.as_slice().ct_eq(&self.verifier)) && valid_length
    }
}

/// A versioned settings record, intended to be stored by an authenticated,
/// crash-consistent provider. This type does not implement persistence.
pub enum LockSettings {
    Disabled,
    Enabled {
        credential: Credential,
        idle_ms: u64,
    },
}

impl LockSettings {
    pub fn disabled() -> Self {
        Self::Disabled
    }

    pub fn enabled(credential: Credential, idle_ms: u64) -> Result<Self, Error> {
        if idle_ms == 0 {
            return Err(Error::InvalidTimeout);
        }
        Ok(Self::Enabled {
            credential,
            idle_ms,
        })
    }

    pub fn encode(&self) -> [u8; SETTINGS_SIZE] {
        let mut result = [0; SETTINGS_SIZE];
        result[..8].copy_from_slice(SETTINGS_MAGIC);
        if let Self::Enabled {
            credential,
            idle_ms,
        } = self
        {
            result[8] = 1;
            result[16..24].copy_from_slice(&idle_ms.to_le_bytes());
            result[24..].copy_from_slice(&credential.encode());
        }
        result
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != SETTINGS_SIZE || &bytes[..8] != SETTINGS_MAGIC || bytes[9..16] != [0; 7] {
            return Err(Error::InvalidRecord);
        }
        match bytes[8] {
            0 if bytes[16..].iter().all(|value| *value == 0) => Ok(Self::Disabled),
            1 => {
                let idle_ms =
                    u64::from_le_bytes(bytes[16..24].try_into().map_err(|_| Error::InvalidRecord)?);
                if idle_ms == 0 {
                    return Err(Error::InvalidRecord);
                }
                Ok(Self::Enabled {
                    credential: Credential::decode(&bytes[24..])?,
                    idle_ms,
                })
            }
            _ => Err(Error::InvalidRecord),
        }
    }

    pub fn into_controller(self, now_ms: u64) -> Result<Option<LockController>, Error> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled {
                credential,
                idle_ms,
            } => LockController::new(credential, idle_ms, now_ms).map(Some),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlockResult {
    Unlocked,
    Denied,
    Wait,
}

pub struct LockController {
    credential: Credential,
    locked: bool,
    idle_ms: u64,
    last_ms: u64,
    last_activity_ms: u64,
    retry_after_ms: u64,
    failures: u32,
}

impl LockController {
    pub fn new(credential: Credential, idle_ms: u64, now_ms: u64) -> Result<Self, Error> {
        if idle_ms == 0 {
            return Err(Error::InvalidTimeout);
        }
        Ok(Self {
            credential,
            locked: true,
            idle_ms,
            last_ms: now_ms,
            last_activity_ms: now_ms,
            retry_after_ms: now_ms,
            failures: 0,
        })
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// `now_ms` must be derived from the platform's monotonic clock.
    pub fn tick(&mut self, now_ms: u64) {
        if now_ms < self.last_ms {
            self.locked = true;
            self.retry_after_ms = now_ms.saturating_add(1_000);
            self.last_activity_ms = now_ms;
        } else if !self.locked && now_ms.saturating_sub(self.last_activity_ms) >= self.idle_ms {
            self.locked = true;
        }
        self.last_ms = now_ms;
    }

    pub fn activity(&mut self, now_ms: u64) {
        self.tick(now_ms);
        if !self.locked {
            self.last_activity_ms = now_ms;
        }
    }

    /// Call after a potentially expensive verifier finishes so the cooldown
    /// begins at completion, rather than expiring during the KDF itself.
    pub fn authentication_finished(&mut self, now_ms: u64, result: UnlockResult) {
        self.tick(now_ms);
        match result {
            UnlockResult::Denied => {
                let seconds = 1u64 << self.failures.saturating_sub(1).min(6);
                self.retry_after_ms = self.retry_after_ms.max(now_ms.saturating_add(seconds * 1_000));
            }
            UnlockResult::Unlocked if !self.locked => self.last_activity_ms = now_ms,
            _ => {}
        }
    }

    pub fn try_unlock(&mut self, password: &[u8], now_ms: u64) -> UnlockResult {
        self.tick(now_ms);
        if !self.locked {
            return UnlockResult::Unlocked;
        }
        if now_ms < self.retry_after_ms {
            return UnlockResult::Wait;
        }
        if self.credential.verify(password) {
            self.locked = false;
            self.failures = 0;
            self.last_activity_ms = now_ms;
            UnlockResult::Unlocked
        } else {
            self.failures = self.failures.saturating_add(1);
            let seconds = 1u64 << self.failures.saturating_sub(1).min(6);
            self.retry_after_ms = now_ms.saturating_add(seconds * 1_000);
            UnlockResult::Denied
        }
    }
}
