use crate::device_lock::{Credential, LockController, LockSettings, UnlockResult};
use crate::system_lock_record::{decode_system, encode_system};

const PASSWORD: &[u8] = b"a long test passphrase";
const SALT: [u8; 16] = [7; 16];

#[test]
fn credential_requires_the_exact_passphrase_and_rejects_corrupt_records() {
    let credential = Credential::create(PASSWORD, SALT).unwrap();
    let mut bytes = credential.encode();
    let restored = Credential::decode(&bytes).unwrap();
    assert!(restored.verify(PASSWORD));
    assert!(!restored.verify(b"a long test passphrasf"));
    assert!(!restored.verify(b"a long test passphrase "));
    assert!(!restored.verify(b""));
    assert!(!restored.verify(&[b'a'; 129]));
    bytes[0] ^= 1;
    assert!(Credential::decode(&bytes).is_err());
    assert!(Credential::decode(&bytes[..bytes.len() - 1]).is_err());
    assert!(Credential::create(b"short", SALT).is_err());
    assert!(Credential::create(PASSWORD, [0; 16]).is_err());
}

#[test]
fn enabled_lock_starts_locked_and_throttles_wrong_passwords() {
    let credential = Credential::create(PASSWORD, SALT).unwrap();
    let mut lock = LockController::new(credential, 30_000, 0).unwrap();
    assert!(lock.is_locked());
    assert_eq!(
        lock.try_unlock(b"incorrect passphrase", 100),
        UnlockResult::Denied
    );
    assert_eq!(lock.try_unlock(PASSWORD, 101), UnlockResult::Wait);
    assert!(lock.is_locked());
    assert_eq!(lock.try_unlock(PASSWORD, 1_100), UnlockResult::Unlocked);
    assert!(!lock.is_locked());
}

#[test]
fn activity_delays_auto_lock_and_clock_reversal_fails_closed() {
    let credential = Credential::create(PASSWORD, SALT).unwrap();
    let mut lock = LockController::new(credential, 30_000, 0).unwrap();
    assert_eq!(lock.try_unlock(PASSWORD, 1_000), UnlockResult::Unlocked);
    lock.activity(20_000);
    lock.tick(49_999);
    assert!(!lock.is_locked());
    lock.tick(50_000);
    assert!(lock.is_locked());
    assert_eq!(lock.try_unlock(PASSWORD, 51_000), UnlockResult::Unlocked);
    lock.tick(50_999);
    assert!(lock.is_locked());
}

#[test]
fn failed_attempts_increase_wait_and_do_not_unlock_with_correct_password_early() {
    let credential = Credential::create(PASSWORD, SALT).unwrap();
    let mut lock = LockController::new(credential, 30_000, 0).unwrap();
    assert_eq!(
        lock.try_unlock(b"wrong passphrase", 0),
        UnlockResult::Denied
    );
    assert_eq!(
        lock.try_unlock(b"wrong passphrase", 1_000),
        UnlockResult::Denied
    );
    assert_eq!(lock.try_unlock(PASSWORD, 2_999), UnlockResult::Wait);
    assert!(lock.is_locked());
    assert_eq!(lock.try_unlock(PASSWORD, 3_000), UnlockResult::Unlocked);
}

#[test]
fn failure_cooldown_starts_after_slow_password_check_finishes() {
    let credential = Credential::create(PASSWORD, SALT).unwrap();
    let mut lock = LockController::new(credential, 30_000, 0).unwrap();
    assert_eq!(lock.try_unlock(b"wrong passphrase", 100), UnlockResult::Denied);
    lock.authentication_finished(500, UnlockResult::Denied);
    assert_eq!(lock.try_unlock(PASSWORD, 1_100), UnlockResult::Wait);
    assert_eq!(lock.try_unlock(PASSWORD, 1_500), UnlockResult::Unlocked);
}

#[test]
fn manual_lock_requires_password_again_and_restart_starts_locked() {
    let record = Credential::create(PASSWORD, SALT).unwrap().encode();
    let mut lock = LockController::new(Credential::decode(&record).unwrap(), 30_000, 0).unwrap();
    assert_eq!(lock.try_unlock(PASSWORD, 1), UnlockResult::Unlocked);
    lock.lock();
    assert!(lock.is_locked());
    assert_eq!(
        lock.try_unlock(b"wrong passphrase", 2),
        UnlockResult::Denied
    );
    let reboot = LockController::new(Credential::decode(&record).unwrap(), 30_000, 0).unwrap();
    assert!(reboot.is_locked());
}

#[test]
fn settings_restore_without_treating_corruption_as_disabled() {
    let credential = Credential::create(PASSWORD, SALT).unwrap();
    let settings = LockSettings::enabled(credential, 30_000).unwrap();
    let record = settings.encode();
    let restored = LockSettings::decode(&record).unwrap();
    let mut after_reboot = restored.into_controller(0).unwrap().unwrap();
    assert!(after_reboot.is_locked());
    assert_eq!(after_reboot.try_unlock(PASSWORD, 0), UnlockResult::Unlocked);
    after_reboot.tick(30_000);
    assert!(after_reboot.is_locked());

    let mut corrupted = record;
    corrupted[0] ^= 1;
    assert!(LockSettings::decode(&corrupted).is_err());
    assert!(LockSettings::decode(&record[..record.len() - 1]).is_err());
    let disabled = LockSettings::disabled().encode();
    assert!(
        LockSettings::decode(&disabled)
            .unwrap()
            .into_controller(0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn system_record_retains_existing_value_and_rejects_malformed_lock_settings() {
    let settings = LockSettings::enabled(Credential::create(PASSWORD, SALT).unwrap(), 30_000).unwrap().encode();
    let record = encode_system(0x1234_5678_9abc_def0, &settings).unwrap();
    let (value, lock) = decode_system(&record).unwrap();
    assert_eq!(value, 0x1234_5678_9abc_def0);
    assert_eq!(lock, settings);
    let mut bad = record;
    bad[16] ^= 1;
    assert!(decode_system(&bad).is_err());
    assert!(decode_system(&record[..record.len() - 1]).is_err());
}

#[test]
fn lock_system_payload_requires_a_committed_authenticated_vault_slot() {
    use crate::vault_blob_record;
    use crate::vault_crypto::TestCryptoProvider;
    let crypto = TestCryptoProvider::new([42; 32]);
    let settings = LockSettings::enabled(Credential::create(PASSWORD, SALT).unwrap(), 30_000).unwrap().encode();
    let payload = encode_system(0x1234, &settings).unwrap();
    let mut staged = vault_blob_record::encode(&crypto, 0, 7, &payload, false).unwrap().unwrap();
    assert!(vault_blob_record::decode(&crypto, 0, &mut staged).unwrap().is_none());
    assert!(vault_blob_record::set_committed(&mut staged));
    let mut corrupted = staged;
    corrupted[vault_blob_record::CIPHERTEXT_OFFSET + 16] ^= 1;
    assert!(vault_blob_record::decode(&crypto, 0, &mut corrupted).unwrap().is_none());
    let decoded = vault_blob_record::decode(&crypto, 0, &mut staged).unwrap().unwrap();
    assert_eq!(decoded.generation, 7);
    let start = vault_blob_record::CIPHERTEXT_OFFSET;
    let (value, restored) = decode_system(&staged[start..start + decoded.payload_length]).unwrap();
    assert_eq!(value, 0x1234);
    assert_eq!(restored, settings);
}
