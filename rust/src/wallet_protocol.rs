//! Wire labels for the NexaWal wallet protection domain.
#![allow(dead_code)]

use sel4_microkit::{Channel, MessageLabel};

/// Vault-side endpoint for the wallet app (after simplex_agent on id 7).
pub const WALLET_VAULT_CHANNEL: Channel = Channel::new(8);
/// Wallet-local endpoint used for vault protected calls.
pub const WALLET_TO_VAULT_CHANNEL: Channel = Channel::new(0);
/// Wallet-local endpoint used by the Slint shell.
pub const WALLET_SHELL_CHANNEL: Channel = Channel::new(1);

/// Shell → wallet: report readiness / stub status. Reply carries no MRs yet.
pub const REQUEST_STATUS: MessageLabel = 0x501;
/// Shell → wallet: open/restore stub (step 2+).
pub const REQUEST_OPEN: MessageLabel = 0x502;
/// Shell → wallet: read balance stub (step 2+).
pub const REQUEST_BALANCE: MessageLabel = 0x503;
/// Shell → wallet: daemon get_info tip height (read-only sync probe).
pub const REQUEST_REFRESH: MessageLabel = 0x506;
/// Shell → wallet: prepare_send (dest/amount on transfer page; no broadcast yet).
pub const REQUEST_PREPARE_SEND: MessageLabel = 0x507;
/// Shell → wallet: broadcast pending prepared tx via daemon send_raw_transaction.
pub const REQUEST_SEND: MessageLabel = 0x508;
/// Begin wallet creation. MR0 is network (0 mainnet, 1 stagenet, 2 testnet),
/// MR1 is restore height. The caller must notify the wallet after this call.
pub const REQUEST_CREATE: MessageLabel = 0x509;
/// Verify the full mnemonic in the shell transfer page and queue its vault commit.
pub const REQUEST_CONFIRM_BACKUP: MessageLabel = 0x50a;
/// Discard an uncommitted creation and clear the transfer page.
pub const REQUEST_CANCEL_CREATE: MessageLabel = 0x50b;
/// Restore a mnemonic from the transfer page. MR0 is network, MR1 restore height.
pub const REQUEST_RESTORE: MessageLabel = 0x50c;
/// Lock the active wallet or unlock it using its mnemonic in the transfer page.
pub const REQUEST_LOCK: MessageLabel = 0x50d;
pub const REQUEST_UNLOCK: MessageLabel = 0x50e;
/// Query the configured daemon for the pending transaction ID.
pub const REQUEST_RECONCILE: MessageLabel = 0x50f;
pub const REQUEST_SET_NODE: MessageLabel = 0x510;
pub const REQUEST_GET_NODE: MessageLabel = 0x511;
/// Revalidate an unsigned preview; the transfer page must contain its exact request.
pub const REQUEST_VALIDATE_PREVIEW: MessageLabel = 0x512;
/// Test-network only: confirm, revalidate, persist and relay a signed preview.
pub const REQUEST_SIGN_TEST_PREVIEW: MessageLabel = 0x513;

pub const REPLY_OK: MessageLabel = 0x500;
pub const REPLY_BAD_REQUEST: MessageLabel = 0x504;
pub const REPLY_DENIED: MessageLabel = 0x505;
