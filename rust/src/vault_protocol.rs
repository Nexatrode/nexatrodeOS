//! Wire labels for the private-data service.
#![allow(dead_code)] // Each protection-domain binary uses only part of this wire API.

use sel4_microkit::{Channel, MessageLabel, MessageRegisterValue};

// These are the local endpoint IDs in the receiving protection domains.
pub const OWNER_CHANNEL: Channel = Channel::new(1);
pub const CLIENT_CHANNEL: Channel = Channel::new(6);
// This is the crasher's local endpoint ID; the corresponding vault-side ID is 2.
pub const CRASHER_CHANNEL: Channel = Channel::new(1);
// Vault-side endpoints bind app capabilities to fixed disk namespaces.
pub const NOTES_APP_CHANNEL: Channel = Channel::new(3);
pub const CONTACTS_APP_CHANNEL: Channel = Channel::new(4);
// Local app endpoint shared by the supervisor's explicit namespace test calls.
pub const APP_TEST_SUPERVISOR_CHANNEL: Channel = Channel::new(1);
pub const NAMESPACE_COUNT: usize = 3;
pub const SYSTEM_NAMESPACE: usize = 0;
pub const NOTES_NAMESPACE: usize = 1;
pub const CONTACTS_NAMESPACE: usize = 2;

pub const REQUEST_READ: MessageLabel = 0x301;
pub const REQUEST_WRITE: MessageLabel = 0x302;
/// Test-only request that advances the isolated storage-recovery scenario.
pub const REQUEST_TEST_ADVANCE: MessageLabel = 0x308;
pub const REQUEST_NAMESPACE_TEST: MessageLabel = 0x309;
pub const REQUEST_READ_BLOB: MessageLabel = 0x30b;
pub const REQUEST_WRITE_BLOB: MessageLabel = 0x30c;

pub const REPLY_OK: MessageLabel = 0x300;
pub const REPLY_BAD_REQUEST: MessageLabel = 0x303;
pub const REPLY_DENIED: MessageLabel = 0x304;
pub const REPLY_CONFLICT: MessageLabel = 0x305;
pub const REPLY_CORRUPT: MessageLabel = 0x306;
pub const REPLY_STORAGE: MessageLabel = 0x307;
pub const REPLY_NAMESPACE_TEST: MessageLabel = 0x30a;
pub const REPLY_NO_BLOB: MessageLabel = 0x30d;

pub const INITIAL_VALUE: MessageRegisterValue = 0;
pub const TEST_VALUE: MessageRegisterValue = 0x4e4f_5445;
pub const DENIED_WRITE_VALUE: MessageRegisterValue = 0xdead_beef;
