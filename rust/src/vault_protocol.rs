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
pub const CALENDAR_APP_CHANNEL: Channel = Channel::new(5);
pub const MESSAGES_APP_CHANNEL: Channel = Channel::new(6);
/// Dedicated AgentStore caller; shares the Messages namespace during bring-up.
pub const SIMPLEX_AGENT_CHANNEL: Channel = Channel::new(7);
/// NexaWal wallet app — vault file I/O for WalletState (not a legacy namespace slot).
pub const WALLET_APP_CHANNEL: Channel = Channel::new(8);
/// Read-only preview worker endpoint; uses its own transfer page.
pub const PHOTO_READER_CHANNEL: Channel = Channel::new(9);
#[cfg(feature = "utm-device-lock")]
pub const DEVICE_LOCK_CHANNEL: Channel = Channel::new(10);
#[cfg(feature = "utm-device-lock")]
pub const REQUEST_DEVICE_LOCK_READ: MessageLabel = 0x450;
#[cfg(feature = "utm-device-lock")]
pub const REQUEST_DEVICE_LOCK_WRITE: MessageLabel = 0x451;
#[cfg(feature = "utm-wrapped-vault-experiment")]
pub const REQUEST_DEVICE_VAULT_UNLOCK: MessageLabel = 0x452;
#[cfg(feature = "utm-wrapped-vault-experiment")]
pub const REQUEST_DEVICE_VAULT_LOCK: MessageLabel = 0x453;
/// Test-only status exposed only to the SimpleX agent startup gate.
#[cfg(feature = "utm-wrapped-vault-experiment")]
pub const REQUEST_DEVICE_VAULT_STATUS: MessageLabel = 0x454;
/// Private manager-only endpoint in the disposable dynamic-lock fixture.
#[cfg(feature = "utm-dynamic-vault-lock")]
pub const VAULT_LIFECYCLE_CHANNEL: Channel = Channel::new(12);
#[cfg(feature = "utm-dynamic-vault-lock")]
pub const REQUEST_VAULT_COMMIT: MessageLabel = 0x455;
#[cfg(feature = "utm-dynamic-vault-lock")]
pub const REQUEST_VAULT_ARM: MessageLabel = 0x456;
// Local app endpoint shared by the supervisor's explicit namespace test calls.
pub const APP_TEST_SUPERVISOR_CHANNEL: Channel = Channel::new(1);
pub const NAMESPACE_COUNT: usize = 5;
pub const SYSTEM_NAMESPACE: usize = 0;
pub const NOTES_NAMESPACE: usize = 1;
pub const CONTACTS_NAMESPACE: usize = 2;
pub const CALENDAR_NAMESPACE: usize = 3;
pub const MESSAGES_NAMESPACE: usize = 4;

pub const REQUEST_READ: MessageLabel = 0x301;
pub const REQUEST_WRITE: MessageLabel = 0x302;
/// Test-only request that advances the isolated storage-recovery scenario.
pub const REQUEST_TEST_ADVANCE: MessageLabel = 0x308;
pub const REQUEST_NAMESPACE_TEST: MessageLabel = 0x309;
pub const REQUEST_READ_BLOB: MessageLabel = 0x30b;
pub const REQUEST_WRITE_BLOB: MessageLabel = 0x30c;
/// Messages app fixed-file service: MR0=file ID, MR1=offset, MR2=length.
pub const REQUEST_FILE_READ: MessageLabel = 0x430;
pub const REQUEST_FILE_WRITE: MessageLabel = 0x431;
/// MR0=file ID, MR1=new size.
pub const REQUEST_FILE_TRUNCATE: MessageLabel = 0x432;
/// MR0=file ID; flush data then durably commit file metadata.
pub const REQUEST_FILE_SYNC: MessageLabel = 0x433;
/// MR0=file ID; reply MR0=size, MR1=exists.
pub const REQUEST_FILE_SIZE: MessageLabel = 0x434;
/// MR0=file ID; rollback-journal deletion is persisted immediately.
pub const REQUEST_FILE_DELETE: MessageLabel = 0x435;

// Messages-only encrypted JPEG snapshot. Stage chunks in order, then commit.
pub const REQUEST_PHOTO_STAGE: MessageLabel = 0x436;
pub const REQUEST_PHOTO_COMMIT: MessageLabel = 0x437;
pub const REQUEST_PHOTO_READ: MessageLabel = 0x438;
pub const PHOTO_CHUNK: usize = 3072;
/// Physical slot geometry is part of the existing disk format.
pub const PHOTO_SLOT_CHUNKS: usize = 4;
/// Attachments may occupy multiple individually authenticated legacy slots.
#[cfg(not(feature="utm-files-picker"))]
pub const PHOTO_MAX: usize = 1024 * 1024;
#[cfg(feature="utm-files-picker")]
pub const PHOTO_MAX: usize = 8 * 1024 * 1024 + 4096;

/// Append a history event. MR0=chat handle, payload=event. Reply MR0=record ID.
pub const REQUEST_HISTORY_APPEND: MessageLabel = 0x439;
/// MR0=record ID. Reply MR0=chat handle, MR1=payload length.
pub const REQUEST_HISTORY_READ: MessageLabel = 0x43a;
/// Reply MR0=highest reserved record ID. Reserved IDs may have gaps.
pub const REQUEST_HISTORY_END: MessageLabel = 0x43b;
/// Reclaim photo extents owned by the deleted chat in MR0.
pub const REQUEST_PHOTO_PURGE_CHAT: MessageLabel = 0x43c;
/// Commit staged file bytes in the same owner-scoped extent as photos.
pub const REQUEST_ATTACHMENT_COMMIT: MessageLabel = 0x43d;
/// Read the wallet identity on the private vault-to-wallet page.
pub const REQUEST_WALLET_IDENTITY_READ: MessageLabel = 0x43e;
/// Provision an identity once. The input uses the same private page.
pub const REQUEST_WALLET_IDENTITY_CREATE: MessageLabel = 0x43f;
pub const REQUEST_WALLET_TX_STAGE: MessageLabel = 0x440;
pub const REQUEST_WALLET_TX_COMMIT: MessageLabel = 0x441;
pub const REQUEST_WALLET_TX_READ: MessageLabel = 0x442;
pub const REQUEST_WALLET_TX_CLEAR: MessageLabel = 0x443;
/// Authenticated node endpoint on the private wallet transfer page.
pub const REQUEST_WALLET_NODE_READ: MessageLabel = 0x445;
pub const REQUEST_WALLET_NODE_WRITE: MessageLabel = 0x446;
/// PIN envelope stored separately from the legacy identity during migration.
pub const REQUEST_WALLET_PIN_READ: MessageLabel = 0x447;
pub const REQUEST_WALLET_PIN_CREATE: MessageLabel = 0x448;
/// Copy an existing stored attachment into the independent file library.
/// MR0=source attachment ID, MR1=source chat owner, transfer payload=UTF-8 filename.
pub const REQUEST_LIBRARY_CLONE: MessageLabel = 0x449;
/// Find the next library entry after MR0 (a history cursor).
/// Reply MR0=next cursor, MR1=library attachment ID, MR2=file length;
/// transfer payload=UTF-8 filename.
pub const REQUEST_LIBRARY_NEXT: MessageLabel = 0x44a;
/// Remove the library-owned attachment in MR0 and reclaim its extents.
pub const REQUEST_LIBRARY_DELETE: MessageLabel = 0x44b;
/// Commit staged bytes as a library file. MR0=length, payload=UTF-8 filename.
pub const REQUEST_LIBRARY_IMPORT: MessageLabel = 0x44c;
/// Copy a library file to a chat-owned attachment. MR0=file ID, MR1=chat owner.
pub const REQUEST_LIBRARY_COPY_TO_CHAT: MessageLabel = 0x44d;
/// Read a bounded range of authenticated history records. MR0=start, MR1=end
/// from a prior HISTORY_END; reply MR0=record count, MR1=payload length.
pub const REQUEST_HISTORY_READ_BATCH: MessageLabel = 0x44e;
pub const LIBRARY_OWNER: u64 = u64::MAX;
pub const WALLET_TX_CHUNK: usize = 3072;
pub const WALLET_TX_MAX: usize = 4 * WALLET_TX_CHUNK;
/// Version, network, canonical spend and view keys, and little-endian restore height.
pub const WALLET_IDENTITY_SIZE: usize = 1 + 1 + 32 + 32 + 8;

pub const REPLY_OK: MessageLabel = 0x300;
pub const REPLY_BAD_REQUEST: MessageLabel = 0x303;
pub const REPLY_DENIED: MessageLabel = 0x304;
pub const REPLY_CONFLICT: MessageLabel = 0x305;
pub const REPLY_CORRUPT: MessageLabel = 0x306;
pub const REPLY_STORAGE: MessageLabel = 0x307;
pub const REPLY_NAMESPACE_TEST: MessageLabel = 0x30a;
pub const REPLY_NO_BLOB: MessageLabel = 0x30d;

/// Client → Messages: seal the fixed PSL1 demo datagram and hand it to dhcp_client.
pub const REQUEST_SEND_SEAL_DEMO: MessageLabel = 0x410;
pub const REPLY_SEND_SEAL_DEMO_OK: MessageLabel = 0x411;
pub const REPLY_NO_NET: MessageLabel = 0x412;
/// Messages → dhcp_client: transmit the shared-page datagram to the host.
pub const REQUEST_NET_TX: MessageLabel = 0x413;
pub const REPLY_NET_TX_OK: MessageLabel = 0x414;
/// Client → Messages: drive the UI compose→seal path and TX the sealed packet.
pub const REQUEST_SEND_UI_SEALED: MessageLabel = 0x415;
pub const REPLY_SEND_UI_SEALED_OK: MessageLabel = 0x416;
/// Client → Messages: lock sealed-TX peer IP/port on the shared net page.
/// MR0 = IPv4 as `a | b<<8 | c<<16 | d<<24`, MR1 = UDP port (host endian).
pub const REQUEST_SET_NET_PEER: MessageLabel = 0x417;
pub const REPLY_SET_NET_PEER_OK: MessageLabel = 0x418;

pub const INITIAL_VALUE: MessageRegisterValue = 0;
pub const TEST_VALUE: MessageRegisterValue = 0x4e4f_5445;
pub const DENIED_WRITE_VALUE: MessageRegisterValue = 0xdead_beef;
