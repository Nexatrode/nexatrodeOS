#![no_std]
#![no_main]

mod app_view;
mod entropy_protocol;
mod wallet_protocol;
mod wallet_scan_protocol;
#[cfg(feature = "wallet-scan-workers")]
mod wallet_scan_pool;
#[cfg(feature = "wallet-scan-workers")]
mod wallet_scan_mailbox;
mod wallet_tcp;
#[cfg(feature = "wallet-sync-timer")]
mod wallet_bulk;
#[cfg(feature = "wallet-range-profile")]
mod wallet_profile_clock;
#[cfg(feature = "wallet-sync-timer")]
mod wallet_range_plan;
#[cfg(feature = "wallet-sync-timer")]
mod wallet_range_fetch;
#[cfg(feature = "wallet-https")]
mod wallet_tls_platform;

extern crate alloc;

use alloc::string::String;
use zeroize::Zeroizing;
use alloc::vec::Vec;
use app_view::AppViewState;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr,
    memory_region_symbol, protection_domain, set_mr,
};
use wallet_protocol::{
    REPLY_BAD_REQUEST, REPLY_OK, REQUEST_BALANCE, REQUEST_OPEN, REQUEST_PREPARE_SEND,
    REQUEST_REFRESH, REQUEST_SEND, REQUEST_STATUS, REQUEST_CREATE, REQUEST_CONFIRM_BACKUP,
    REQUEST_CANCEL_CREATE, REQUEST_RESTORE, REQUEST_LOCK, REQUEST_UNLOCK, REQUEST_RECONCILE, WALLET_SHELL_CHANNEL,
};
use wallet_scan_protocol::{
    REPLY_WORKER_OK, REQUEST_WORKER_ARM_BLOCK, REQUEST_WORKER_PING, SCAN_JOB_MAGIC,
    ScanJobPage, WALLET_TO_WORKER_CHANNEL,
};
use wallet_tcp::{
    WalletTcp, build_json_rpc_http, http_body, http_response_complete, json_object_complete,
};
use walletcore_core::{
    Network, NodeEndpoint, RestoreChain, PendingChainEvidence, WalletJournal, HistoryObservation, JournalPlan, plan_journal_reconciliation, RestoredOutput, RestoredActivityOutput, restored_activity, restored_balances, output_key_image, daemon_network_matches, WalletIdentity, PendingTransaction, PendingObservation, observe_pending_transaction, PrepareError, RING_LEN, RingMember, SCAN_BLOCKS, SendError, TX_BATCH, WalletState, FeeRate, build_fee_estimate_body, parse_normal_fee_rate, build_spent_status_body, DistributionDownload, OutputDistribution, build_output_distribution_body, DecoyRingSession, select_restored_inputs, build_get_outs_batch_body, PaymentPreview, create_payment_preview, PaymentRevalidation, sign_revalidated_preview,
    balance_atomic, build_get_block_body, build_get_outs_body, build_get_output_histogram_body, build_get_pruned_transactions_body,
    build_get_transactions_body, build_http_post, build_send_raw_body, decode_block_blob,
    get_info_request_body, json_output_index_lists, json_string_array,
    json_string_value, parse_daemon_height, parse_get_outs_key_mask,
    parse_get_outs_ok, parse_histogram_total, parse_send_raw_ok, parse_spent_statuses, parse_tx_batch,
    parse_tx_batch_verified, strict_tx_request_range, http_response_status, next_tls_stall_polls, scan_assembled_strict, decode_hex32,
    first_output_key_image, prepare_and_sign, prepare_send, refresh_stub, scan_assembled,
    send_prepared,
};
use walletcore_core::{Block, Pruned, Transaction};

// Use the mapped region as a reusable heap. Long scans allocate and free each
// block's temporary data, so a bump allocator would eventually exhaust it.
// UTM's wallet PD maps a dedicated 32 MiB region. Keep this in sync with
// simplex-agent-slint-qemu.system; the larger arena accommodates long chains.
const HEAP_BYTES: usize = 32 * 1024 * 1024;
static HEAP_USED: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wallet-sync-timer-test")]
static TIMER_TEST_START: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wallet-sync-timer-test")]
static TIMER_TEST_PHASE: AtomicUsize = AtomicUsize::new(0);
type HeapMutex = sel4_microkit::_private::heap::RawOneShotMutex;
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<HeapMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();

fn heap_used_bytes() -> usize { HEAP_USED.load(Ordering::Relaxed) }

fn heap_init_from_mr() {
    let base = memory_region_symbol!(wallet_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(base.as_ptr(), HEAP_BYTES))
        .expect("wallet heap initialized once");
}

struct BootHeap;
unsafe impl GlobalAlloc for BootHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { HEAP.alloc(layout) };
        if !ptr.is_null() { HEAP_USED.fetch_add(layout.size(), Ordering::Relaxed); }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { HEAP.dealloc(ptr, layout) };
        HEAP_USED.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: BootHeap = BootHeap;

const WORKER: Channel = WALLET_TO_WORKER_CHANNEL;
#[cfg(feature = "wallet-https")]
const TLS_ENTROPY_CHANNEL: Channel = Channel::new(4);
#[cfg(feature = "wallet-https")]
const TLS_CLOCK_CHANNEL: Channel = Channel::new(5);
#[cfg(feature = "wallet-sync-timer")]
const SYNC_TIMER_CHANNEL: Channel = Channel::new(6);
#[cfg(any(feature = "utm-nexawal-rpc-bridge", feature = "wallet-https"))]
const DEFAULT_NODE: &str = "rpc.nexawal.com:443";
#[cfg(not(any(feature = "utm-nexawal-rpc-bridge", feature = "wallet-https")))]
const DEFAULT_NODE: &str = "xmr-node.cakewallet.com:18081";

#[protection_domain]
fn init() -> WalletService {
    heap_init_from_mr();
    let view = memory_region_symbol!(wallet_view: *mut AppViewState);
    let blob_transfer = memory_region_symbol!(blob_transfer: *mut BlobTransferPage);
    let scan_job = memory_region_symbol!(scan_job: *mut ScanJobPage);
    let identity_transfer = *sel4_microkit::var!(wallet_identity_transfer: usize = 0)
        as *mut BlobTransferPage;
    unsafe {
        view.as_ptr().write(AppViewState::new());
        blob_transfer.as_ptr().write(BlobTransferPage::new());
        scan_job.as_ptr().write(ScanJobPage {
            magic: SCAN_JOB_MAGIC,
            height: 0,
            status: ScanJobPage::STATUS_IDLE,
            outs: 0,
        });
    }
    let mut service = WalletService {
        view,
        transfer: blob_transfer,
        identity_transfer,
        scan_job,
        state: WalletState::closed(),
        pending_creation: None,
        creation_request: None,
        creation_height_lookup: None,
        restore_request: None,
        unlock_request: None,
        user_locked: false,
        commit_creation: false,
        pending_durable: false,
        journal: None,
        pending_evidence: None,
        node: NodeEndpoint::parse(DEFAULT_NODE).unwrap(),
        node_ip: DAEMON_IP,
        node_verified: false,
        fee_rate: None,
        distribution_request_pending: false,
        ring_request_pending: false,
        distribution_download: None,
        distribution: None,
        ring_session: None,
        restored_rings: Vec::new(),
        ring_targets: Vec::new(),
        ring_target_cursor: 0,
        ring_dest: String::new(),
        ring_amount: 0,
        ring_max: false,
        payment_preview: None,
        validation_pending: false,
        validation_sign_after: false,
        payment_validation: None,
        validation_statuses: Vec::new(),
        validation_batch_count: 0,
        ring_input_index: 0,
        pending_node: None,
        tcp: None,
        #[cfg(feature = "wallet-https")]
        tls_session: None,
        #[cfg(feature = "wallet-sync-timer")]
        bulk: None,
        #[cfg(feature = "wallet-sync-timer")]
        bulk_begin: None,
        #[cfg(feature = "wallet-range-profile")]
        range_profile: wallet_bulk::RangeProfile::default(),
        #[cfg(feature = "wallet-sync-timer")]
        bulk_unsupported: false,
        sync_phase: SyncPhase::Idle,
        sync_polls: 0,
        sync_req: [0u8; 16384],
        sync_req_len: 0,
        sync_resp: Vec::new(),
        sync_resp_len: 0,
        sync_rpc: SyncRpcKind::GetInfo,
        sync_recv_retries: 0,
        #[cfg(feature = "wallet-sync-timer")]
        sync_rate_retries: 0,
        #[cfg(feature = "wallet-sync-timer")]
        sync_timer_restart: false,
        sync_recv_stall: 0,
        scan_height: 0,
        restore_chain: None,
        verify_restore_anchor: false,
        seed_restore_anchor: false,
        anchor_after_spent_checks: false,
        restored_outputs: Vec::new(),
        history_observations: Vec::new(),
        restore_check_index: 0,
        restore_check_count: 0,
        scan_miner_hash: None,
        scan_blocks_left: 0,
        test_scan_i: 0,
        scan_hashes: Vec::new(),
        scan_batch: 0,
        scan_full_fallback: false,
        scan_full_fallback_offset: 0,
        scan_block: None,
        scan_txs: Vec::new(),
        /// First global RCT index for each entry in `scan_txs` (from get_transactions).
        scan_tx_first_idx: Vec::new(),
        scan_first_rct: None,
        decoy_out_index: 0,
        decoy_queue: Vec::new(),
        decoy_ring: Vec::new(),
        tip_locked: false,
        daemon_tip_height: 0,
        decoy_rct_total: 0,
        open_pending: false,
        refresh_pending: false,
        tcp_open_sent: false,
        deposit_hint_spent: false,
        change_hint_spent: false,
        later_hint_armed: false,
    };
    // Do not touch vault during init: a hung vault PPC would leave this
    // passive PD unable to serve the shell. Restore on first OPEN instead.
    if !service.publish_status() {
        debug_println!("UIAPP|WALLET|ERROR: invalid wallet display snapshot");
    }
    debug_println!("UIAPP|WALLET|READY: nexawal + monero-wallet");
    #[cfg(feature = "wallet-compact-batch-test")]
    {
        let hashes: Vec<String> = [
            "4b86310b2b9bc243dd2ec0847db0cbf381f3061aea88135f41040dfc3745cc62",
            "123947a7f124c441d1c3f6d435667d192086761b2e32a062cf124ed718053647",
            "185d2b4bb0d7c860fb2a0787cbc802426ef5129ff63a565c972ba0d1f3a84463",
            "4c1703b8ffcca6761ab8b2b8ea55cb16a0ef46939e91d99e1f072d78431f5e61",
            "6c1312a0fa981637cc5267406bd5603351a3f605a53e890648889783f1984bb1",
            "9b03867ceae394d663e35bb8a4949ae77d71c1318c0535b3411872e95ab4cbdd",
            "603ed28eadf35d800900761f758edbd236c8e8549d06bbe43769f09fdd11f2c4",
            "4b92114fd9e4119bedae92267313517b6d4db3386b79a9e827e37426ac2bbbfa",
        ].iter().map(|s| String::from(*s)).collect();
        let compact = parse_tx_batch_verified(include_bytes!("../walletcore-core/tests/fixtures/mainnet-3806204-pruned8.json"), &hashes);
        let full = parse_tx_batch_verified(include_bytes!("../walletcore-core/tests/fixtures/mainnet-3806204-full8.json"), &hashes);
        let ok = match (compact, full) {
            (Some(a), Some(b)) if a.len() == 8 && b.len() == 8 => a.iter().zip(b.iter()).all(|(x,y)| x.0.serialize() == y.0.serialize() && x.1 == y.1),
            _ => false,
        };
        debug_println!("UIAPP|WALLET|COMPACT EIGHT TEST {}", if ok { "PASS" } else { "FAIL" });
    }
    service
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SyncPhase {
    Idle,
    /// Prep + TCP OPEN once, then WaitConnect.
    Start,
    /// One STATUS poll per shell notify (lets dhcp run RX/timer).
    WaitConnect,
    /// Send HTTP body already in sync_req.
    Send,
    /// One RECV poll per notify until complete/closed.
    Recv,
    /// Tip scan finished; drive scan_worker get_block over its own dhcp slot.
    WaitWorker,
    #[cfg(feature = "wallet-sync-timer")]
    WaitTimer,
    #[cfg(feature = "wallet-sync-timer")]
    Range,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SyncRpcKind {
    GetInfo,
    GetBlock,
    GetMinerIndices,
    GetTxBatch,
    /// Decoy plumbing: rct pool size via get_output_histogram.
    GetHistogram,
    GetFeeEstimate,
    GetDistribution,
    GetDistributionAnchor,
    GetRingBatch,
    ValidateInfo,
    ValidateAnchorBefore,
    ValidateSpent,
    ValidateAnchorAfter,
    ValidateFee,
    /// Decoy plumbing: sample one absolute index via /get_outs.
    GetOuts,
    /// Broadcast signed hex via `/send_raw_transaction`.
    SendRaw,
    Reconcile,
    /// Drop spent outs before prepare (`/is_key_image_spent`).
    IsKeyImageSpent,
    RestoreSpent,
}

#[cfg(feature = "wallet-https")]
struct RpcTls {
    session: nexatrode_guest_https::Session,
    preflight: Option<(NodeEndpoint, [u8; 4], nexatrode_guest_https::binary_response::BufferedRpcResponse)>,
}

struct WalletService {
    view: NonNull<AppViewState>,
    transfer: NonNull<BlobTransferPage>,
    identity_transfer: *mut BlobTransferPage,
    scan_job: NonNull<ScanJobPage>,
    state: WalletState,
    pending_creation: Option<(WalletIdentity, Zeroizing<String>)>,
    creation_request: Option<(Network, u64)>,
    creation_height_lookup: Option<Network>,
    restore_request: Option<(Network, u64, Zeroizing<String>)>,
    unlock_request: Option<Zeroizing<String>>,
    user_locked: bool,
    commit_creation: bool,
    pending_durable: bool,
    journal: Option<WalletJournal>,
    pending_evidence: Option<PendingChainEvidence>,
    node: NodeEndpoint,
    node_ip: [u8; 4],
    node_verified: bool,
    fee_rate: Option<FeeRate>,
    distribution_request_pending: bool,
    ring_request_pending: bool,
    distribution_download: Option<DistributionDownload>,
    distribution: Option<OutputDistribution>,
    ring_session: Option<DecoyRingSession>,
    restored_rings: Vec<(usize, Vec<RingMember>)>,
    ring_targets: Vec<usize>,
    ring_target_cursor: usize,
    ring_dest: String,
    ring_amount: u64,
    ring_max: bool,
    payment_preview: Option<PaymentPreview>,
    validation_pending: bool,
    validation_sign_after: bool,
    payment_validation: Option<PaymentRevalidation>,
    validation_statuses: Vec<u8>,
    validation_batch_count: usize,
    ring_input_index: usize,
    pending_node: Option<NodeEndpoint>,
    tcp: Option<WalletTcp>,
    #[cfg(feature = "wallet-https")]
    tls_session: Option<RpcTls>,
    #[cfg(feature = "wallet-sync-timer")]
    bulk: Option<wallet_bulk::BulkScan>,
    #[cfg(feature = "wallet-sync-timer")]
    bulk_begin: Option<(u64, u64)>,
    #[cfg(feature = "wallet-range-profile")]
    range_profile: wallet_bulk::RangeProfile,
    #[cfg(feature = "wallet-sync-timer")]
    bulk_unsupported: bool,
    /// Advanced from notified() so dhcp PPC is not nested under shell Call and
    /// connect-wait does not STATUS-spin (that starved SYN-ACK on B7E4).
    sync_phase: SyncPhase,
    sync_polls: u32,
    sync_req: [u8; 16384],
    sync_req_len: usize,
    sync_resp: Vec<u8>,
    sync_resp_len: usize,
    sync_rpc: SyncRpcKind,
    /// TCG/dhcp RECV flakes during dense tip tx batches — reconnect same req.
    sync_recv_retries: u8,
    #[cfg(feature = "wallet-sync-timer")]
    sync_rate_retries: u8,
    #[cfg(feature = "wallet-sync-timer")]
    sync_timer_restart: bool,
    /// Recv polls with no byte growth (keep-alive / missing FIN stall).
    sync_recv_stall: u32,
    scan_height: u64,
    /// Next historical block, inclusive. Only advanced after a complete scan.
    restore_chain: Option<RestoreChain>,
    verify_restore_anchor: bool,
    seed_restore_anchor: bool,
    anchor_after_spent_checks: bool,
    restored_outputs: Vec<RestoredOutput>,
    history_observations: Vec<HistoryObservation>,
    restore_check_index: usize,
    restore_check_count: usize,
    scan_miner_hash: Option<String>,
    /// Remaining tip-window blocks to fetch after the current one completes.
    scan_blocks_left: u64,
    /// Next index into `TEST_SCAN_HEIGHTS` for the test restore wallet.
    test_scan_i: usize,
    scan_hashes: Vec<String>,
    scan_batch: usize,
    /// Retry a batch with complete transactions if compact data cannot verify.
    scan_full_fallback: bool,
    scan_full_fallback_offset: usize,
    scan_block: Option<Block>,
    scan_txs: Vec<Transaction<Pruned>>,
    /// Parallel to `scan_txs`: first global output index from get_transactions.
    scan_tx_first_idx: Vec<u64>,
    scan_first_rct: Option<u64>,
    /// Absolute rct index for /get_outs decoy probe.
    decoy_out_index: u64,
    /// Remaining absolute indices to fetch for the CLSAG ring (excludes real).
    decoy_queue: Vec<u64>,
    /// Collected ring members (absolute index + key + mask).
    decoy_ring: Vec<RingMember>,
    decoy_rct_total: u64,
    /// Tip+scan label published — progress chatter must not overwrite transfer.
    tip_locked: bool,
    /// Daemon tip height from last get_info (for durable tip+scan labels).
    daemon_tip_height: u64,
    /// OPEN Call returns immediately; key derivation runs on the next notify
    /// so shell↔dhcp cannot deadlock under a nested OPEN PPC (dump222).
    open_pending: bool,
    /// REFRESH Call only arms this; close+Start+TCP run on notify (dump223).
    refresh_pending: bool,
    /// TCP OPEN issued for the current WaitConnect attempt (dump223/d80c96a1).
    tcp_open_sent: bool,
    /// DEPOSIT_HINT out's key image is spent — skip EARLY_FUNDED re-hit; tip-hunt for fresh funds.
    deposit_hint_spent: bool,
    /// CHANGE_HINT out's key image is spent too. Do not refetch that block.
    change_hint_spent: bool,
    /// Spent change at CHANGE_HINT; the next self-send's change is still unspent.
    /// Ignore refresh until that block is scanned.
    later_hint_armed: bool,
}

/// Public clearnet probe node (HTTP JSON-RPC). Replaceable via later config IPC.
/// Cake A-record (guest has no DNS). Host get_info OK; guest TCG still flaky.
const DAEMON_IP: [u8; 4] = [173, 255, 212, 53];

/// B7E4 mainnet test wallet (small dedicated funds). Keys only — mnemonic never
/// stored in-tree. Restore height from the operator.
#[cfg(feature = "wallet-mainnet-fixture")]
const TEST_RESTORE_HEIGHT: u64 = 3_519_450;
/// Host refresh receives (newest first). Tip-window scan misses these.
/// Do NOT put CHANGE_HINT_HEIGHT here — TEST heights force MINER_ONLY (drop
/// peers); the change outs live in peer txs.
const TEST_SCAN_HEIGHTS: &[u64] = &[
    3_794_104, 3_788_645, 3_777_587, 3_777_564, 3_775_801, 3_671_481,
];
/// First change after the H3803776 deposit was spent (`627bf7ab…`, peer 4).
/// That output was itself spent in the next self-send. Do not treat it as the
/// spendable balance.
const CHANGE_HINT_HEIGHT: u64 = 3_804_540;
/// Peer tx index of the change/self-send inside CHANGE_HINT_HEIGHT.
const CHANGE_TX_PEER_INDEX: usize = 4;
/// Change after the H3804540 output was spent (`d41b2c1f…`, peer 38).
/// Spent by the confirmed 0.001 send. Do not treat it as the balance.
const LATER_HINT_HEIGHT: u64 = 3_805_635;
/// Peer tx index of that change inside LATER_HINT_HEIGHT.
const LATER_TX_PEER_INDEX: usize = 38;
/// Confirmed 0.001 self-send (`8bea3eef…`, peer 14). Unspent change
/// 11307178818 plus the 0.001 receive, global indexes 167719278 and 167719279.
const POST_HINT_HEIGHT: u64 = 3_805_838;
/// Peer tx index of the confirmed send inside POST_HINT_HEIGHT.
const POST_TX_PEER_INDEX: usize = 14;
#[cfg(feature = "wallet-mainnet-fixture")]
const TEST_SPEND_SECRET_HEX: &[u8] =
    b"95accd6f6682cf9148941e54441a3b5e4c220c9a0fc536cc8e9299acc566ee0f";
#[cfg(feature = "wallet-mainnet-fixture")]
const TEST_VIEW_SECRET_HEX: &[u8] =
    b"43c75289deb8148a537b8b3f5eb5130d59650c4be44747eef1c6f01c8ad3b40d";
/// Expected primary: 4A1ZVG4SrnpMgcnr3EDTHz2rzprHZm9pq7sZKj3dNL6L5j9CFcVd3RK3yZFJm3XWW3W2YtCAUxb7yBJEvsnicvriSZVA5nq
#[cfg(feature = "wallet-mainnet-fixture")]
const TEST_ADDR_PREFIX: &[u8] = b"4A1ZVG4SrnpM";
// Tip-window predecessors after arming tip (GetInfo / hang-skip / test-done).
// dump220: tip64 missed deposit at tip-108; keep wide enough for unlock lag.
// Deposit HIT at H3803776; tip ~3804336 ⇒ need ≥560. Keep headroom for tip drift.
// Tip drifted past tip1280 (deposit 3803776 vs tip ~3805075 ⇒ ~1299 back).
const TIP_HUNT_BLOCKS_LEFT: u64 = 2560;
/// Unlocked deposit height (host HIT). Prefer this before walking tip→down so
/// tip768 TCG doesn't burn hours on tip-only blocks first.
const DEPOSIT_HINT_HEIGHT: u64 = 3_803_776;
/// TCG + shell notify chains can burn this before a 50ms RTT lands — keep high.
const SYNC_CONNECT_POLL_MAX: u32 = 8_000;

impl WalletService {
    fn clear_payment_validation(&mut self) {
        self.validation_pending = false;
        self.validation_sign_after = false;
        self.payment_validation = None;
        self.validation_statuses.clear();
        self.validation_batch_count = 0;
    }
    fn tip_label_showing(&self) -> bool {
        // r25: once EARLY_FUNDED sets balance_atomic, stop folding progress into
        // the TIP line — that path left the face without a parseable BAL for
        // apply_wallet_face (OCR stayed on Unlocked —). Prefer sticky BAL.
        if self.state.balance_atomic > 0 {
            return false;
        }
        self.tip_locked
            || unsafe { self.transfer.as_ref() }
                .payload()
                .is_some_and(|p| p.windows(6).any(|w| w == b"TIP "))
    }

    /// Durable tip+scan line (uses publish_bytes so it always lands).
    fn publish_tip_scan(&mut self, tip: u64, scan: u64, tag: &[u8]) {
        let mut msg = [0u8; 56];
        let p = b"NEXAWAL | TIP ";
        msg[..p.len()].copy_from_slice(p);
        let mut n = p.len();
        fn push_u64(msg: &mut [u8], n: &mut usize, mut v: u64) {
            let mut dig = [0u8; 12];
            let mut i = 0;
            if v == 0 {
                dig[0] = b'0';
                i = 1;
            } else {
                while v > 0 && i < dig.len() {
                    dig[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                    i += 1;
                }
            }
            while i > 0 && *n < msg.len() {
                i -= 1;
                msg[*n] = dig[i];
                *n += 1;
            }
        }
        push_u64(&mut msg, &mut n, tip);
        let mid = b" + SCAN ";
        if n + mid.len() < msg.len() {
            msg[n..n + mid.len()].copy_from_slice(mid);
            n += mid.len();
            push_u64(&mut msg, &mut n, scan);
        }
        if !tag.is_empty() && n + 1 + tag.len() <= msg.len() {
            msg[n] = b' ';
            n += 1;
            msg[n..n + tag.len()].copy_from_slice(tag);
            n += tag.len();
        }
        let _ = self.publish_bytes(&msg[..n]);
        self.tip_locked = true;
        self.daemon_tip_height = tip;
    }

    /// Progress chatter must not erase tip height; fold a short tag into the tip line.
    fn publish_progress(&mut self, text: &[u8]) -> bool {
        // Fee lookup is a follow-up to a completed scan; its network chatter
        // must not replace the scan result or payment recovery status.
        if self.sync_rpc == SyncRpcKind::GetFeeEstimate { return true; }
        if self.tip_label_showing() {
            // dump147: "CONNECTED" matched CONN→NET and looked identical to
            // WAIT NET, so a live Recv freeze looked like a connect hang.
            let tag: &[u8] = if text.windows(9).any(|w| w == b"CONNECTED") {
                b"TCP"
            } else if text.first() == Some(&b'R') {
                b"RX"
            } else if text.windows(5).any(|w| w == b"FETCH") {
                b"FETCH"
            } else if text.windows(4).any(|w| w == b"WAIT")
                || text.windows(11).any(|w| w == b"CONNECTING")
            {
                b"NET"
            } else if text.windows(4).any(|w| w == b"RECV") {
                b"RX"
            } else {
                return true;
            };
            let tip = if self.daemon_tip_height != 0 {
                self.daemon_tip_height
            } else {
                self.scan_height
            };
            self.publish_tip_scan(tip, self.scan_height, tag);
            return true;
        }
        self.publish_bytes(text)
    }

    fn publish_bytes(&mut self, text: &[u8]) -> bool {
        // Shell IPC reads wallet_transfer only. AppViewState::set_display_text
        // rejects '|' and lowercase (Contacts alphabet) — do not gate IPC on it.
        // Keep BAL visible when decoy/connect progress would otherwise wipe it
        // (EARLY_FUNDED → DECOY HIST CONNECTING left the face on Unlocked — XMR).
        let mut buf = [0u8; 512];
        let payload: &[u8] = if self.state.balance_atomic > 0
            && !text.windows(4).any(|w| w == b"BAL ")
            && text.len() + 24 < buf.len()
        {
            buf[..text.len()].copy_from_slice(text);
            let mut n = text.len();
            // " | BAL " is 7 bytes — was n+6 and panicked after EARLY_FUNDED.
            const BAL_TAG: &[u8] = b" | BAL ";
            buf[n..n + BAL_TAG.len()].copy_from_slice(BAL_TAG);
            n += BAL_TAG.len();
            let mut v = self.state.balance_atomic;
            let mut dig = [0u8; 20];
            let mut i = 0usize;
            if v == 0 {
                dig[0] = b'0';
                i = 1;
            } else {
                while v > 0 && i < dig.len() {
                    dig[i] = b'0' + (v % 10) as u8;
                    v /= 10;
                    i += 1;
                }
            }
            while i > 0 {
                i -= 1;
                buf[n] = dig[i];
                n += 1;
            }
            &buf[..n]
        } else if self.deposit_hint_spent
            && self.state.balance_atomic == 0
            && !text.windows(12).any(|w| w == b"NEED DEPOSIT")
            && text.len() + 28 < buf.len()
        {
            // r42/r50: tip-hunt after KI_SPENT overwrote NEED DEPOSIT before
            // shell painted — sticky-append even when the tip line already has
            // other tags (do not require absence of BAL).
            buf[..text.len()].copy_from_slice(text);
            let tag = b" | NEED DEPOSIT | BAL 0";
            let n = text.len();
            buf[n..n + tag.len()].copy_from_slice(tag);
            &buf[..n + tag.len()]
        } else {
            text
        };
        if payload.windows(4).any(|w| w == b"BAL ") {
            debug_println!("UIAPP|WALLET|PUBLISH_HAS_BAL len={}", payload.len());
        }
        let transfer_ok = unsafe { self.transfer.as_mut().set_payload(payload) };
        let mut view = [0u8; 128];
        let mut n = 0;
        for &b in payload.iter().take(view.len()) {
            view[n] = match b {
                b'a'..=b'z' => b - 32,
                b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-' => b,
                _ => b'-',
            };
            n += 1;
        }
        let _ = unsafe { self.view.as_mut().set_display_text(&view[..n]) };
        transfer_ok
    }

    fn publish_status(&mut self) -> bool {
        // Full ADDR (~95) + BAL/SCAN/HIST needs more than the old 128B tip line.
        let mut buf = [0u8; 512];
        let Some(n) = self.state.format_status(&mut buf) else {
            return false;
        };
        self.publish_bytes(&buf[..n])
    }

    fn publish_status_tag(&mut self, tag: &[u8]) -> bool {
        let mut buf = [0u8; 512];
        let Some(n) = self.state.format_status(&mut buf) else {
            return false;
        };
        if n + tag.len() > buf.len() {
            return self.publish_bytes(&buf[..n]);
        }
        buf[n..n + tag.len()].copy_from_slice(tag);
        self.publish_bytes(&buf[..n + tag.len()])
    }

    fn format_prepare_ok(buf: &mut [u8], fee: u64) -> usize {
        let prefix = b"NEXAWAL | PREPARE OK FEE ";
        if prefix.len() >= buf.len() {
            return 0;
        }
        buf[..prefix.len()].copy_from_slice(prefix);
        let mut n = prefix.len();
        let mut value = fee;
        let mut digits = [0u8; 20];
        let mut i = digits.len();
        if value == 0 {
            i -= 1;
            digits[i] = b'0';
        } else {
            while value > 0 && i > 0 {
                i -= 1;
                digits[i] = b'0' + (value % 10) as u8;
                value /= 10;
            }
        }
        let dig = &digits[i..];
        if n + dig.len() > buf.len() {
            return 0;
        }
        buf[n..n + dig.len()].copy_from_slice(dig);
        n + dig.len()
    }

    fn tcp_mut(&mut self) -> &mut WalletTcp {
        self.tcp.get_or_insert_with(WalletTcp::map)
    }

    fn status(&mut self) -> MessageInfo {
        if !self.publish_status() {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        MessageInfo::new(REPLY_OK, 0)
    }

    fn is_mainnet_fixture(&self) -> bool {
        #[cfg(feature = "wallet-mainnet-fixture")]
        {
            self.state.address().as_bytes().starts_with(TEST_ADDR_PREFIX)
        }
        #[cfg(not(feature = "wallet-mainnet-fixture"))]
        {
            false
        }
    }

    fn open_wallet(&mut self) -> bool {
        if self.state.open {
            return true;
        }
        if self.user_locked { return false; }
        #[cfg(feature = "wallet-mainnet-fixture")]
        {
            let fixture = (|| {
                use walletcore_core::decode_hex32;
                let spend = decode_hex32(TEST_SPEND_SECRET_HEX)?;
                let view = decode_hex32(TEST_VIEW_SECRET_HEX)?;
                let state = WalletState::open_from_key_bytes(
                    "mainnet-test", &spend, &view, TEST_RESTORE_HEIGHT,
                ).ok()?;
                state.address().as_bytes().starts_with(TEST_ADDR_PREFIX).then_some(state)
            })();
            if let Some(state) = fixture {
                self.state = state;
                debug_println!("UIAPP|WALLET|OPEN|FIXTURE");
                return true;
            }
        }
        let Some(stored) = self.read_wallet_identity() else { return false; };
        self.open_identity(stored)
    }

    fn request_creation(&mut self, network: Network, height: u64) -> MessageInfo {
        if self.state.open || self.creation_request.is_some() || self.creation_height_lookup.is_some() || self.restore_request.is_some()
            || self.pending_creation.is_some() {
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        self.creation_request = Some((network, height));
        let _ = self.publish_bytes(b"NEXAWAL | CREATING");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn generate_pending_wallet(&mut self, network: Network, height: u64) -> bool {
        const ENTROPY: Channel = Channel::new(4);
        let mut entropy = Zeroizing::new([0u8; 64]);
        for chunk in entropy.chunks_mut(entropy_protocol::SAMPLE_BYTES) {
            let reply = ENTROPY.pp_call(MessageInfo::new(entropy_protocol::REQUEST_BYTES, 0));
            if reply.label() != entropy_protocol::REPLY_OK || reply.count() != 2 {
                return false;
            }
            chunk[..8].copy_from_slice(&get_mr(0).to_le_bytes());
            chunk[8..].copy_from_slice(&get_mr(1).to_le_bytes());
        }
        let Ok((identity, words)) = WalletIdentity::generate_from_entropy(entropy, height, network) else {
            return false;
        };
        // This page is intentionally visible to the shell for seed backup.
        if !unsafe { self.transfer.as_mut() }.set_payload(words.as_bytes()) {
            return false;
        }
        self.pending_creation = Some((identity, words));
        true
    }

    fn request_restore(&mut self, network: Network, height: u64) -> MessageInfo {
        if self.state.open || self.creation_request.is_some() || self.restore_request.is_some()
            || self.pending_creation.is_some()
        {
            unsafe { self.transfer.as_mut() }.set_payload(&[]);
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        let words = unsafe { self.transfer.as_ref() }.payload()
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .filter(|words| !words.is_empty() && words.len() <= 1024)
            .map(|words| Zeroizing::new(String::from(words)));
        unsafe { self.transfer.as_mut() }.set_payload(&[]);
        let Some(words) = words else {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        };
        self.restore_request = Some((network, height, words));
        let _ = self.publish_bytes(b"NEXAWAL | RESTORING");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn confirm_backup(&mut self) -> MessageInfo {
        unsafe { self.transfer.as_mut() }.set_payload(&[]);
        if self.pending_creation.is_none() || self.commit_creation {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        self.commit_creation = true;
        let _ = self.publish_bytes(b"NEXAWAL | SAVING WALLET");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn persist_pending_identity(&mut self) -> Result<(), bool> {
        let Some((identity, _)) = self.pending_creation.as_ref() else {
            return Err(false);
        };
        let Some(page) = (unsafe { self.identity_transfer.as_mut() }) else {
            return Err(false);
        };
        let encoded = identity.encode();
        if !page.set_payload(&encoded[..]) {
            return Err(false);
        }
        let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x43f, 0));
        page.set_payload(&[]);
        if reply.label() != 0x300 || reply.count() != 0 {
            // A flush can report failure after the record reached the disk.
            // Accept a retry only when the committed identity is identical.
            let read = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x43e, 0));
            let matches = read.label() == 0x300 && read.count() == 0
                && page.payload() == Some(&encoded[..]);
            page.set_payload(&[]);
            if !matches {
                return Err(reply.label() == 0x305);
            }
        }
        // Keep the pending identity on failure. A successful commit clears
        // the mnemonic, while opening remains a separate authenticated flow.
        self.pending_creation = None;
        Ok(())
    }

    // Creation and restore must not invite a backup that cannot be saved.
    // A read error is not evidence that the identity slot is empty.
    fn identity_slot_available(&mut self) -> Result<bool, ()> {
        let Some(page) = (unsafe { self.identity_transfer.as_mut() }) else { return Err(()); };
        let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x43e, 0));
        page.set_payload(&[]);
        match reply.label() {
            0x300 if reply.count() == 0 => Ok(false),
            0x30d => Ok(true),
            _ => Err(()),
        }
    }

    fn persist_signed_transaction(&mut self) -> bool {
        let record = PendingTransaction {
            wallet_address: String::from(self.state.address()),
            tx_hex: Zeroizing::new(self.state.pending_tx_hex.clone()),
            fee: self.state.pending_fee_atomic,
            amount: self.state.pending_send_amount,
        };
        let Some(txid) = record.signed_payment_id() else { return false; };
        // Never replace an existing record until it has been read and bound
        // to this wallet, including when running the explicit fixture build.
        let Some(mut journal) = self.journal.clone() else { return false; };
        if journal.wallet_address != self.state.address() { return false; }
        if let Some(existing) = journal.pending.as_ref() {
            if existing.tx_hex.as_str() != record.tx_hex.as_str() || existing.fee != record.fee || existing.amount != record.amount {
                return false;
            }
        }
        journal.pending = Some(record.clone());
        if !self.persist_wallet_journal(&journal) { return false; }
        self.journal = Some(journal);
        self.pending_evidence = Some(PendingChainEvidence::new(txid));
        true
    }

    /// Write the entire pending/history snapshot before changing in-memory
    /// state. If a commit reply is ambiguous, accept it only after exact
    /// authenticated readback of the committed candidate.
    fn persist_wallet_journal(&mut self, journal: &WalletJournal) -> bool {
        let Some(bytes) = journal.encode() else { return false; };
        let Some(page) = (unsafe { self.identity_transfer.as_mut() }) else { return false; };
        for (part, chunk) in bytes.chunks(3072).enumerate() {
            if !page.set_payload(chunk) { return false; }
            set_mr(0, (part * 3072) as u64);
            let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x440, 1));
            page.set_payload(&[]);
            if reply.label() != 0x300 || reply.count() != 0 { return false; }
        }
        set_mr(0, bytes.len() as u64);
        let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x441, 1));
        if reply.label() == 0x300 && reply.count() == 0 { return true; }
        matches!(self.read_wallet_journal_bytes(), Ok(Some(readback)) if readback.as_slice() == bytes.as_slice())
    }

    fn read_wallet_journal_bytes(&mut self) -> Result<Option<Zeroizing<Vec<u8>>>, ()> {
        let Some(page) = (unsafe { self.identity_transfer.as_mut() }) else { return Err(()); };
        let mut bytes = Zeroizing::new(Vec::new());
        let mut total = 0usize;
        for part in 0..4 {
            set_mr(0, part);
            let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x442, 1));
            if part == 0 && reply.label() == 0x30d {
                page.set_payload(&[]);
                return Ok(None);
            }
            if reply.label() != 0x300 || reply.count() != if part == 0 { 1 } else { 0 } {
                page.set_payload(&[]); return Err(());
            }
            if part == 0 {
                total = get_mr(0) as usize;
                if total == 0 || total > WalletJournal::MAX_BYTES { page.set_payload(&[]); return Err(()); }
            }
            let valid = page.payload().filter(|chunk| chunk.len() == 3072.min(total - bytes.len()));
            let Some(chunk) = valid else { page.set_payload(&[]); return Err(()); };
            bytes.extend_from_slice(chunk);
            page.set_payload(&[]);
            if bytes.len() == total { return Ok(Some(bytes)); }
        }
        Err(())
    }

    fn restore_pending_transaction(&mut self) -> Result<(), ()> {
        let journal = match self.read_wallet_journal_bytes()? {
            Some(bytes) => WalletJournal::decode(&bytes).ok_or(())?,
            None => WalletJournal::empty(self.state.address()).ok_or(())?,
        };
        if journal.wallet_address != self.state.address() { return Err(()); }
        if let Some(record) = journal.pending.as_ref() {
            if !record.belongs_to(&self.state) { return Err(()); }
            let txid = record.signed_payment_id().ok_or(())?;
            self.pending_evidence = Some(PendingChainEvidence::new(txid));
            self.state.pending_tx_hex = String::from(record.tx_hex.as_str());
            self.state.pending_fee_atomic = record.fee;
            self.state.pending_send_amount = record.amount;
            self.pending_durable = true;
        }
        self.history_observations = alloc::vec![HistoryObservation::Unchecked; journal.sent.len()];
        self.journal = Some(journal);
        Ok(())
    }

    fn request_unlock(&mut self) -> MessageInfo {
        if self.state.open || self.unlock_request.is_some() || self.commit_creation
            || self.creation_request.is_some() || self.restore_request.is_some()
            || self.pending_creation.is_some()
        {
            unsafe { self.transfer.as_mut() }.set_payload(&[]);
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        let words = unsafe { self.transfer.as_ref() }.payload()
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .filter(|words| !words.is_empty() && words.len() <= 1024)
            .map(|words| Zeroizing::new(String::from(words)));
        unsafe { self.transfer.as_mut() }.set_payload(&[]);
        let Some(words) = words else {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        };
        self.unlock_request = Some(words);
        let _ = self.publish_bytes(b"NEXAWAL | UNLOCKING");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn unlock_identity(&mut self, words: Zeroizing<String>) -> bool {
        let Some(stored) = self.read_wallet_identity() else { return false; };
        let Ok(candidate) = WalletIdentity::from_mnemonic(words, stored.restore_height, stored.network) else {
            return false;
        };
        if candidate.encode()[..] != stored.encode()[..] { return false; }
        if self.open_identity(stored) {
            self.user_locked = false;
            true
        } else {
            false
        }
    }

    fn read_wallet_identity(&mut self) -> Option<WalletIdentity> {
        let Some(page) = (unsafe { self.identity_transfer.as_mut() }) else {
            return None;
        };
        let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x43e, 0));
        let stored = if reply.label() == 0x300 && reply.count() == 0 {
            page.payload().and_then(|bytes| WalletIdentity::decode(bytes).ok())
        } else {
            None
        };
        page.set_payload(&[]);
        stored
    }

    fn open_identity(&mut self, stored: WalletIdentity) -> bool {
        let Ok(state) = stored.open() else {
            return false;
        };
        self.state = state;
        self.restore_chain = Some(RestoreChain::new(stored.restore_height));
        self.verify_restore_anchor = false;
        self.seed_restore_anchor = false;
        self.anchor_after_spent_checks = false;
        self.restored_outputs.clear();
        self.restore_check_index = 0;
        if self.restore_pending_transaction().is_err() || self.restore_node().is_err() {
            self.state = WalletState::closed();
            self.pending_durable = false;
            return false;
        }
        // The journal may only fast-forward over a prefix in which no owned
        // output was found. Funded/history-bearing journals always rescan.
        if let Some(journal) = self.journal.as_ref() {
            if journal.permits_empty_scan_checkpoint(0, 0) {
                if let Some((next, hash)) = journal.empty_scan_checkpoint {
                    if let Some(chain) = RestoreChain::resume_empty(stored.restore_height, next, hash) {
                        self.restore_chain = Some(chain);
                        debug_println!("UIAPP|WALLET|EMPTY CHECKPOINT RESUMED {}", next);
                    }
                }
            }
        }
        true
    }

    fn rpc_busy_for_user_action(&self) -> bool {
        match self.sync_phase {
            SyncPhase::Idle => false,
            #[cfg(feature = "wallet-sync-timer")]
            SyncPhase::WaitTimer | SyncPhase::Range => false,
            _ => true,
        }
    }

    fn lock_wallet(&mut self) -> MessageInfo {
        self.clear_payment_validation();
        self.fee_rate = None;
        self.payment_preview = None;
        if self.commit_creation || self.pending_node.is_some()
            || (!self.state.pending_tx_hex.is_empty() && !self.pending_durable)
            || self.rpc_busy_for_user_action() {
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        #[cfg(feature = "wallet-sync-timer")]
        self.interrupt_history_scan();
        self.creation_request = None;
        self.restore_request = None;
        self.unlock_request = None;
        self.pending_creation = None;
        self.distribution_request_pending = false;
        self.ring_request_pending = false;
        self.distribution_download = None;
        self.distribution = None;
        self.ring_session = None;
        self.restored_rings.clear();
        self.ring_targets.clear();
        self.user_locked = true;
        self.state = WalletState::closed();
        self.restore_chain = None;
        self.verify_restore_anchor = false;
        self.seed_restore_anchor = false;
        self.anchor_after_spent_checks = false;
        self.restored_outputs.clear();
        self.restore_check_index = 0;
        self.scan_miner_hash = None;
        self.pending_durable = false;
        self.journal = None;
        self.history_observations.clear();
        self.pending_evidence = None;
        self.node_verified = false;
        unsafe { self.transfer.as_mut() }.set_payload(&[]);
        if let Some(page) = unsafe { self.identity_transfer.as_mut() } {
            page.set_payload(&[]);
        }
        let _ = self.publish_bytes(b"NEXAWAL | LOCKED");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn open_request(&mut self) -> MessageInfo {
        // Vault restore/persist is deferred: a hung vault PPC freezes this
        // passive PD (and the shell smoke call that opens the wallet).
        // dump222 async open_pending left FB on OPENING for 15+ min — notify
        // derive never completed on guest. Sync derive in the OPEN Call again
        // (that path previously painted OPEN mainnet); shell must not nest
        // dhcp STATUS PPC under this Call.
        if self.state.open {
            if !self.publish_status() {
                return MessageInfo::new(REPLY_BAD_REQUEST, 0);
            }
            return MessageInfo::new(REPLY_OK, 0);
        }
        let _ = self.publish_bytes(b"NEXAWAL | OPENING WALLET PD...");
        self.open_pending = false;
        if self.open_wallet() {
            if !self.publish_status() {
                return MessageInfo::new(REPLY_BAD_REQUEST, 0);
            }
            MessageInfo::new(REPLY_OK, 0)
        } else {
            let _ = self.publish_bytes(b"NEXAWAL | SETUP REQUIRED");
            MessageInfo::new(REPLY_BAD_REQUEST, 0)
        }
    }

    fn restore_node(&mut self) -> Result<(), ()> {
        let Some(page) = (unsafe { self.identity_transfer.as_mut() }) else { return Err(()); };
        let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x445, 0));
        let endpoint = if reply.label() == 0x300 && reply.count() == 0 {
            page.payload().and_then(|p| core::str::from_utf8(p).ok()).and_then(NodeEndpoint::parse)
        } else if reply.label() == 0x30d || reply.label() == 0x305 {
            // No saved node, or a legacy wallet file overlaps this new extent.
            NodeEndpoint::parse(DEFAULT_NODE)
        } else { None };
        page.set_payload(&[]);
        self.node = endpoint.ok_or(())?;
        self.node_verified = false;
        self.fee_rate = None;
        Ok(())
    }

    fn persist_node(&mut self, endpoint: NodeEndpoint) -> bool {
        let Some(page) = (unsafe { self.identity_transfer.as_mut() }) else { return false; };
        if !page.set_payload(endpoint.authority().as_bytes()) { return false; }
        let reply = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x446, 0));
        page.set_payload(&[]);
        if reply.label() == 0x300 && reply.count() == 0 { return true; }
        // A failed flush may still have committed the new endpoint.
        let read = wallet_protocol::WALLET_TO_VAULT_CHANNEL.pp_call(MessageInfo::new(0x445, 0));
        let matches = read.label() == 0x300 && read.count() == 0
            && page.payload() == Some(endpoint.authority().as_bytes());
        page.set_payload(&[]);
        matches
    }

    fn set_node(&mut self) -> MessageInfo {
        if self.rpc_busy_for_user_action() || self.distribution_request_pending || self.ring_request_pending || self.validation_pending || !self.state.open || self.pending_node.is_some() {
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        let endpoint = unsafe { self.transfer.as_ref() }.payload()
            .and_then(|payload| core::str::from_utf8(payload).ok())
            .and_then(NodeEndpoint::parse);
        unsafe { self.transfer.as_mut() }.set_payload(&[]);
        let Some(endpoint) = endpoint else {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        };
        #[cfg(feature = "wallet-sync-timer")]
        self.interrupt_history_scan();
        self.pending_node = Some(endpoint);
        let _ = self.publish_bytes(b"NEXAWAL | SAVING NODE");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn queue_refresh(&mut self) -> MessageInfo {
        self.clear_payment_validation();
        self.fee_rate = None;
        self.payment_preview = None;
        self.distribution_request_pending = false;
        self.ring_request_pending = false;
        self.distribution_download = None;
        self.distribution = None;
        self.ring_session = None;
        self.restored_rings.clear();
        self.ring_targets.clear();
        // dump223: even tcp.close() inside the shell Call wedged OPEN→refresh
        // under dhcp. Arm a pending flag only; notified() closes + Start.
        self.refresh_pending = true;
        if let Some(evidence) = self.pending_evidence.as_mut() { evidence.invalidate(); }
        if let Some(chain) = self.restore_chain.as_mut() { chain.invalidate(); }
        let _ = self.publish_bytes(b"NEXAWAL | SYNC QUEUED");
        debug_println!("UIAPP|WALLET|SYNC|QUEUED");
        MessageInfo::new(REPLY_OK, 0)
    }

    fn apply_refresh_pending(&mut self) {
        if !self.refresh_pending {
            return;
        }
        // tip768: shell tip-rearm during TIP·SCAN aborted live get_block/tx and
        // restarted TIP_ONLY from tip — never walked down to H3803776. Ignore
        // refresh while tip window is locked; tip hang-skip / SCAN 0 re-arm later.
        // Also ignore once deposit outs exist (decoy/prepare must not be aborted).
        if self.change_hint_spent
            || self.later_hint_armed
            || self.tip_locked
            || (self.is_mainnet_fixture() && self.node_verified && self.state.balance_atomic > 0 && !self.state.outputs.is_empty())
        {
            self.refresh_pending = false;
            debug_println!(
                "UIAPP|WALLET|SYNC|REFRESH_IGNORE tip_locked={} bal={}",
                self.tip_locked,
                self.state.balance_atomic
            );
            return;
        }
        self.refresh_pending = false;
        #[cfg(feature = "wallet-sync-timer")]
        {
            self.cancel_bulk_scan();
            self.bulk_unsupported = false;
        }
        // dump223: tcp.close() here also wedged under dhcp and left QUEUED.
        // Soft-reset sync state only; begin_rpc_connect opens a fresh socket.
        self.tip_locked = false;
        self.tcp_open_sent = false;
        self.sync_rpc = SyncRpcKind::GetInfo;
        self.sync_phase = SyncPhase::Start;
        self.sync_polls = 0;
        self.sync_req_len = 0;
        self.sync_resp_len = 0;
        self.sync_recv_retries = 0;
        self.sync_recv_stall = 0;
        #[cfg(feature = "wallet-sync-timer")]
        { self.sync_rate_retries = 0; self.sync_timer_restart = false; }
        let _ = self.publish_bytes(b"NEXAWAL | SYNC QUEUED");
        debug_println!("UIAPP|WALLET|SYNC|QUEUED|APPLY");
    }

    #[cfg(feature = "wallet-https")]
    fn retry_read_only_network_failure(&mut self) -> bool {
        if !self.scan_request_is_read_only() || self.sync_req_len == 0 {
            return false;
        }
        #[cfg(feature = "wallet-sync-timer")]
        {
            self.tcp_mut().close();
            self.tls_session = None;
            self.sync_recv_retries = 0;
            self.schedule_scan_request(true);
            if self.tip_label_showing() {
                self.publish_tip_scan(self.daemon_tip_height, self.scan_height, b"NETWORK WAIT");
            } else {
                let _ = self.publish_bytes(b"NEXAWAL | NETWORK RETRY WAIT");
            }
            debug_println!("UIAPP|WALLET|SYNC|NETWORK RETRY WAIT");
            true
        }
        #[cfg(not(feature = "wallet-sync-timer"))]
        self.sync_retry_recv("HTTPS_NETWORK")
    }

    fn sync_fail(&mut self, msg: &[u8]) {
        #[cfg(feature = "wallet-sync-timer")]
        if walletcore_core::transient_scan_network_error(msg) && self.retry_read_only_network_failure() {
            return;
        }
        #[cfg(feature = "wallet-sync-timer")]
        self.cancel_bulk_scan();
        self.clear_payment_validation();
        self.fee_rate = None;
        self.payment_preview = None;
        debug_println!(
            "UIAPP|WALLET|SYNC|FAIL {}",
            core::str::from_utf8(msg).unwrap_or("?")
        );
        self.tip_locked = false;
        let restore_failure = !self.is_mainnet_fixture() && self.state.open && matches!(self.sync_rpc,
            SyncRpcKind::GetInfo | SyncRpcKind::GetBlock | SyncRpcKind::GetMinerIndices
                | SyncRpcKind::GetTxBatch | SyncRpcKind::RestoreSpent);
        if self.creation_height_lookup.take().is_some() {
            let _ = self.publish_bytes(b"NEXAWAL | CREATION HEIGHT FAILED");
        } else if matches!(self.sync_rpc, SyncRpcKind::GetDistribution | SyncRpcKind::GetDistributionAnchor | SyncRpcKind::GetRingBatch) {
            self.ring_request_pending = false;
            self.distribution_download = None;
            self.distribution = None;
            self.ring_session = None;
            self.restored_rings.clear();
            self.ring_targets.clear();
            let _ = self.publish_status_tag(b" | DECOY DATA UNAVAILABLE | SCAN OK | SEND NOT READY");
        } else if self.sync_rpc == SyncRpcKind::GetFeeEstimate {
            let _ = self.publish_status_tag(b" | FEE UNAVAILABLE | SCAN OK | SEND NOT READY");
        } else if restore_failure {
            if let Some(evidence) = self.pending_evidence.as_mut() { evidence.invalidate(); }
            if let Some(chain) = self.restore_chain.as_mut() { chain.invalidate(); }
            self.state.scanned = false;
            self.state.balance_atomic = 0;
            self.state.unlocked_atomic = 0;
            let mut visible = [0u8; 256];
            let suffix = b" | BAL 0 | UNLOCKED 0";
            let take = msg.len().min(visible.len() - suffix.len());
            visible[..take].copy_from_slice(&msg[..take]);
            visible[take..take + suffix.len()].copy_from_slice(suffix);
            let _ = self.publish_bytes(&visible[..take + suffix.len()]);
        } else {
            let _ = self.publish_bytes(msg);
        }
        self.tcp_mut().close();
        // Must clear so the next Start/WaitConnect issues a fresh OPEN — leaving
        // this true after FAIL skipped OPEN and left FB on Connecting with no SYN.
        self.tcp_open_sent = false;
        self.sync_phase = SyncPhase::Idle;
        self.sync_polls = 0;
        self.sync_recv_retries = 0;
        self.sync_recv_stall = 0;
        #[cfg(feature = "wallet-sync-timer")]
        { self.sync_rate_retries = 0; self.sync_timer_restart = false; }
    }

    /// Tip-window peer get_tx can hang RECV forever. Skip the hung batch and
    /// keep walking remaining peer hashes so a deposit not in tx[0] is still
    /// findable; miner-index hang continues with peers. Historical TEST_SCAN
    /// heights: skip the hung height and advance the restore walk / tip window.
    fn tip_abandon_peers_and_finish(&mut self) -> bool {
        if !self.is_mainnet_fixture() { return false; }
        let on_test = TEST_SCAN_HEIGHTS.contains(&self.scan_height);
        let tip_window = !on_test;
        let scan_rpc = matches!(
            self.sync_rpc,
            SyncRpcKind::GetBlock
                | SyncRpcKind::GetTxBatch
                | SyncRpcKind::GetMinerIndices
        );
        if !scan_rpc {
            return false;
        }
        // TEST_SCAN walk: don't park forever on one historical height (dump119
        // SCAN 3794104 RX). Advance like assemble-Err TEST_SKIP.
        if on_test {
            let tip = if self.daemon_tip_height != 0 {
                self.daemon_tip_height
            } else {
                self.scan_height
            };
            self.tcp_mut().close();
            self.sync_recv_retries = 0;
            self.sync_recv_stall = 0;
            self.sync_polls = 0;
            self.scan_hashes.clear();
            self.scan_txs.clear();
            self.scan_tx_first_idx.clear();
            self.scan_batch = 0;
            self.scan_block = None;
            self.scan_first_rct = None;
            self.test_scan_i = self.test_scan_i.saturating_add(1);
            if self.test_scan_i < TEST_SCAN_HEIGHTS.len() {
                let next = TEST_SCAN_HEIGHTS[self.test_scan_i];
                debug_println!(
                    "UIAPP|WALLET|SCAN|TEST_HANG_SKIP h={} -> {}",
                    self.scan_height,
                    next
                );
                self.publish_tip_scan(tip, self.scan_height, b"SKIP");
                self.arm_get_block(next);
                return true;
            }
            if tip > 0 {
                self.scan_blocks_left = TIP_HUNT_BLOCKS_LEFT;
                self.test_scan_i = self.test_scan_i.saturating_add(1);
                debug_println!(
                    "UIAPP|WALLET|SCAN|TIP_WINDOW_AFTER_HANG tip={}",
                    tip
                );
                self.publish_tip_scan(tip, self.scan_height, b"SKIP");
                self.arm_get_block(tip);
                return true;
            }
            return false;
        }
        if !(tip_window && self.scan_block.is_some()) {
            // Tip-window GetBlock hang (or GetTx/Miner before assemble): dump123
            // used to park SCAN 0 immediately — that aborted the whole tip
            // window on the first hung height, so a deposit at tip-N was never
            // reached (dump221: tip256 + SCAN 0 in ~80s). Skip the hung height
            // and keep walking predecessors while blocks_left remain.
            if tip_window && scan_rpc {
                self.tcp_mut().close();
                self.sync_recv_stall = 0;
                self.sync_polls = 0;
                let tip = self.daemon_tip_height.max(self.scan_height);
                let h = self.scan_height;
                // Deposit block (~21 txs) often exceeds tip poll-cap; never
                // hang-skip past it or the funded prepare path is unreachable.
                if (h == DEPOSIT_HINT_HEIGHT || h == CHANGE_HINT_HEIGHT)
                    && self.sync_rpc == SyncRpcKind::GetBlock
                {
                    self.sync_recv_retries = self.sync_recv_retries.saturating_add(1);
                    debug_println!(
                        "UIAPP|WALLET|SCAN|DEPOSIT_BLOCK_RETRY n={} filled_rpc={}",
                        self.sync_recv_retries,
                        self.sync_rpc as u8
                    );
                    let _ = self.publish_progress(b"NEXAWAL | DEPOSIT RETRY");                    self.arm_get_block(h);
                    return true;
                }
                if self.scan_blocks_left > 0 && h > 0 {
                    let next = h.saturating_sub(1);
                    if self.state.balance_atomic == 0
                        && h > DEPOSIT_HINT_HEIGHT
                        && next < DEPOSIT_HINT_HEIGHT
                    {
                        debug_println!(
                            "UIAPP|WALLET|SCAN|DEPOSIT_REARM from={} tip={}",
                            h,
                            tip
                        );
                        self.sync_recv_retries = 0;
                        self.arm_get_block(DEPOSIT_HINT_HEIGHT);
                        return true;
                    }
                    debug_println!(
                        "UIAPP|WALLET|SCAN|TIP_HANG_SKIP h={} left={} rpc={}",
                        h,
                        self.scan_blocks_left,
                        self.sync_rpc as u8
                    );
                    self.sync_recv_retries = 0;
                    self.scan_blocks_left = self.scan_blocks_left.saturating_sub(1);
                    self.scan_hashes.clear();
                    self.scan_txs.clear();
                    self.scan_tx_first_idx.clear();
                    self.scan_batch = 0;
                    self.scan_block = None;
                    self.scan_first_rct = None;
                    self.publish_tip_scan(tip, h, b"SKIP");
                    self.arm_get_block(next);
                    return true;
                }
                debug_println!(
                    "UIAPP|WALLET|SCAN|TIP_BLOCK_DONE_HANG h={} left={} rpc={}",
                    h,
                    self.scan_blocks_left,
                    self.sync_rpc as u8
                );
                // Exhausted tip window — drop tip_locked so SCAN 0/OK sticks.
                self.tip_locked = false;
                if self.state.balance_atomic > 0 && !self.state.outputs.is_empty() {
                    // mark scanned so format_status emits SCAN/HIST for Activity.
                    self.state.scanned = true;
                    let _ = self.publish_status();
                    self.sync_phase = SyncPhase::Idle;
                    self.arm_decoy_histogram();
                } else {
                    self.state.mark_scanned_empty();
                    let _ = self.publish_status();
                    self.sync_phase = SyncPhase::Idle;
                }
                return true;
            }
            return false;
        }
        self.tcp_mut().close();
        self.sync_recv_stall = 0;
        self.sync_polls = 0;
        if self.sync_rpc == SyncRpcKind::GetTxBatch {
            // Must match arm_tx_batch tip stride (1).
            let batch = 1usize;
            let start = self.scan_batch * batch;
            // Deposit hint: retry hung peer txs (pack8 TIP_SKIP_BATCH → outs=0).
            // After 8 retries on the same batch, skip that one hash only.
            if self.scan_height == DEPOSIT_HINT_HEIGHT && self.sync_recv_retries < 8 {
                self.sync_recv_retries = self.sync_recv_retries.saturating_add(1);
                debug_println!(
                    "UIAPP|WALLET|SCAN|DEPOSIT_TX_RETRY h={} batch={} n={}",
                    self.scan_height,
                    self.scan_batch,
                    self.sync_recv_retries
                );
                let _ = self.publish_progress(b"NEXAWAL | DEPOSIT TX RETRY");
                self.arm_tx_batch();
                return true;
            }
            self.sync_recv_retries = 0;
            let next = self.scan_batch.saturating_add(1);
            debug_println!(
                "UIAPP|WALLET|SCAN|TIP_SKIP_BATCH h={} batch={} start={} left={}",
                self.scan_height,
                self.scan_batch,
                start,
                self.scan_hashes.len().saturating_sub(start + batch)
            );
            self.scan_batch = next;
            let _ = self.publish_progress(b"NEXAWAL | TIP TX SKIP");
            self.arm_tx_batch();
            return true;
        }
        self.sync_recv_retries = 0;
        // Miner indices hung: continue with peer txs (miner outs still from blob).
        debug_println!(
            "UIAPP|WALLET|SCAN|TIP_SKIP_MINER h={} peers={}",
            self.scan_height,
            self.scan_hashes.len()
        );
        let _ = self.publish_progress(b"NEXAWAL | TIP MINER SKIP");
        self.arm_tx_batch();
        true
    }

    fn sync_retry_recv(&mut self, why: &str) -> bool {
        let retryable = matches!(
            self.sync_rpc,
            SyncRpcKind::GetInfo
                | SyncRpcKind::GetBlock
                | SyncRpcKind::GetMinerIndices
                | SyncRpcKind::GetTxBatch
                | SyncRpcKind::GetHistogram
                | SyncRpcKind::GetFeeEstimate
                | SyncRpcKind::GetDistribution
                | SyncRpcKind::GetDistributionAnchor
                | SyncRpcKind::GetRingBatch
                | SyncRpcKind::ValidateInfo
                | SyncRpcKind::ValidateAnchorBefore
                | SyncRpcKind::ValidateSpent
                | SyncRpcKind::ValidateAnchorAfter
                | SyncRpcKind::ValidateFee
                | SyncRpcKind::GetOuts
                | SyncRpcKind::SendRaw
                | SyncRpcKind::Reconcile
                | SyncRpcKind::IsKeyImageSpent
                | SyncRpcKind::RestoreSpent
        );
        let tip_scan = matches!(
            self.sync_rpc,
            SyncRpcKind::GetBlock
                | SyncRpcKind::GetTxBatch
                | SyncRpcKind::GetMinerIndices
        ) && !TEST_SCAN_HEIGHTS.contains(&self.scan_height);
        // tip768: NO_BLOB after truncated FIN needed >1 retry; 1→hang-skip storm.
        // Deposit get_tx often EMPTY_FIN/BAD_HTTP under TCG — allow more reopens
        // before tip_abandon skips the hung peer hash.
        let deposit_tx = self.scan_height == DEPOSIT_HINT_HEIGHT
            && self.sync_rpc == SyncRpcKind::GetTxBatch;
        let max_retries = if deposit_tx {
            16
        } else if tip_scan {
            6
        } else {
            8
        };
        if !retryable || self.sync_req_len == 0 || self.sync_recv_retries >= max_retries {
            return false;
        }
        self.sync_recv_retries = self.sync_recv_retries.saturating_add(1);
        debug_println!(
            "UIAPP|WALLET|SYNC|RETRY {} n={} rpc={}",
            why,
            self.sync_recv_retries,
            self.sync_rpc as u8
        );
        self.tcp_mut().close();
        self.begin_rpc_connect();
        true
    }

    /// Fire-and-forget arm: worker fetches on its own dhcp slot while wallet TCP runs.
    fn kick_scan_worker(&mut self, height: u64) {
        // The legacy worker uses a fixed Cake node. Never use it for a
        // different configured node; the wallet's normal scan is authoritative.
        if height == 0 || self.node.host() != "xmr-node.cakewallet.com" || self.node.port != 18081 {
            return;
        }
        let ping = WORKER.pp_call(MessageInfo::new(REQUEST_WORKER_PING, 0));
        debug_println!("UIAPP|WALLET|WORKER|PING 0x{:x}", ping.label());
        if ping.label() != REPLY_WORKER_OK {
            return;
        }
        set_mr(0, height);
        let reply = WORKER.pp_call(MessageInfo::new(REQUEST_WORKER_ARM_BLOCK, 1));
        if reply.label() != REPLY_WORKER_OK {
            debug_println!("UIAPP|WALLET|WORKER|ARM_FAIL 0x{:x}", reply.label());
            return;
        }
        WORKER.notify();
        debug_println!("UIAPP|WALLET|WORKER|PARALLEL h={}", height);
    }

    fn maybe_nudge_worker(&mut self) {
        let status = unsafe { self.scan_job.as_ref().status };
        if status == ScanJobPage::STATUS_ARMED {
            WORKER.notify();
        }
    }

    fn finish_with_worker(&mut self, tip_hint: u64) {
        let status = unsafe { self.scan_job.as_ref().status };
        match status {
            ScanJobPage::STATUS_DONE => {
                let page = unsafe { self.scan_job.as_ref() };
                debug_println!(
                    "UIAPP|WALLET|WORKER|BLOCK_OK h={} txs={}",
                    page.height,
                    page.outs
                );
                self.arm_decoy_histogram();
            }
            ScanJobPage::STATUS_ARMED => {
                self.sync_polls = 0;
                self.sync_phase = SyncPhase::WaitWorker;
                let _ = self.publish_progress(b"NEXAWAL | WORKER FETCH...");
                WORKER.notify();
            }
            _ => {
                // No parallel job — sequential get_block proof on tip height.
                self.kick_scan_worker(tip_hint);
                let status = unsafe { self.scan_job.as_ref().status };
                if status == ScanJobPage::STATUS_ARMED {
                    self.sync_polls = 0;
                    self.sync_phase = SyncPhase::WaitWorker;
                    let _ = self.publish_progress(b"NEXAWAL | WORKER FETCH...");
                } else {
                    self.arm_decoy_histogram();
                }
            }
        }
    }

    fn reset_restore_chain(&mut self) {
        // A reorganization invalidates any durable empty-prefix checkpoint.
        if let Some(mut candidate) = self.journal.clone() {
            if candidate.empty_scan_checkpoint.take().is_some() {
                if self.persist_wallet_journal(&candidate) { self.journal = Some(candidate); }
            }
        }
        self.clear_payment_validation();
        self.fee_rate = None;
        self.payment_preview = None;
        self.distribution_request_pending = false;
        self.ring_request_pending = false;
        self.distribution_download = None;
        self.distribution = None;
        self.ring_session = None;
        self.restored_rings.clear();
        self.ring_targets.clear();
        if let Some(chain) = self.restore_chain.as_mut() { chain.reset(); }
        if let Some(evidence) = self.pending_evidence.as_mut() { evidence.reset(); }
        self.verify_restore_anchor = false;
        self.seed_restore_anchor = false;
        self.anchor_after_spent_checks = false;
        self.restored_outputs.clear();
        for observation in &mut self.history_observations { *observation = HistoryObservation::Unchecked; }
        self.restore_check_index = 0;
        self.state.clear_restore_history();
        self.scan_hashes.clear();
        self.scan_txs.clear();
        self.scan_tx_first_idx.clear();
        self.scan_block = None;
        self.scan_first_rct = None;
    }

    fn begin_restore_spent_checks(&mut self) {
        if self.restored_outputs.len() != self.state.outputs.len() {
            self.sync_fail(b"NEXAWAL | SCAN FAIL | OUTPUT STATE");
            return;
        }
        self.restore_check_index = 0;
        self.restore_check_count = 0;
        for output in &mut self.restored_outputs { output.spent_status = None; }
        self.state.scanned = false;
        self.state.balance_atomic = 0;
        self.state.unlocked_atomic = 0;
        self.arm_restore_spent_check();
    }

    fn finish_restore_spent_checks(&mut self) {
        let Some((total, unlocked)) = restored_balances(&self.restored_outputs, self.daemon_tip_height) else {
            self.sync_fail(b"NEXAWAL | SCAN FAIL | INCOMPLETE SPENT CHECK"); return;
        };
        if let (Some(evidence), Some(chain)) = (self.pending_evidence.as_mut(), self.restore_chain.as_ref()) {
            evidence.finish(chain, self.daemon_tip_height);
        }
        // Plan using only the complete, rechecked scan; persist the entire
        // candidate before changing the active pending payment in memory.
        let plan = match (self.journal.as_ref(), self.restore_chain.as_ref()) {
            (Some(journal), Some(chain)) => plan_journal_reconciliation(
                journal, &self.history_observations, self.pending_evidence.as_ref(),
                chain, self.daemon_tip_height, 10,
            ),
            _ => None,
        };
        let mut journal_status: &[u8] = b"";
        let mut history_ready = false;
        match plan {
            Some(JournalPlan::Confirm(candidate)) => {
                let matches_state = self.pending_durable && self.journal.as_ref()
                    .and_then(|journal| journal.pending.as_ref())
                    .is_some_and(|pending| pending.tx_hex.as_str() == self.state.pending_tx_hex
                        && pending.amount == self.state.pending_send_amount && pending.fee == self.state.pending_fee_atomic);
                if matches_state && self.persist_wallet_journal(&candidate) {
                    if candidate.sent.len() > self.history_observations.len() {
                        if let Some(entry) = candidate.sent.last() {
                            self.history_observations.push(HistoryObservation::Block { hash: entry.block_hash, contains_transaction: true });
                        }
                    }
                    self.state.pending_tx_hex.clear();
                    self.state.pending_fee_atomic = 0;
                    self.state.pending_send_amount = 0;
                    self.pending_durable = false;
                    self.pending_evidence = None;
                    self.journal = Some(candidate);
                    history_ready = true;
                    journal_status = b" | PAYMENT RECONCILED";
                } else { journal_status = b" | JOURNAL SAVE FAILED"; }
            }
            Some(JournalPlan::Reopen(candidate)) => {
                let old = self.journal.as_ref();
                let reopened = candidate.pending.as_ref();
                let index = old.and_then(|journal| reopened.and_then(|pending| pending.transaction_id()
                    .and_then(|txid| journal.sent.iter().position(|entry| entry.txid == txid))));
                if self.state.pending_tx_hex.is_empty() && index.is_some() && self.persist_wallet_journal(&candidate) {
                    let pending = candidate.pending.as_ref().unwrap();
                    self.state.pending_tx_hex = String::from(pending.tx_hex.as_str());
                    self.state.pending_fee_atomic = pending.fee;
                    self.state.pending_send_amount = pending.amount;
                    self.pending_evidence = pending.transaction_id().map(PendingChainEvidence::new);
                    self.pending_durable = true;
                    self.history_observations.remove(index.unwrap());
                    self.journal = Some(candidate);
                    history_ready = true;
                    journal_status = b" | PAYMENT REORGED";
                } else { journal_status = b" | JOURNAL SAVE FAILED"; }
            }
            Some(JournalPlan::RecoveryRequired) => journal_status = b" | REORG RECOVERY REQUIRED",
            Some(JournalPlan::NoChange) => history_ready = true,
            None => journal_status = b" | HISTORY UNVERIFIED",
        }
        self.state.set_recent_activity(&[]);
        if history_ready {
            let outputs: Vec<_> = self.state.outputs.iter().zip(&self.restored_outputs)
                .map(|(output, meta)| RestoredActivityOutput {
                    txid: output.transaction(), height: meta.height, amount: meta.amount,
                }).collect();
            match self.journal.as_ref().and_then(|journal| restored_activity(&outputs, journal)) {
                Some(events) => self.state.set_recent_activity(&events),
                None => { journal_status = b" | HISTORY INVALID"; history_ready = false; },
            }
        }
        self.state.balance_atomic = total;
        self.state.unlocked_atomic = unlocked;
        // Sending remains gated until input selection is verified.
        self.state.scanned = false;
        self.state.height = self.daemon_tip_height;
        self.tip_locked = false;
        self.sync_phase = SyncPhase::Idle;
        let mut status = [0u8; 128];
        let suffix = b" | SCAN OK | SEND NOT READY";
        let n = journal_status.len();
        status[..n].copy_from_slice(journal_status);
        status[n..n + suffix.len()].copy_from_slice(suffix);
        let _ = self.publish_status_tag(&status[..n + suffix.len()]);
        #[cfg(feature = "wallet-sync-timer")]
        if let Some((start, began)) = self.bulk_begin.take() {
            debug_println!("UIAPP|WALLET|RANGE|VERIFIED start={} next={} elapsed_ms={}", start, self.daemon_tip_height, wallet_bulk::monotonic_ms().saturating_sub(began));
        }
        if history_ready && unlocked > 0 && self.state.pending_tx_hex.is_empty() { self.arm_fee_estimate(); }
    }

    fn arm_fee_estimate(&mut self) {
        self.fee_rate = None;
        if !self.node_verified { return; }
        let mut body = [0u8; 128];
        let Some(body_len) = build_fee_estimate_body(&mut body) else { return; };
        let mut http = [0u8; 512];
        let Some(http_len) = build_http_post(self.node.authority(), "/json_rpc", &body[..body_len], &mut http) else { return; };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::GetFeeEstimate;
        self.begin_rpc_connect();
    }

    fn arm_distribution_chunk(&mut self) {
        self.sync_rpc = SyncRpcKind::GetDistribution;
        let Some((from, to)) = self.distribution_download.as_ref().and_then(DistributionDownload::next_range) else {
            self.sync_fail(b"NEXAWAL | DISTRIBUTION RANGE"); return;
        };
        let mut body = [0u8; 256];
        let Some(n) = build_output_distribution_body(from, to, &mut body) else { self.sync_fail(b"NEXAWAL | DISTRIBUTION REQUEST"); return; };
        let Some(n) = build_http_post(self.node.authority(), "/json_rpc", &body[..n], &mut self.sync_req) else { self.sync_fail(b"NEXAWAL | DISTRIBUTION REQUEST"); return; };
        self.sync_req_len = n;
        self.sync_rpc = SyncRpcKind::GetDistribution;
        self.begin_rpc_connect();
    }

    fn arm_distribution_anchor(&mut self) {
        self.sync_rpc = SyncRpcKind::GetDistributionAnchor;
        let Some((height, _)) = self.restore_chain.and_then(|chain| chain.anchor()) else { self.sync_fail(b"NEXAWAL | DISTRIBUTION ANCHOR"); return; };
        let mut body = [0u8; 256];
        let Some(n) = build_get_block_body(height, &mut body) else { self.sync_fail(b"NEXAWAL | DISTRIBUTION ANCHOR"); return; };
        let Some(n) = build_http_post(self.node.authority(), "/json_rpc", &body[..n], &mut self.sync_req) else { self.sync_fail(b"NEXAWAL | DISTRIBUTION ANCHOR"); return; };
        self.sync_req_len = n;
        self.sync_rpc = SyncRpcKind::GetDistributionAnchor;
        self.begin_rpc_connect();
    }

    fn start_restored_ring(&mut self) {
        self.sync_rpc = SyncRpcKind::GetRingBatch;
        let Some(index) = self.ring_targets.get(self.ring_target_cursor).copied() else { self.sync_fail(b"NEXAWAL | NO SPENDABLE OUTPUT"); return; };
        let Some(output) = self.state.outputs.get(index) else { self.sync_fail(b"NEXAWAL | RING OUTPUT STATE"); return; };
        if output.is_legacy() { self.sync_fail(b"NEXAWAL | LEGACY SPENDING UNAVAILABLE"); return; }
        let real = (output.index_on_blockchain(), output.key().compress().to_bytes(), output.commitment().commit().compress().to_bytes());
        let mut entropy = Zeroizing::new([0u8; 32]);
        for chunk in entropy.chunks_mut(entropy_protocol::SAMPLE_BYTES) {
            let reply = Channel::new(4).pp_call(MessageInfo::new(entropy_protocol::REQUEST_BYTES, 0));
            if reply.label() != entropy_protocol::REPLY_OK || reply.count() != 2 {
                self.sync_fail(b"NEXAWAL | ENTROPY UNAVAILABLE"); return;
            }
            chunk[..8].copy_from_slice(&get_mr(0).to_le_bytes());
            chunk[8..].copy_from_slice(&get_mr(1).to_le_bytes());
        }
        self.ring_session = self.distribution.as_ref().and_then(|distribution| DecoyRingSession::start(distribution, entropy, real));
        self.ring_input_index = index;
        if self.ring_session.is_none() { self.sync_fail(b"NEXAWAL | RING SELECTION FAILED"); return; }
        self.arm_restored_ring_batch();
    }

    fn arm_restored_ring_batch(&mut self) {
        self.sync_rpc = SyncRpcKind::GetRingBatch;
        let mut body = [0u8; 2048];
        let Some(n) = self.ring_session.as_ref().and_then(|session| build_get_outs_batch_body(session.request_indices(), &mut body)) else {
            self.sync_fail(b"NEXAWAL | RING REQUEST FAILED"); return;
        };
        let Some(n) = build_http_post(self.node.authority(), "/get_outs", &body[..n], &mut self.sync_req) else {
            self.sync_fail(b"NEXAWAL | RING REQUEST FAILED"); return;
        };
        self.sync_req_len = n;
        self.begin_rpc_connect();
    }

    fn request_preview_validation(&mut self, sign_after: bool) -> MessageInfo {
        if self.sync_phase != SyncPhase::Idle || self.validation_pending || self.payment_preview.is_none()
            || !self.state.pending_tx_hex.is_empty() || !self.node_verified || self.restored_rings.is_empty()
            || (sign_after && (self.state.network == Network::Mainnet
                || !self.journal.as_ref().is_some_and(WalletJournal::can_record_new_transaction))) {
            unsafe { self.transfer.as_mut() }.set_payload(&[]);
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        let expected = if self.ring_max { alloc::format!("DEST {} MAX", self.ring_dest) }
            else { alloc::format!("DEST {} AMOUNT {}", self.ring_dest, self.ring_amount) };
        let matches = unsafe { self.transfer.as_ref() }.payload() == Some(expected.as_bytes());
        unsafe { self.transfer.as_mut() }.set_payload(&[]);
        if !matches { return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0); }
        self.clear_payment_validation();
        self.validation_pending = true;
        self.validation_sign_after = sign_after;
        let _ = self.publish_bytes(b"NEXAWAL | VALIDATING PREVIEW");
        WALLET_SHELL_CHANNEL.notify();
        MessageInfo::new(REPLY_OK, 0)
    }

    fn arm_validation_rpc(&mut self, kind: SyncRpcKind, body: &[u8], path: &str) {
        self.sync_rpc = kind;
        let Some(n) = build_http_post(self.node.authority(), path, body, &mut self.sync_req) else {
            self.sync_fail(b"NEXAWAL | VALIDATION REQUEST FAILED"); return;
        };
        self.sync_req_len = n;
        self.begin_rpc_connect();
    }

    fn arm_validation_anchor(&mut self, after_spent: bool) {
        let Some((height, _)) = self.restore_chain.and_then(|chain| chain.anchor()) else {
            self.sync_fail(b"NEXAWAL | VALIDATION ANCHOR MISSING"); return;
        };
        let mut body = [0u8; 256];
        let Some(n) = build_get_block_body(height, &mut body) else { self.sync_fail(b"NEXAWAL | VALIDATION REQUEST FAILED"); return; };
        self.arm_validation_rpc(if after_spent { SyncRpcKind::ValidateAnchorAfter } else { SyncRpcKind::ValidateAnchorBefore }, &body[..n], "/json_rpc");
    }

    fn arm_validation_spent(&mut self) {
        let Some(validation) = self.payment_validation.as_ref() else { self.sync_fail(b"NEXAWAL | VALIDATION STATE"); return; };
        let start = self.validation_statuses.len();
        let end = start.saturating_add(16).min(validation.key_images().len());
        if start == end { self.sync_fail(b"NEXAWAL | VALIDATION STATE"); return; }
        let mut body = [0u8; 1200];
        let Some(n) = build_spent_status_body(&validation.key_images()[start..end], &mut body) else {
            self.sync_fail(b"NEXAWAL | VALIDATION REQUEST FAILED"); return;
        };
        self.validation_batch_count = end - start;
        self.arm_validation_rpc(SyncRpcKind::ValidateSpent, &body[..n], "/is_key_image_spent");
    }

    fn check_restored_ring_fees(&mut self) {
        let result = match (self.restore_chain.as_ref(), self.fee_rate) {
            (Some(chain), Some(fee)) => create_payment_preview(
                &mut self.state, &self.restored_outputs, chain, self.daemon_tip_height,
                &self.restored_rings, &self.ring_dest, self.ring_amount, self.ring_max, fee),
            _ => Err(PrepareError::NeedScan),
        };
        match result {
            Ok(preview) => {
                self.sync_phase = SyncPhase::Idle;
                let request = if self.ring_max { alloc::format!("DEST {} MAX", self.ring_dest) }
                    else { alloc::format!("DEST {} AMOUNT {}", self.ring_dest, self.ring_amount) };
                let readiness = if self.state.network == Network::Mainnet { "SEND NOT READY" } else { "TEST SEND READY" };
                let status = alloc::format!("NEXAWAL | PREVIEW FEE {} AMOUNT {} | REQUEST {} | {}", preview.fee(), preview.amount(), request, readiness);
                self.payment_preview = Some(preview);
                let _ = self.publish_bytes(status.as_bytes());
            }
            Err(PrepareError::Insufficient) if !self.ring_max => {
                let sum = self.ring_targets.iter().try_fold(0u64, |total, index| {
                    total.checked_add(self.restored_outputs.get(*index)?.amount)
                });
                let expanded = sum.and_then(|total| total.checked_add(1))
                    .and_then(|required| select_restored_inputs(&self.restored_outputs, self.daemon_tip_height, required));
                if let Some(targets) = expanded.filter(|targets| targets.len() > self.ring_targets.len()
                    && targets.starts_with(&self.ring_targets)) {
                    self.ring_target_cursor = self.ring_targets.len();
                    self.ring_targets = targets;
                    self.start_restored_ring();
                } else {
                    self.sync_phase = SyncPhase::Idle;
                    let _ = self.publish_bytes(b"NEXAWAL | PREPARE INSUFFICIENT");
                }
            }
            Err(_) => { self.sync_fail(b"NEXAWAL | PREPARE FAILED"); }
        }
    }

    fn arm_restore_spent_check(&mut self) {
        if self.restore_check_index == self.restored_outputs.len() {
            // Recheck the scanned tip after all network-dependent spent checks,
            // so a reorganization during reconciliation does not publish stale data.
            if let Some((height, _)) = self.restore_chain.and_then(|chain| chain.anchor()) {
                self.verify_restore_anchor = true;
                self.anchor_after_spent_checks = true;
                self.arm_get_block(height);
            } else {
                self.finish_restore_spent_checks();
            }
            return;
        }
        let end = self.restore_check_index.saturating_add(16).min(self.restored_outputs.len());
        let mut images = Vec::with_capacity(end - self.restore_check_index);
        for index in self.restore_check_index..end {
            let Some(output) = self.state.outputs.get(index) else { self.sync_fail(b"NEXAWAL | SCAN FAIL | OUTPUT STATE"); return; };
            let Some(image) = output_key_image(&self.state, output) else { self.sync_fail(b"NEXAWAL | SCAN FAIL | KEY IMAGE"); return; };
            images.push(image);
        }
        let mut body = [0u8; 1200];
        let Some(n) = build_spent_status_body(&images, &mut body) else { self.sync_fail(b"NEXAWAL | SCAN FAIL | KEY IMAGE"); return; };
        self.restore_check_count = images.len();
        let Some(length) = build_http_post(self.node.authority(), "/is_key_image_spent", &body[..n], &mut self.sync_req) else {
            self.sync_fail(b"NEXAWAL | SCAN FAIL | SPENT REQUEST"); return;
        };
        self.sync_req_len = length;
        self.sync_rpc = SyncRpcKind::RestoreSpent;
        self.begin_rpc_connect();
    }

    fn arm_key_image_spent_check(&mut self) {
        let Some(ki) = first_output_key_image(&self.state) else {
            debug_println!("UIAPP|WALLET|KI_CHECK|NO_KI");
            let _ = self.publish_status();
            self.sync_phase = SyncPhase::Idle;
            return;
        };
        let mut body = [0u8; 96];
        let mut n = 0usize;
        let pre = b"{\"key_images\":[\"";
        let post = b"\"]}";
        if pre.len() + 64 + post.len() > body.len() {
            let _ = self.publish_status();
            self.sync_phase = SyncPhase::Idle;
            return;
        }
        body[..pre.len()].copy_from_slice(pre);
        n = pre.len();
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for b in ki {
            body[n] = HEX[(b >> 4) as usize];
            body[n + 1] = HEX[(b & 0xf) as usize];
            n += 2;
        }
        body[n..n + post.len()].copy_from_slice(post);
        n += post.len();
        let mut http = [0u8; 512];
        let Some(http_len) =
            build_http_post(self.node.authority(), "/is_key_image_spent", &body[..n], &mut http)
        else {
            let _ = self.publish_status();
            self.sync_phase = SyncPhase::Idle;
            return;
        };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::IsKeyImageSpent;
        let _ = self.publish_progress(b"NEXAWAL | KI CHECK...");
        debug_println!("UIAPP|WALLET|KI_CHECK|ARM");
        self.begin_rpc_connect();
    }

    fn arm_decoy_histogram(&mut self) {
        let mut body = [0u8; 256];
        let Some(body_len) = build_get_output_histogram_body(&mut body) else {
            let _ = self.publish_status();
            self.sync_phase = SyncPhase::Idle;
            return;
        };
        let mut http = [0u8; 2048];
        let Some(http_len) =
            build_http_post(self.node.authority(), "/json_rpc", &body[..body_len], &mut http)
        else {
            let _ = self.publish_status();
            self.sync_phase = SyncPhase::Idle;
            return;
        };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::GetHistogram;
        let _ = self.publish_progress(b"NEXAWAL | DECOY HIST...");
        debug_println!("UIAPP|WALLET|DECOY|HIST_ARM");
        self.begin_rpc_connect();
    }

    fn arm_decoy_get_outs(&mut self, index: u64) {
        self.decoy_out_index = index;
        let mut body = [0u8; 128];
        let Some(body_len) = build_get_outs_body(index, &mut body) else {
            let _ = self.publish_status();
            self.sync_phase = SyncPhase::Idle;
            return;
        };
        let mut http = [0u8; 2048];
        let Some(http_len) = build_http_post(self.node.authority(), "/get_outs", &body[..body_len], &mut http)
        else {
            let _ = self.publish_status();
            self.sync_phase = SyncPhase::Idle;
            return;
        };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::GetOuts;
        let _ = self.publish_progress(b"NEXAWAL | DECOY OUTS...");
        debug_println!("UIAPP|WALLET|DECOY|OUTS_ARM idx={}", index);
        self.begin_rpc_connect();
    }

    fn sync_step_wait_worker(&mut self) {
        WORKER.notify();
        let page = unsafe { self.scan_job.as_ref() };
        match page.status {
            ScanJobPage::STATUS_DONE => {
                debug_println!(
                    "UIAPP|WALLET|WORKER|BLOCK_OK h={} txs={}",
                    page.height,
                    page.outs
                );
                self.arm_decoy_histogram();
            }
            ScanJobPage::STATUS_FAIL => {
                debug_println!(
                    "UIAPP|WALLET|WORKER|FAIL h={} status={}",
                    page.height,
                    page.status
                );
                // Tip scan already succeeded; continue decoy plumbing proof.
                self.arm_decoy_histogram();
            }
            _ => {
                self.sync_polls = self.sync_polls.saturating_add(1);
                if self.sync_polls % 25 == 0 {
                    debug_println!(
                        "UIAPP|WALLET|WORKER|WAIT status={} polls={}",
                        page.status,
                        self.sync_polls
                    );
                }
                if self.sync_polls >= SYNC_CONNECT_POLL_MAX {
                    debug_println!("UIAPP|WALLET|WORKER|TIMEOUT");
                    self.arm_decoy_histogram();
                }
            }
        }
    }

    fn sync_step_start(&mut self) {
        debug_println!("UIAPP|WALLET|SYNC|BEGIN {}", self.node.host());
        if self.creation_height_lookup.is_none() && !self.open_wallet() {
            self.sync_fail(b"NEXAWAL | SYNC OPEN FAIL");
            return;
        }
        self.node_verified = false;
        let node = self.node;
        let ip = if cfg!(feature = "utm-nexawal-rpc-bridge") && !cfg!(feature = "wallet-https") && node.host() == "rpc.nexawal.com" && node.port == 443 {
            Some([10, 0, 2, 2])
        } else if let Some(ip) = node.ipv4() {
            Some(ip)
        } else if node.host() == "xmr-node.cakewallet.com" {
            Some(DAEMON_IP)
        } else {
            self.tcp_mut().dns_lookup(node.host()).ok()
        };
        let Some(ip) = ip else {
            #[cfg(feature = "wallet-sync-timer")]
            {
                self.sync_rpc = SyncRpcKind::GetInfo;
                self.schedule_scan_request(true);
                self.sync_timer_restart = true;
                let _ = self.publish_bytes(b"NEXAWAL | DNS RETRY WAIT");
                debug_println!("UIAPP|WALLET|SYNC|DNS RETRY WAIT");
                return;
            }
            #[cfg(not(feature = "wallet-sync-timer"))]
            self.sync_fail(b"NEXAWAL | SYNC CONNECT FAIL | DNS");
            #[cfg(not(feature = "wallet-sync-timer"))]
            return;
        };
        self.node_ip = ip;
        // dump223: vault FILE_SIZE + dhcp STATUS probes before CONNECTING left
        // FB on SYNC QUEUED forever when vault/dhcp PPC wedged. Tip uses fixed
        // Cake A-record — skip those probes; paint CONNECTING and open TCP.
        debug_println!(
            "UIAPP|WALLET|SYNC|DNS {}.{}.{}.{}",
            self.node_ip[0],
            self.node_ip[1],
            self.node_ip[2],
            self.node_ip[3]
        );
        let body = get_info_request_body();
        let mut http = [0u8; 2048];
        let format = if self.scan_preflight(SyncRpcKind::GetInfo) {
            wallet_tcp::build_scan_preflight_http
        } else { build_json_rpc_http };
        let Some(http_len) = format(self.node.authority(), body, &mut http) else {
            self.sync_fail(b"NEXAWAL | SYNC BAD REQ");
            return;
        };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::GetInfo;
        self.begin_rpc_connect();
        // Wake shell to paint CONNECTING before any TCP PPC that might nest.
        WALLET_SHELL_CHANNEL.notify();
    }

    fn scan_preflight(&self, rpc: SyncRpcKind) -> bool {
        #[cfg(feature = "wallet-sync-timer")]
        if self.bulk_unsupported { return false; }
        if cfg!(feature = "wallet-handoff-prime")
            || !cfg!(all(feature = "wallet-sync-timer", feature = "wallet-wide-rx"))
            || self.node.port != 443 || self.is_mainnet_fixture()
            || self.creation_height_lookup.is_some() || !self.state.open
        { return false; }
        let Some(chain) = self.restore_chain.as_ref() else { return false; };
        match rpc {
            SyncRpcKind::GetInfo => chain.anchor().is_none(),
            SyncRpcKind::GetBlock => self.verify_restore_anchor && !self.anchor_after_spent_checks
                && chain.next() < self.daemon_tip_height,
            _ => false,
        }
    }

    fn begin_rpc_connect(&mut self) {
        self.sync_polls = 0;
        self.tcp_open_sent = false;
        #[cfg(feature = "wallet-sync-timer")]
        if self.scan_request_is_read_only() {
            self.schedule_scan_request(false);
            return;
        }
        // dump223: tcp.open() here nested dhcp under the same notify that
        // armed CONNECTING and hung before shell could paint. Enter
        // WaitConnect first; sync_step_wait_connect opens after one bounce.
        self.sync_phase = SyncPhase::WaitConnect;
        let _ = self.publish_bytes(b"NEXAWAL | SYNC CONNECTING");
    }

    #[cfg(feature = "wallet-https")]
    fn scan_request_is_read_only(&self) -> bool {
        matches!(self.sync_rpc, SyncRpcKind::GetInfo | SyncRpcKind::GetBlock
            | SyncRpcKind::GetMinerIndices | SyncRpcKind::GetTxBatch)
    }

    #[cfg(feature = "wallet-sync-timer")]
    fn schedule_scan_request(&mut self, rate_limited: bool) {
        self.sync_timer_restart = false;
        if rate_limited {
            self.sync_rate_retries = self.sync_rate_retries.saturating_add(1);
        }
        let delay = walletcore_core::scan_request_delay_ns(true, self.sync_rate_retries).unwrap();
        #[cfg(feature = "wallet-sync-timer-test")]
        let delay = if self.sync_rate_retries == 1 { 1_000_000_000 } else { delay };
        self.sync_phase = SyncPhase::WaitTimer;
        if rate_limited && self.tip_label_showing() {
            self.publish_tip_scan(self.daemon_tip_height, self.scan_height, b"RATE WAIT");
        } else if rate_limited {
            let _ = self.publish_bytes(b"NEXAWAL | NODE RATE WAIT");
        } else {
            let _ = self.publish_progress(b"NEXAWAL | SYNC PACED");
        }
        debug_println!("UIAPP|WALLET|SYNC|PACE ns={} rate_retries={}", delay, self.sync_rate_retries);
        set_mr(0, delay);
        let _ = SYNC_TIMER_CHANNEL.pp_call(MessageInfo::new(1, 1));
    }

    fn arm_history_scan(&mut self, height: u64) {
        #[cfg(feature = "wallet-sync-timer")]
        if height > 0 && self.node.port == 443 && !self.bulk_unsupported {
            self.start_bulk_scan(height);
            return;
        }
        self.arm_get_block(height);
    }

    fn arm_get_block(&mut self, height: u64) {
        self.scan_full_fallback = false;
        self.scan_full_fallback_offset = 0;
        let mut body = [0u8; 256];
        let Some(body_len) = build_get_block_body(height, &mut body) else {
            self.sync_fail(b"NEXAWAL | SCAN BAD REQ");
            return;
        };
        let mut http = [0u8; 2048];
        let request = if self.scan_preflight(SyncRpcKind::GetBlock) {
            wallet_tcp::build_scan_preflight_http(self.node.authority(), &body[..body_len], &mut http)
        } else {
            build_http_post(self.node.authority(), "/json_rpc", &body[..body_len], &mut http)
        };
        let Some(http_len) = request else {
            self.sync_fail(b"NEXAWAL | SCAN BAD REQ");
            return;
        };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::GetBlock;
        self.scan_height = height;
        debug_println!("UIAPP|WALLET|SCAN|GET_BLOCK {}", height);
        let tip = if self.daemon_tip_height != 0 {
            self.daemon_tip_height
        } else {
            height
        };
        self.publish_tip_scan(tip, height, &[]);
        self.begin_rpc_connect();
    }

    fn arm_miner_indices(&mut self, miner_hash: &str) {
        let refs = [miner_hash];
        let mut body = [0u8; 512];
        let Some(body_len) = build_get_transactions_body(&refs, &mut body) else {
            self.sync_fail(b"NEXAWAL | SCAN BAD TX REQ");
            return;
        };
        let mut http = [0u8; 2048];
        let Some(http_len) =
            build_http_post(self.node.authority(), "/get_transactions", &body[..body_len], &mut http)
        else {
            self.sync_fail(b"NEXAWAL | SCAN BAD TX REQ");
            return;
        };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::GetMinerIndices;
        debug_println!("UIAPP|WALLET|SCAN|MINER_IDX");
        let _ = self.publish_progress(b"NEXAWAL | FETCH MINER...");
        self.begin_rpc_connect();
    }

    fn arm_tx_batch(&mut self) {
        // Tip window: one peer tx per RPC — dense tip batches hang RECV on TCG;
        // hung singles skip via tip_abandon and walk the rest.
        let tip_window = !TEST_SCAN_HEIGHTS.contains(&self.scan_height);
        let range = if !self.is_mainnet_fixture() {
            strict_tx_request_range(self.scan_batch, self.scan_hashes.len(), self.scan_full_fallback.then_some(self.scan_full_fallback_offset))
        } else {
            let batch = if tip_window { 1 } else { TX_BATCH };
            let start = self.scan_batch.saturating_mul(batch);
            (start < self.scan_hashes.len()).then_some((start, start.saturating_add(batch).min(self.scan_hashes.len())))
        };
        let Some((start, end)) = range else {
            self.finish_scan_block();
            return;
        };
        let refs: Vec<&str> = self.scan_hashes[start..end]
            .iter()
            .map(|s| s.as_str())
            .collect();
        let mut body = [0u8; 2048];
        let body_len = if !self.is_mainnet_fixture() && !self.scan_full_fallback {
            build_get_pruned_transactions_body(&refs, &mut body)
        } else {
            build_get_transactions_body(&refs, &mut body)
        };
        let Some(body_len) = body_len else {
            self.sync_fail(b"NEXAWAL | SCAN BAD TX REQ");
            return;
        };
        let mut http = [0u8; 2048];
        let Some(http_len) =
            build_http_post(self.node.authority(), "/get_transactions", &body[..body_len], &mut http)
        else {
            self.sync_fail(b"NEXAWAL | SCAN BAD TX REQ");
            return;
        };
        self.sync_req[..http_len].copy_from_slice(&http[..http_len]);
        self.sync_req_len = http_len;
        self.sync_rpc = SyncRpcKind::GetTxBatch;
        debug_println!(
            "UIAPP|WALLET|SCAN|TX_BATCH {}..{}",
            start,
            end
        );
        let _ = self.publish_progress(b"NEXAWAL | FETCH TXS...");
        self.begin_rpc_connect();
    }


    /// Split a compact batch only after it fails; verify every full transaction.
    fn fallback_to_single_full_transactions(&mut self) -> bool {
        if self.is_mainnet_fixture() || self.sync_rpc != SyncRpcKind::GetTxBatch || self.scan_full_fallback {
            return false;
        }
        self.tcp_mut().close();
        self.scan_full_fallback = true;
        self.scan_full_fallback_offset = 0;
        self.sync_recv_retries = 0;
        debug_println!("UIAPP|WALLET|SCAN|FULL_TX_FALLBACK batch={}", self.scan_batch);
        self.arm_tx_batch();
        true
    }

    /// Apply a verified block through the same wallet, journal and checkpoint path
    /// for both binary range downloads and single-block daemons.
    fn commit_restore_block(&mut self, scanned_h: u64, block: Block, txs: Vec<Transaction<Pruned>>, first: Option<u64>) -> Option<u64> {
        let Some(view) = self.state.view_pair().cloned() else {
            self.sync_fail(b"NEXAWAL | SCAN NO VIEW");
            return None;
        };
        // The bundled scanner ignores version-one outputs entirely. A
        // wallet restored across such a block could own one, so publishing
        // a complete balance would silently hide potentially spendable funds.
        if (matches!(block.miner_transaction(), Transaction::V1 { .. }) && !block.miner_transaction().prefix().outputs.is_empty())
            || txs.iter().any(|tx| matches!(tx, Transaction::V1 { .. }) && !tx.prefix().outputs.is_empty()) {
            self.sync_fail(b"NEXAWAL | SCAN FAIL | LEGACY OUTPUTS UNSUPPORTED"); return None;
        }
        let transactions = block.transactions.clone();
        let miner_hash = block.miner_transaction().hash();
        let block_hash = block.hash();
        let previous = block.header.previous;
        let Ok(outputs) = scan_assembled_strict(&view, block, txs, first) else {
            self.sync_fail(b"NEXAWAL | SCAN FAIL");
            return None;
        };
        self.commit_scan_result(walletcore_core::ScannedBlock {
            height: scanned_h, hash: block_hash, previous, miner_hash, transactions, outputs,
        })
    }

    fn commit_scan_result(&mut self, scanned: walletcore_core::ScannedBlock) -> Option<u64> {
        let walletcore_core::ScannedBlock { height: scanned_h, hash: block_hash, previous,
            miner_hash, transactions, outputs } = scanned;
        let pending_match = self.pending_evidence.as_ref().and_then(|evidence| {
            transactions.iter().copied().find(|id| evidence.matches(id))
        });
        let mut history_matches = Vec::new();
        if let Some(journal) = self.journal.as_ref() {
            for (index, entry) in journal.sent.iter().enumerate() {
                if entry.block_height == scanned_h {
                    history_matches.push((index, transactions.contains(&entry.txid)));
                }
            }
        }
        if !self.restore_chain.as_mut().is_some_and(|chain| chain.commit(scanned_h, previous, block_hash)) {
            self.reset_restore_chain();
            self.sync_fail(b"NEXAWAL | SCAN FAIL | CHAIN CHANGED");
            return None;
        }
        if let (Some(evidence), Some(chain)) = (self.pending_evidence.as_mut(), self.restore_chain.as_ref()) {
            if evidence.record_block(chain, scanned_h, block_hash, pending_match.as_slice()).is_err() {
                self.reset_restore_chain();
                self.sync_fail(b"NEXAWAL | SCAN FAIL | DUPLICATE TRANSACTION");
                return None;
            }
        }
        for (index, contains_transaction) in history_matches {
            let Some(observation) = self.history_observations.get_mut(index) else {
                self.reset_restore_chain();
                self.sync_fail(b"NEXAWAL | SCAN FAIL | HISTORY STATE"); return None;
            };
            *observation = HistoryObservation::Block { hash: block_hash, contains_transaction };
        }
        for output in &outputs {
            self.restored_outputs.push(RestoredOutput {
                amount: output.commitment().amount,
                height: scanned_h,
                coinbase: output.transaction() == miner_hash,
                timelock: output.additional_timelock(),
                spent_status: None,
            });
        }
        self.state.append_restore_outputs(&outputs);
        // Owned outputs are not spendable until all spent-status and
        // maturity checks are complete. Those checks are still pending.
        self.state.scanned = false;
        self.state.balance_atomic = 0;
        self.state.unlocked_atomic = 0;
        let Some(next) = self.restore_chain.as_ref().map(RestoreChain::next) else { self.sync_fail(b"NEXAWAL | SCAN BAD HEIGHT"); return None; };
        if next % 16 == 0 && self.journal.as_ref().is_some_and(|j|
            j.permits_empty_scan_checkpoint(self.state.outputs.len(), self.restored_outputs.len())) {
            if let (Some((_, hash)), Some(mut candidate)) =
                (self.restore_chain.as_ref().and_then(RestoreChain::anchor), self.journal.clone()) {
                candidate.empty_scan_checkpoint = Some((next, hash));
                #[cfg(feature = "wallet-range-profile")]
                let checkpoint_started = wallet_bulk::clock_us();
                let saved = self.persist_wallet_journal(&candidate);
                #[cfg(feature = "wallet-range-profile")]
                if self.sync_phase == SyncPhase::Range {
                    self.range_profile.checkpoint_us += wallet_bulk::clock_us() - checkpoint_started;
                }
                if saved {
                    self.journal = Some(candidate);
                    debug_println!("UIAPP|WALLET|EMPTY CHECKPOINT SAVED {}", next);
                }
            }
        }
        Some(next)
    }

    fn finish_scan_block(&mut self) {
        let Some(block) = self.scan_block.take() else {
            self.sync_fail(b"NEXAWAL | SCAN NO BLOCK");
            return;
        };
        let txs = core::mem::take(&mut self.scan_txs);
        let first = self.scan_first_rct;
        let scanned_h = self.scan_height;
        if !self.is_mainnet_fixture() {
            if txs.len() != self.scan_tx_first_idx.len() { self.sync_fail(b"NEXAWAL | SCAN BAD INDICES"); return; }
            let mut index = first;
            if matches!(block.miner_transaction(), Transaction::V2 { .. }) && !block.miner_transaction().prefix().outputs.is_empty() {
                index = index.and_then(|value| value.checked_add(block.miner_transaction().prefix().outputs.len() as u64));
                if index.is_none() { self.sync_fail(b"NEXAWAL | SCAN BAD INDICES"); return; }
            }
            for (tx, first_index) in txs.iter().zip(&self.scan_tx_first_idx) {
                if !matches!(tx, Transaction::V2 { .. }) || tx.prefix().outputs.is_empty() { continue; }
                if index != Some(*first_index) { self.sync_fail(b"NEXAWAL | SCAN BAD INDICES"); return; }
                let Some(next) = index.and_then(|value| value.checked_add(tx.prefix().outputs.len() as u64)) else {
                    self.sync_fail(b"NEXAWAL | SCAN BAD INDICES"); return;
                };
                index = Some(next);
            }
            if let Some(next) = self.commit_restore_block(scanned_h, block, txs, first) {
                if next == self.daemon_tip_height {
                    self.tip_locked = false;
                    self.begin_restore_spent_checks();
                } else if heap_used_bytes() >= HEAP_BYTES / 2 {
                    self.sync_fail(b"NEXAWAL | SCAN FAIL | MEMORY LIMIT");
                } else {
                    self.arm_history_scan(next);
                }
            }
            return;
        }
        let Some(view) = self.state.view_pair().cloned() else {
            self.sync_fail(b"NEXAWAL | SCAN NO VIEW");
            return;
        };
        match scan_assembled(&view, block, txs, first) {
            Ok(outputs) => {
                let n = outputs.len();
                self.state.apply_scan_outputs(&outputs);
                debug_println!(
                    "UIAPP|WALLET|SCAN|BLOCK_DONE h={} outs={} bal={} left={}",
                    scanned_h,
                    n,
                    self.state.balance_atomic,
                    self.scan_blocks_left
                );
                let tip = if self.daemon_tip_height != 0 {
                    self.daemon_tip_height
                } else {
                    scanned_h
                };
                // Short tag: outs count so tip line proves scan progress.
                let mut tag = [0u8; 8];
                tag[0] = b'O';
                let mut tn = 1usize;
                let mut v = n;
                let mut dig = [0u8; 4];
                let mut i = 0;
                if v == 0 {
                    dig[0] = b'0';
                    i = 1;
                } else {
                    while v > 0 && i < dig.len() {
                        dig[i] = b'0' + (v % 10) as u8;
                        v /= 10;
                        i += 1;
                    }
                }
                while i > 0 && tn < tag.len() {
                    i -= 1;
                    tag[tn] = dig[i];
                    tn += 1;
                }
                self.publish_tip_scan(tip, scanned_h, &tag[..tn]);
                // tip768: deposit outs found mid-window — stop walking 768
                // predecessors and go SCAN OK → decoy/prepare/send immediately.
                if self.state.balance_atomic > 0 && !self.state.outputs.is_empty() {
                    debug_println!(
                        "UIAPP|WALLET|SCAN|FUNDED_STOP h={} outs={} bal={}",
                        scanned_h,
                        n,
                        self.state.balance_atomic
                    );
                    self.tip_locked = false;
                    self.scan_blocks_left = 0;
                    self.state.scanned = true;
                    let _ = self.publish_status();
                    self.finish_with_worker(scanned_h);
                    return;
                }
                let test_wallet = self.is_mainnet_fixture();
                if test_wallet {
                    self.test_scan_i = self.test_scan_i.saturating_add(1);
                    if self.test_scan_i < TEST_SCAN_HEIGHTS.len() {
                        let next = TEST_SCAN_HEIGHTS[self.test_scan_i];
                        debug_println!("UIAPP|WALLET|SCAN|TEST_NEXT h={}", next);
                        self.arm_get_block(next);
                        return;
                    }
                    // Exactly once after the last TEST_SCAN height: tip window
                    // (peer-cap) so a fresh deposit can fund prepare/send_raw.
                    if self.test_scan_i == TEST_SCAN_HEIGHTS.len() && tip > 0 {
                        self.scan_blocks_left = TIP_HUNT_BLOCKS_LEFT;
                        self.test_scan_i = self.test_scan_i.saturating_add(1);
                        debug_println!(
                            "UIAPP|WALLET|SCAN|TIP_WINDOW tip={} n={}",
                            tip,
                            self.scan_blocks_left + 1
                        );
                        self.arm_get_block(tip);
                        return;
                    }
                }
                if self.scan_blocks_left > 0 && scanned_h > 0 {
                    self.scan_blocks_left = self.scan_blocks_left.saturating_sub(1);
                    self.arm_get_block(scanned_h.saturating_sub(1));
                } else {
                    debug_println!(
                        "UIAPP|WALLET|SCAN|DONE outs={} bal={} h={}",
                        self.state.history_count,
                        self.state.balance_atomic,
                        scanned_h
                    );
                    self.tip_locked = false;
                    // dump193: mark_scanned_empty cleared deposit outs found
                    // earlier in the tip window — keep them for prepare/sign.
                    if self.state.balance_atomic > 0 && !self.state.outputs.is_empty() {
                        self.state.scanned = true;
                        let _ = self.publish_status();
                    } else {
                        // Empty apply still marks scanned so PREPARE→INSUFFICIENT.
                        self.state.mark_scanned_empty();
                        let _ = self.publish_status();
                    }
                    // Wallet TCP Idle: join worker result (or arm if none ran).
                    // Funded path needs decoy histogram before SignableTransaction.
                    self.finish_with_worker(scanned_h);
                }
            }
            Err(()) => {
                // Assemble can fail on partial peer txs or odd miner blobs.
                // Skip forward instead of aborting (dump110/112 SCAN FAIL).
                let tip = if self.daemon_tip_height != 0 {
                    self.daemon_tip_height
                } else {
                    scanned_h
                };
                let test_wallet = self.is_mainnet_fixture();
                if test_wallet {
                    self.test_scan_i = self.test_scan_i.saturating_add(1);
                    if self.test_scan_i < TEST_SCAN_HEIGHTS.len() {
                        let next = TEST_SCAN_HEIGHTS[self.test_scan_i];
                        debug_println!(
                            "UIAPP|WALLET|SCAN|TEST_SKIP h={} -> {}",
                            scanned_h,
                            next
                        );
                        self.publish_tip_scan(tip, scanned_h, b"SKIP");
                        self.arm_get_block(next);
                        return;
                    }
                    if self.test_scan_i == TEST_SCAN_HEIGHTS.len() && tip > 0 {
                        self.scan_blocks_left = TIP_HUNT_BLOCKS_LEFT;
                        self.test_scan_i = self.test_scan_i.saturating_add(1);
                        debug_println!(
                            "UIAPP|WALLET|SCAN|TIP_WINDOW_AFTER_SKIP tip={}",
                            tip
                        );
                        self.publish_tip_scan(tip, scanned_h, b"SKIP");
                        self.arm_get_block(tip);
                        return;
                    }
                }
                let past_test = self.test_scan_i > TEST_SCAN_HEIGHTS.len();
                if past_test && self.scan_blocks_left > 0 && scanned_h > 0 {
                    debug_println!(
                        "UIAPP|WALLET|SCAN|TIP_SKIP h={} left={}",
                        scanned_h,
                        self.scan_blocks_left
                    );
                    self.scan_blocks_left = self.scan_blocks_left.saturating_sub(1);
                    self.publish_tip_scan(tip, scanned_h, b"SKIP");
                    self.arm_get_block(scanned_h.saturating_sub(1));
                    return;
                }
                self.sync_fail(b"NEXAWAL | SCAN FAIL");
            }
        }
    }

    fn sync_step_wait_connect(&mut self) {
        let node_ip = self.node_ip;
        let node_port = if cfg!(feature = "utm-nexawal-rpc-bridge") && !cfg!(feature = "wallet-https") && self.node.host() == "rpc.nexawal.com" && self.node.port == 443 { 18881 } else { self.node.port };
        // dump145b/146: TIP·SCAN … NET stuck ~4min — connect budget 8000 never
        // fired. dump146 stayed NET after GetBlock-only abandon: GetInfo tip
        // refresh also folds WAIT NET into the tip line. Cap any tip-locked
        // connect (GetInfo + tip-window scan RPCs).
        let tip_scan_rpc = matches!(
            self.sync_rpc,
            SyncRpcKind::GetBlock
                | SyncRpcKind::GetTxBatch
                | SyncRpcKind::GetMinerIndices
        ) && !TEST_SCAN_HEIGHTS.contains(&self.scan_height);
        // dump222: including bare GetInfo in tip_connect made boot WaitConnect
        // fail in 8 polls before dhcp had a lease → permanent CONNECT FAIL.
        // Tip-locked / tip-label GetInfo still fail-fast; boot GetInfo waits.
        let tip_connect = self.tip_locked
            || self.tip_label_showing()
            || tip_scan_rpc;
        // dump147/171/183: tip connect hung under TCG; shell stuck-rearm used
        // to reset WaitConnect before abandon. dump tip768: lim=8 + close-every
        // Err burned GET_BLOCK into TIP_HANG_SKIP (1 BLOCK_OK / 35+ skips) and
        // walked past H3803776 without scanning. Allow open() stale-clear + Cake
        // SYN under TCG before skip.
        // SYN→ESTABLISHED under TCG often needs longer than 96 (SYN_RCVD then FAIL).
        const TIP_CONNECT_POLL_MAX: u32 = 384;
        // Boot GetInfo: allow dhcp settle on TCG (NOT_READY → WAIT NET retries).
        // dump222: 512 still CONNECT FAIL ~2min on cold dhcp; keep longer.
        const BOOT_GETINFO_CONNECT_POLL_MAX: u32 = 4_096;
        let connect_lim = if tip_connect {
            TIP_CONNECT_POLL_MAX
        } else if self.sync_rpc == SyncRpcKind::GetInfo {
            BOOT_GETINFO_CONNECT_POLL_MAX
        } else {
            SYNC_CONNECT_POLL_MAX
        };
        // d80c96a1: deferring OPEN until Err%25 meant STATUS-on-IDLE (CLOSED)
        // spun for 15m+ with no OPEN if the Err path never armed — or hung
        // inside the first STATUS PPC before WAIT NET painted. Issue OPEN
        // after one WaitConnect bounce (shell already shows CONNECTING), then
        // STATUS-poll. Retry OPEN on NOT_READY/CLOSED every 25.
        if !self.tcp_open_sent {
            if self.sync_polls == 0 {
                self.sync_polls = 1;
                for _ in 0..2_000 {
                    sel4::r#yield();
                }
                return;
            }
            // publish_bytes so boot GetInfo WAIT NET is visible (not tip-folded).
            if self.sync_rpc != SyncRpcKind::GetFeeEstimate { let _ = self.publish_bytes(b"NEXAWAL | SYNC WAIT NET"); }
            match self.tcp_mut().open(node_ip, node_port) {
                Ok(()) => {
                    self.tcp_open_sent = true;
                    debug_println!("UIAPP|WALLET|TCP|OPEN_ARMED");
                }
                Err(()) => {
                    // dhcp NOT_READY — keep trying on later notifies.
                    self.sync_polls = self.sync_polls.saturating_add(1);
                    if self.sync_polls >= connect_lim {
                        if tip_connect {
                            self.tip_locked = false;
                        }
                        self.sync_fail(b"NEXAWAL | SYNC CONNECT FAIL");
                        return;
                    }
                    self.tcp_mut().pause_poll();
                    return;
                }
            }
        }
        match self.tcp_mut().poll_connected() {
            Ok(true) => {
                // Request body is already armed by sync_step_start / arm_*.
                // Do not rebuild get_info here — that clobbers get_block / tx batches.
                if self.sync_req_len == 0 {
                    self.sync_fail(b"NEXAWAL | SYNC BAD REQ");
                    return;
                }
                self.sync_resp_len = 0;
                self.sync_polls = 0;
                self.sync_phase = SyncPhase::Send;
                debug_println!(
                    "UIAPP|WALLET|TCP|CONNECTED rpc={} req={}",
                    self.sync_rpc as u8,
                    self.sync_req_len
                );
                let _ = self.publish_progress(b"NEXAWAL | SYNC CONNECTED");
            }
            Ok(false) => {
                self.sync_polls = self.sync_polls.saturating_add(1);
                // Boot GetInfo: STATUS every 2k yields never left enough CPU for
                // the net PD to finish Cake's SYN→ESTABLISHED (r9: WAIT→4096→
                // FAIL, never HASKELL_TCP|connected). Use the same off-CPU gap
                // as agent STATUS polls so dhcp/lwIP timers and RX can run.
                // Tip-locked paths keep the short yield so tip abandon stays snappy.
                if !tip_connect && self.sync_rpc == SyncRpcKind::GetInfo {
                    self.tcp_mut().pause_poll();
                } else {
                    // dump222: long pause_poll here starved the shell bounce on TCG
                    // so CONNECTING UI froze and connect_lim never painted FAIL.
                    for _ in 0..2_000 {
                        sel4::r#yield();
                    }
                }
                // Stuck SYN (usernet SYN_RCVD, no ESTABLISHED): close+reOPEN so
                // dhcp/Cake can try a fresh handshake before connect_lim.
                // Quiet-host TCG burns 128 polls in ~1–2s and aborts Cake's SYN
                // before ESTABLISHED (r8: OPEN_OK×N → CONNECT FAIL, never
                // CONNECTED). Boot GetInfo needs a longer reopen period.
                let reopen_every: u32 = if !tip_connect
                    && self.sync_rpc == SyncRpcKind::GetInfo
                {
                    768
                } else {
                    256
                };
                if self.sync_polls > 0 && self.sync_polls % reopen_every == 0 {
                    debug_println!(
                        "UIAPP|WALLET|TCP|SYN_REOPEN polls={} every={}",
                        self.sync_polls,
                        reopen_every
                    );
                    self.tcp_mut().close();
                    self.tcp_open_sent = false;
                    let _ = self.publish_progress(b"NEXAWAL | SYNC WAIT NET");
                    for _ in 0..8_000 {
                        sel4::r#yield();
                    }
                    if self.tcp_mut().open(node_ip, node_port).is_ok() {
                        self.tcp_open_sent = true;
                        debug_println!("UIAPP|WALLET|TCP|OPEN_OK");
                    }
                } else if self.sync_polls % 50 == 0 {
                    debug_println!(
                        "UIAPP|WALLET|TCP|WAIT pending polls={}",
                        self.sync_polls
                    );
                    let _ = self.publish_progress(b"NEXAWAL | SYNC WAIT NET");
                }
                if self.sync_polls >= connect_lim {
                    if tip_scan_rpc && self.tip_abandon_peers_and_finish() {
                        return;
                    }
                    // GetInfo / tip-locked connect: clear tip lock and fail so
                    // shell tip rearm can retry (dump146 GetInfo NET freeze).
                    if tip_connect {
                        self.tip_locked = false;
                    }
                    // publish_bytes (not progress) so shell peek always sees FAIL
                    self.sync_fail(b"NEXAWAL | SYNC CONNECT FAIL");
                }
            }
            Err(()) => {
                // OPEN/STATUS failed (often NOT_READY before dhcp lease). Retry.
                // tip768: do NOT close+reopen every Err — that thrashed Cake and
                // hit TIP_HANG_SKIP before SYN completed. Re-OPEN periodically;
                // open() clears OPEN_BUSY_STALE (PEER_CLOSED+rx) itself.
                // r13: Err%8 OPEN stormed qemu usernet into dual TCP[SYN_RCVD]
                // toward Cake then Connection-aborted — wait much longer between
                // boot GetInfo re-OPENs so one handshake can finish.
                self.sync_polls = self.sync_polls.saturating_add(1);
                if self.sync_polls >= connect_lim {
                    if tip_scan_rpc && self.tip_abandon_peers_and_finish() {
                        return;
                    }
                    if tip_connect {
                        self.tip_locked = false;
                    }
                    self.sync_fail(b"NEXAWAL | SYNC CONNECT FAIL");
                    return;
                }
                let reopen_err_every: u32 = if !tip_connect
                    && self.sync_rpc == SyncRpcKind::GetInfo
                {
                    384
                } else {
                    32
                };
                if self.sync_polls % reopen_err_every == 0 {
                    if self.sync_rpc != SyncRpcKind::GetFeeEstimate { let _ = self.publish_bytes(b"NEXAWAL | SYNC WAIT NET"); }
                    self.tcp_open_sent = false;
                    if self.tcp_mut().open(node_ip, node_port).is_ok() {
                        self.tcp_open_sent = true;
                        debug_println!(
                            "UIAPP|WALLET|TCP|OPEN_AFTER_ERR polls={} every={}",
                            self.sync_polls,
                            reopen_err_every
                        );
                    }
                }
                self.tcp_mut().pause_poll();
            }
        }
    }

    fn sync_step_send(&mut self) {
        // BSS copy — sync_req can be a full send_raw HTTP body (~10KiB+).
        static mut SEND_COPY: [u8; 16384] = [0; 16384];
        let req_len = self.sync_req_len;
        if req_len == 0 || req_len > self.sync_req.len() {
            self.sync_fail(b"NEXAWAL | SYNC BAD REQ");
            return;
        }
        let req_copy = unsafe { &mut *core::ptr::addr_of_mut!(SEND_COPY) };
        req_copy[..req_len].copy_from_slice(&self.sync_req[..req_len]);
        #[cfg(feature = "wallet-https")]
        if self.node.port == 443 {
            let Some(now) = wallet_tls_platform::unix_time() else {
                self.sync_fail(b"NEXAWAL | CLOCK UNAVAILABLE");
                return;
            };
            match nexatrode_guest_https::Session::new(self.node.host(), now, &req_copy[..req_len], 512 * 1024) {
                Ok(session) => {
                    let preflight = self.scan_preflight(self.sync_rpc).then(|| {
                        (self.node, self.node_ip, nexatrode_guest_https::binary_response::BufferedRpcResponse::new(512 * 1024))
                    });
                    self.tls_session = Some(RpcTls { session, preflight });
                }
                Err(error) => {
                    debug_println!("UIAPP|WALLET|SYNC|HTTPS_INIT_FAILURE kind={:?}", error.kind());
                    self.sync_fail(b"NEXAWAL | TLS INIT FAILED");
                    return;
                }
            }
            self.sync_resp.clear();
            self.sync_resp_len = 0;
            self.sync_polls = 0;
            self.sync_recv_stall = 0;
            self.sync_phase = SyncPhase::Recv;
            let _ = self.publish_progress(b"NEXAWAL | SECURE CONNECTING");
            return;
        }
        if self.tcp_mut().send_all(&req_copy[..req_len]).is_err() {
            self.sync_fail(b"NEXAWAL | SYNC SEND FAIL");
            return;
        }
        debug_println!("UIAPP|WALLET|SYNC|SENT {}", req_len);
        // get_transactions batches for busy tip blocks exceed 64KiB.
        self.sync_resp.clear();
        self.sync_resp.resize(512 * 1024, 0);
        self.sync_resp_len = 0;
        self.sync_polls = 0;
        self.sync_recv_stall = 0;
        self.sync_phase = SyncPhase::Recv;
        let _ = self.publish_progress(b"NEXAWAL | SYNC RECV...");
    }

    fn sync_step_recv(&mut self) {
        #[cfg(feature = "wallet-https")]
        if self.node.port == 443 {
            let Some(RpcTls { mut session, mut preflight }) = self.tls_session.take() else {
                self.sync_fail(b"NEXAWAL | TLS STATE LOST");
                return;
            };
            let progress_before = session.wire_progress();
            // Process a small bounded batch so a 128-byte transport does not
            // require one UI round-trip per TLS fragment. Yield between polls
            // to let the network protection domain and renderer run.
            for _ in 0..8 {
                match session.poll(self.tcp_mut(), |response| {
                    let ordinary = |r: &[u8]| walletcore_core::https_rpc_response_complete(r, http_response_complete);
                    match preflight.as_mut() {
                        Some((_, _, reply)) => reply.complete(response, ordinary),
                        None => ordinary(response),
                    }
                }) {
                    Ok(nexatrode_guest_https::Progress::Pending) => {
                        for _ in 0..32 { sel4::r#yield(); }
                    }
                    Ok(nexatrode_guest_https::Progress::Complete(response)) => {
                        self.sync_resp = response;
                        self.sync_resp_len = self.sync_resp.len();
                        let mut status = http_response_status(&self.sync_resp);
                        let mut framed = None;
                        let mut reusable_peer = None;
                        if let Some((node, ip, reply)) = preflight.take() {
                            if !reply.close_delimited() {
                                match reply.finish() {
                                    Ok((body, persistent)) => {
                                        status = Some(200);
                                        framed = Some(body);
                                        reusable_peer = persistent.then_some((node, ip));
                                    }
                                    Err(nexatrode_guest_https::binary_response::RangeError::Http(code)) => status = Some(code),
                                    Err(_) => {
                                        self.tcp_mut().close();
                                        self.sync_fail(b"NEXAWAL | BAD HTTPS");
                                        return;
                                    }
                                }
                            }
                        }
                        match status {
                            Some(200) => {}
                            Some(429) => {
                                self.tcp_mut().close();
                                #[cfg(feature = "wallet-sync-timer")]
                                if self.scan_request_is_read_only() {
                                    self.schedule_scan_request(true);
                                    return;
                                }
                                self.sync_fail(b"NEXAWAL | SYNC FAIL | NODE RATE LIMITED");
                                return;
                            }
                            status => {
                                debug_println!("UIAPP|WALLET|SYNC|HTTPS_STATUS status={:?} rpc={}", status, self.sync_rpc as u8);
                                if status.is_some_and(walletcore_core::transient_daemon_status)
                                    && self.retry_read_only_network_failure()
                                {
                                    return;
                                }
                                self.sync_fail(b"NEXAWAL | SYNC FAIL | NODE HTTP ERROR");
                                return;
                            }
                        }
                        let body = match framed.or_else(|| http_body(&self.sync_resp).map(<[u8]>::to_vec)) {
                            Some(json) => json,
                            None => { self.tcp_mut().close(); self.sync_fail(b"NEXAWAL | BAD HTTPS"); return; }
                        };
                        if self.sync_rpc == SyncRpcKind::GetBlock
                            && !TEST_SCAN_HEIGHTS.contains(&self.scan_height)
                            && !body.windows(6).any(|w| w == b"\"blob\"")
                        {
                            self.tcp_mut().close();
                            if self.sync_retry_recv("HTTPS_NO_BLOB") {
                                let _ = self.publish_progress(b"NEXAWAL | SCAN RETRY BLOB");
                            } else {
                                self.sync_fail(b"NEXAWAL | SCAN NO BLOB");
                            }
                            return;
                        }
                        #[cfg(feature = "wallet-sync-timer")]
                        if let Some((node, ip)) = reusable_peer {
                            // The RPC must create a bulk scan through the usual height,
                            // network and anchor checks before handing it the socket.
                            self.handle_rpc_body(&body);
                            if self.sync_phase == SyncPhase::Range {
                                if let Some(scan) = self.bulk.as_mut() {
                                    scan.adopt_preflight(session, node, ip);
                                    return;
                                }
                            }
                            self.tcp_mut().close();
                            return;
                        }
                        self.tcp_mut().close();
                        self.handle_rpc_body(&body);
                        return;
                    }
                    Err(nexatrode_guest_https::Error::BufferLimit) if self.fallback_to_single_full_transactions() => {
                        return;
                    }
                    Err(error) => {
                        let kind = error.kind();
                        debug_println!("UIAPP|WALLET|SYNC|HTTPS_FAILURE kind={:?} rpc={}", kind, self.sync_rpc as u8);
                        if kind.transient() && self.retry_read_only_network_failure() {
                            return;
                        }
                        self.sync_fail(b"NEXAWAL | TLS FAILED");
                        return;
                    }
                }
            }
            let progress_after = session.wire_progress();
            self.tls_session = Some(RpcTls { session, preflight });
            self.sync_polls = next_tls_stall_polls(self.sync_polls, progress_before, progress_after);
            if self.sync_polls >= 65_536 {
                debug_println!("UIAPP|WALLET|TLS|STALL bytes={} rpc={}", progress_after, self.sync_rpc as u8);
                self.tls_session = None;
                let read_only_scan = matches!(self.sync_rpc,
                    SyncRpcKind::GetInfo | SyncRpcKind::GetBlock
                    | SyncRpcKind::GetMinerIndices | SyncRpcKind::GetTxBatch);
                if read_only_scan && self.sync_retry_recv("TLS_TIMEOUT") {
                    let _ = self.publish_progress(b"NEXAWAL | SYNC RETRY TLS");
                } else if self.fallback_to_single_full_transactions() {
                    return;
                } else {
                    self.tcp_mut().close();
                    self.sync_fail(b"NEXAWAL | TLS TIMEOUT");
                }
            } else {
                self.tcp_mut().pause_poll();
            }
            return;
        }
        let mut filled = self.sync_resp_len;
        if filled > self.sync_resp.len() {
            self.sync_fail(b"NEXAWAL | SYNC BAD HTTP");
            return;
        }
        let filled_before = filled;
        // r21: after HASKELL_TCP|connected, boot GetInfo SENT then peer-closed
        // with only one 128B RECV per shell notify — serial froze in phase=4
        // and never reached tip/deposit. Drain every Recv the same way as
        // deposit get_block/get_tx (those stalled at filled=2880 for the same
        // one-page-per-notify reason). Yield so eth/dhcp can refill.
        let deposit_heavy = self.scan_height == DEPOSIT_HINT_HEIGHT
            && matches!(
                self.sync_rpc,
                SyncRpcKind::GetBlock | SyncRpcKind::GetTxBatch
            );
        let drain_rounds: u32 = if deposit_heavy { 900 } else { 120 };
        let idle_lim: u32 = if deposit_heavy { 28 } else { 16 };
        let mut idle = 0u32;
        let mut last = filled;
        let mut st = Ok(false);
        for _ in 0..drain_rounds {
            st = {
                let buf = &mut self.sync_resp[..];
                self.tcp.as_mut().unwrap().poll_recv(buf, &mut filled)
            };
            match st {
                Ok(true) => break,
                Err(()) => break,
                Ok(false) => {
                    self.sync_resp_len = filled;
                    if filled > last {
                        last = filled;
                        idle = 0;
                        if filled >= 200
                            && http_response_complete(&self.sync_resp[..filled])
                        {
                            st = Ok(true);
                            break;
                        }
                        // Yield enough for the network PD to refill, without
                        // performing hundreds of yields for every 128 bytes.
                        for _ in 0..32 {
                            sel4::r#yield();
                        }
                        continue;
                    }
                    idle = idle.saturating_add(1);
                    self.tcp_mut().pause_poll();
                    if idle >= idle_lim {
                        break;
                    }
                }
            }
        }
        if matches!(st, Ok(true)) {
            debug_println!("UIAPP|WALLET|TCP|DRAIN_DONE filled={}", filled);
        } else if filled > filled_before {
            debug_println!(
                "UIAPP|WALLET|TCP|DRAIN_PARTIAL filled={} idle={}",
                filled,
                idle
            );
        }
        let status = st;
        match status {
            Ok(true) => {
                self.sync_resp_len = filled;
                let tip_block = self.sync_rpc == SyncRpcKind::GetBlock
                    && !TEST_SCAN_HEIGHTS.contains(&self.scan_height);
                let body = {
                    let Some(json) = http_body(&self.sync_resp[..filled]) else {
                        debug_println!("UIAPP|WALLET|SYNC|BAD_HTTP len={}", filled);
                        // Empty/partial close before headers — common while dhcp
                        // is still settling; reopen and resend a few times.
                        self.tcp_mut().close();
                        if self.sync_retry_recv("BAD_HTTP") {
                            let _ = self.publish_progress(b"NEXAWAL | SYNC RETRY HTTP");
                            return;
                        }
                        // Deposit tip GetTxBatch: don't Idle-fail — skip/retry the
                        // hung peer hash so remaining outs can still fund prepare.
                        if self.tip_abandon_peers_and_finish() {
                            return;
                        }
                        self.sync_fail(b"NEXAWAL | SYNC BAD HTTP");
                        return;
                    };
                    // tip768: poll_recv Ok(true) on brace-balanced truncate without
                    // "blob" — do not handle_rpc_body→NO_BLOB hang-skip yet.
                    if tip_block && !json.windows(6).any(|w| w == b"\"blob\"") {
                        debug_println!(
                            "UIAPP|WALLET|SYNC|DONE_NO_BLOB filled={} — retry",
                            filled
                        );
                        self.tcp_mut().close();
                        if self.sync_retry_recv("NO_BLOB") {
                            let _ = self.publish_progress(b"NEXAWAL | SCAN RETRY BLOB");
                            return;
                        }
                        if self.tip_abandon_peers_and_finish() {
                            return;
                        }
                        self.sync_fail(b"NEXAWAL | SCAN NO BLOB");
                        return;
                    }
                    json.to_vec()
                };
                self.tcp_mut().close();
                self.handle_rpc_body(&body);
            }
            Ok(false) => {
                self.sync_resp_len = filled;
                self.sync_polls = self.sync_polls.saturating_add(1);
                if filled > filled_before {
                    self.sync_recv_stall = 0;
                } else {
                    self.sync_recv_stall = self.sync_recv_stall.saturating_add(1);
                }
                // get_info is ~1.4KiB — finish as soon as height is visible.
                // Scan the whole recv buffer for "height" (not only http_body),
                // in case header/body framing is missing on the wire.
                if self.sync_rpc == SyncRpcKind::GetInfo && filled >= 900 {
                    let slice = &self.sync_resp[..filled];
                    let json = http_body(slice).unwrap_or(slice);
                    if parse_daemon_height(json).is_some() {
                        let body = if let Some(i) = json.iter().position(|&b| b == b'{') {
                            json[i..].to_vec()
                        } else {
                            json.to_vec()
                        };
                        debug_println!(
                            "UIAPP|WALLET|SYNC|GETINFO_EARLY filled={}",
                            filled
                        );
                        self.tcp_mut().close();
                        self.handle_rpc_body(&body);
                        return;
                    }
                    // Sticky diagnostic — publish_bytes so RECV chatter cannot
                    // hide it on the next poll (shell peeks transfer).
                    if filled_before < 2048 && filled >= 2048 {
                        let brace_at = slice.iter().position(|&b| b == b'{');
                        let mut height_at = None;
                        let needle = b"\"height\"";
                        if slice.len() >= needle.len() {
                            for i in 0..=(slice.len() - needle.len()) {
                                if &slice[i..i + needle.len()] == needle {
                                    height_at = Some(i);
                                    break;
                                }
                            }
                        }
                        let mut msg = [0u8; 40];
                        // "WIRE b=NNN h=NNN" or "none"
                        msg[..7].copy_from_slice(b"WIRE b=");
                        let mut n = 7usize;
                        fn push_num(msg: &mut [u8], n: &mut usize, v: usize) {
                            let mut dig = [0u8; 6];
                            let mut x = v;
                            let mut k = 0usize;
                            if x == 0 {
                                dig[0] = b'0';
                                k = 1;
                            } else {
                                while x > 0 && k < dig.len() {
                                    dig[k] = b'0' + (x % 10) as u8;
                                    x /= 10;
                                    k += 1;
                                }
                            }
                            while k > 0 && *n < msg.len() {
                                k -= 1;
                                msg[*n] = dig[k];
                                *n += 1;
                            }
                        }
                        match brace_at {
                            Some(i) => push_num(&mut msg, &mut n, i),
                            None => {
                                if n + 4 <= msg.len() {
                                    msg[n..n + 4].copy_from_slice(b"none");
                                    n += 4;
                                }
                            }
                        }
                        if n + 3 <= msg.len() {
                            msg[n..n + 3].copy_from_slice(b" h=");
                            n += 3;
                        }
                        match height_at {
                            Some(i) => push_num(&mut msg, &mut n, i),
                            None => {
                                if n + 4 <= msg.len() {
                                    msg[n..n + 4].copy_from_slice(b"none");
                                    n += 4;
                                }
                            }
                        }
                        let _ = self.publish_progress(&msg[..n]);
                    }
                }
                // One-shot peek — must not erase TIP · SCAN after get_info.
                if filled_before < 64 && filled >= 64 {
                    let mut msg = [0u8; 40];
                    let p = b"NEXAWAL | RX ";
                    msg[..p.len()].copy_from_slice(p);
                    let mut n = p.len();
                    for &b in self.sync_resp[..filled].iter().take(24) {
                        if n >= msg.len() {
                            break;
                        }
                        msg[n] = match b {
                            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'{' | b':'
                            | b'"' | b' ' => b,
                            b'\r' | b'\n' => b'|',
                            _ => b'.',
                        };
                        n += 1;
                    }
                    let _ = self.publish_progress(&msg[..n]);
                }
                // Finish any RPC once Content-Length/JSON is whole (keep-alive /
                // missing header separator). Do not wait for FIN or stall.
                if filled >= 900 && http_response_complete(&self.sync_resp[..filled]) {
                    if let Some(json) = http_body(&self.sync_resp[..filled]) {
                        // Tip get_block: do not finish on a brace-balanced truncated
                        // body that lacks "blob" (dump tip768 NO_BLOB→hang-skip).
                        let tip_block = self.sync_rpc == SyncRpcKind::GetBlock
                            && !TEST_SCAN_HEIGHTS.contains(&self.scan_height);
                        let has_blob = json.windows(6).any(|w| w == b"\"blob\"");
                        if tip_block && !has_blob {
                            debug_println!(
                                "UIAPP|WALLET|SYNC|WAIT_BLOB filled={}",
                                filled
                            );
                        } else {
                            let body = json.to_vec();
                            debug_println!(
                                "UIAPP|WALLET|SYNC|HTTP_COMPLETE rpc={} filled={}",
                                self.sync_rpc as u8,
                                filled
                            );
                            self.tcp_mut().close();
                            self.handle_rpc_body(&body);
                            return;
                        }
                    }
                }
                if self.sync_recv_stall >= 40 {
                    let stalled_body = http_body(&self.sync_resp[..filled])
                        .filter(|json| json_object_complete(json))
                        .map(|json| json.to_vec());
                    if let Some(body) = stalled_body {
                        debug_println!(
                            "UIAPP|WALLET|SYNC|STALL_COMPLETE filled={}",
                            filled
                        );
                        self.tcp_mut().close();
                        self.handle_rpc_body(&body);
                        return;
                    }
                }
                // Let dhcp/ethernet run — shell may re-notify immediately.
                self.tcp_mut().pause_poll();
                if self.sync_polls % 50 == 0 {
                    debug_println!(
                        "UIAPP|WALLET|TCP|RECV pending polls={} filled={}",
                        self.sync_polls,
                        filled
                    );
                    let mut msg = [0u8; 48];
                    // rpc discriminant: 0=GetInfo 1=GetBlock 2=Miner 3=Txs ...
                    msg[0] = b'R';
                    msg[1] = b'0' + (self.sync_rpc as u8).min(9);
                    msg[2] = b' ';
                    let prefix = b"RECV ";
                    msg[3..3 + prefix.len()].copy_from_slice(prefix);
                    let mut n = 3 + prefix.len();
                    let mut v = filled;
                    if v == 0 {
                        msg[n] = b'0';
                        n += 1;
                    } else {
                        let mut dig = [0u8; 10];
                        let mut i = 0;
                        while v > 0 && i < dig.len() {
                            dig[i] = b'0' + (v % 10) as u8;
                            v /= 10;
                            i += 1;
                        }
                        while i > 0 {
                            i -= 1;
                            msg[n] = dig[i];
                            n += 1;
                        }
                    }
                    if filled > 0 && n + 1 < msg.len() {
                        msg[n] = b' ';
                        n += 1;
                        for &b in self.sync_resp[..filled].iter().take(20) {
                            if n >= msg.len() {
                                break;
                            }
                            msg[n] = match b {
                                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'{'
                                | b':' | b'"' | b' ' | b'-' => b,
                                b'\r' | b'\n' => b'|',
                                _ => b'.',
                            };
                            n += 1;
                        }
                    }
                    let _ = self.publish_progress(&msg[..n]);
                }
                // Mid-body stalls (~50KiB) are common on TCG keep-alive; reconnect
                // the same request sooner instead of spinning until TIMEOUT.
                // Tip get_block/tx: abandon sooner so tip_mid cannot thrash us
                // into CONNECTING before hang-skip (dump125).
                let tip_scan = matches!(
                    self.sync_rpc,
                    SyncRpcKind::GetBlock
                        | SyncRpcKind::GetTxBatch
                        | SyncRpcKind::GetMinerIndices
                ) && !TEST_SCAN_HEIGHTS.contains(&self.scan_height);
                // dump147: GetInfo tip-refresh Recv also froze under tip_locked
                // with CONNECTED folded to NET — cap tip-locked GetInfo too.
                // dump222: boot GetInfo (tip_locked still false, no TIP label)
                // stayed SYNC RECV... for minutes — tip_recv was false so only
                // SYNC_CONNECT_POLL_MAX=8000 applied; bounce died mid-recv and
                // FB froze. Cap every GetInfo the same as tip-locked GetInfo.
                let tip_recv = tip_scan || self.sync_rpc == SyncRpcKind::GetInfo;
                // dump133: tip GetBlock RX froze ~4min on same height — trickle
                // bytes reset sync_recv_stall so stall_lim never fired. Cap tip
                // polls hard; stall alone is not enough on TCG keep-alive.
                // dump211: 80 left tip RECV frozen ~2min on TCG (await tip_mid
                // quiet then stale-kick). Cap sooner so hang-skip reaches SCAN 0
                // while a fresh deposit is still in the tip window.
                // tip768: 32 polls left get_block mid-blob → NO_BLOB → hang-skip.
                // Empty tip blocks are tiny; deposit H3803776 has 21 txs — wait.
                // MR2M boot: deposit get_block hit TIP_POLL_CAP at filled=23KiB
                // (polls=192) then hang-skipped past the funded height.
                // Deposit block (~21 txs): GetBlock blob AND per-tx GetTxBatch
                // need a longer TCG poll/stall budget than empty tip heights.
                let deposit_tx = self.scan_height == DEPOSIT_HINT_HEIGHT
                    && matches!(
                        self.sync_rpc,
                        SyncRpcKind::GetBlock | SyncRpcKind::GetTxBatch
                    );
                let tip_recv_max: u32 = if deposit_tx {
                    4096
                } else {
                    192
                };
                // Deposit GetTxBatch often lands CONNECTED+SENT then blackholes
                // (usernet FIN_WAIT_1, filled=0). Waiting stall 512 under TCG
                // burned ~15m per hung peer hash. Fail faster when no bytes
                // arrive so DEPOSIT_TX_RETRY can reclaim the batch.
                let stall_lim = if deposit_tx {
                    if filled == 0 {
                        96
                    } else {
                        512
                    }
                } else if tip_scan {
                    48
                } else if self.sync_rpc == SyncRpcKind::GetInfo {
                    8
                } else {
                    80
                };
                if tip_recv && self.sync_polls >= tip_recv_max {
                    debug_println!(
                        "UIAPP|WALLET|SCAN|TIP_POLL_CAP polls={} stall={} filled={}",
                        self.sync_polls,
                        self.sync_recv_stall,
                        filled
                    );
                    if tip_scan && self.tip_abandon_peers_and_finish() {
                        return;
                    }
                    if tip_recv && self.sync_rpc == SyncRpcKind::GetInfo {
                        self.tip_locked = false;
                        self.sync_fail(b"NEXAWAL | SYNC RECV FAIL");
                        return;
                    }
                    if !self.tip_abandon_peers_and_finish() {
                        if !self.sync_retry_recv("TIP_CAP") {
                            self.sync_fail(b"NEXAWAL | SYNC RECV FAIL");
                        }
                    }
                    return;
                }
                if self.sync_recv_stall >= stall_lim {
                    // Tip: abandon on first stall budget — TCG get_block often
                    // never completes; SCAN 0 + INSUFFICIENT→refresh retries.
                    if tip_scan {
                        if !self.tip_abandon_peers_and_finish() {
                            if !self.sync_retry_recv("STALL") {
                                self.sync_fail(b"NEXAWAL | SYNC RECV FAIL");
                            }
                        }
                        return;
                    }
                    if !self.sync_retry_recv("STALL") {
                        if !self.tip_abandon_peers_and_finish() {
                            self.sync_fail(b"NEXAWAL | SYNC RECV FAIL");
                        }
                    }
                    return;
                }
                if self.sync_polls >= SYNC_CONNECT_POLL_MAX {
                    if !self.sync_retry_recv("TIMEOUT") {
                        if !self.tip_abandon_peers_and_finish() {
                            self.sync_fail(b"NEXAWAL | SYNC RECV FAIL");
                        }
                    }
                }
            }
            Err(()) => {
                // Brief yield then retry — a single ERROR often races dhcp RX.
                self.tcp_mut().pause_poll();
                if !self.sync_retry_recv("ERR") {
                    if !self.tip_abandon_peers_and_finish() {
                        self.sync_fail(b"NEXAWAL | SYNC RECV FAIL");
                    }
                }
            }
        }
    }

    fn handle_rpc_body(&mut self, json: &[u8]) {
        self.sync_recv_retries = 0;
        #[cfg(feature = "wallet-sync-timer")]
        { self.sync_rate_retries = 0; }
        match self.sync_rpc {
            SyncRpcKind::GetInfo => {
                let network = self.creation_height_lookup.unwrap_or(self.state.network);
                if !daemon_network_matches(json, network) {
                    self.node_verified = false;
                    self.sync_fail(b"NEXAWAL | SYNC OPEN FAIL | NODE NETWORK MISMATCH");
                    return;
                }
                let Some(height) = parse_daemon_height(json) else {
                    let mut msg = [0u8; 48];
                    let p = b"BAD JSON ";
                    msg[..p.len()].copy_from_slice(p);
                    let mut n = p.len();
                    for &b in json.iter().take(36) {
                        if n >= msg.len() {
                            break;
                        }
                        msg[n] = match b {
                            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'{' | b'}' | b':' | b'"'
                            | b',' | b' ' | b'_' => b,
                            b'\r' | b'\n' => b'|',
                            _ => b'.',
                        };
                        n += 1;
                    }
                    self.sync_fail(&msg[..n]);
                    return;
                };
                if let Some(network) = self.creation_height_lookup.take() {
                    self.tcp_mut().close();
                    self.tcp_open_sent = false;
                    self.sync_phase = SyncPhase::Idle;
                    // Scan a small recent buffer as well, so a reorg or a node
                    // slightly behind the chain tip cannot skip early deposits.
                    let restore_height = height.saturating_sub(100);
                    if height == 0 || !self.generate_pending_wallet(network, restore_height) {
                        let _ = self.publish_bytes(b"NEXAWAL | CREATION FAILED");
                    } else {
                        debug_println!("UIAPP|WALLET|BACKUP READY AT HEIGHT {}", restore_height);
                    }
                    WALLET_SHELL_CHANNEL.notify();
                    return;
                }
                self.node_verified = true;
                if !self.is_mainnet_fixture() {
                    let Some(chain) = self.restore_chain else { self.sync_fail(b"NEXAWAL | SCAN FAIL | NO RESTORE HEIGHT"); return; };
                    let next = chain.next();
                    self.daemon_tip_height = height;
                    if next > height {
                        self.reset_restore_chain();
                        self.sync_fail(b"NEXAWAL | SCAN FAIL | RESTORE HEIGHT AHEAD OR REORG");
                        return;
                    }
                    // Recheck the last scanned block even when no new blocks
                    // exist; a same-height reorg must invalidate old outputs.
                    if let Some((anchor, _)) = chain.anchor() {
                        self.verify_restore_anchor = true;
                        self.anchor_after_spent_checks = false;
                        self.arm_get_block(anchor);
                        return;
                    }
                    if next == height {
                        if height == 0 { self.sync_fail(b"NEXAWAL | SCAN FAIL | NO CHAIN ANCHOR"); return; }
                        self.seed_restore_anchor = true;
                        self.arm_get_block(height - 1);
                        return;
                    }
                    if heap_used_bytes() >= HEAP_BYTES / 2 { self.sync_fail(b"NEXAWAL | SCAN FAIL | MEMORY LIMIT"); return; }
                    self.state.scanned = false;
                    self.state.balance_atomic = 0;
                    self.state.unlocked_atomic = 0;
                    self.scan_hashes.clear();
                    self.scan_txs.clear();
                    self.scan_tx_first_idx.clear();
                    self.scan_batch = 0;
                    self.scan_block = None;
                    self.scan_first_rct = None;
                    self.arm_history_scan(next);
                    return;
                }
                refresh_stub(&mut self.state, height);
                debug_println!("UIAPP|WALLET|SYNC|HEIGHT {}", height);
                if self.change_hint_spent && self.state.balance_atomic == 0 {
                    debug_println!("UIAPP|WALLET|SYNC|SPENT_DONE tip={}", height);
                    let _ = self.publish_bytes(b"NEXAWAL | NEED DEPOSIT | BAL 0");
                    self.sync_phase = SyncPhase::Idle;
                    return;
                }
                // Prefer known funded heights for the test restore wallet; tip-only
                // SCAN_BLOCKS never reaches receives hundreds of k-blocks below tip.
                let test_wallet = self.is_mainnet_fixture();
                // tip768: after FUNDED_STOP, shell tip-rearm GetInfo re-entered
                // tip walk and re-scanned H3803776 (bal doubled) while decoy
                // was mid-fetch. Stay funded → SCAN OK / decoy, no tip re-hunt.
                if test_wallet
                    && self.state.balance_atomic > 0
                    && !self.state.outputs.is_empty()
                {
                    debug_println!(
                        "UIAPP|WALLET|SYNC|ALREADY_FUNDED bal={} outs={}",
                        self.state.balance_atomic,
                        self.state.outputs.len()
                    );
                    self.tip_locked = false;
                    self.daemon_tip_height = height;
                    self.state.scanned = true;
                    let _ = self.publish_status();
                    self.finish_with_worker(height);
                    return;
                }
                // Tip-mid refresh used to reset test_scan_i→0 every time, so
                // hang-skip never stuck and SCAN 3794104 RX looped forever
                // (dump119). Resume mid-walk; after the walk, tip-window only.
                // dump189: tip window was only tip+2 — a deposit a few blocks
                // behind tip (await cool / unlock lag) was invisible. Cover ~1
                // unlock window so tip-rearm can still find a fresh in.
                // dump219: CONNECTING/RECV lag let tip advance past 12 — deposit
                // at tip-16 was invisible while unlocked. Cover ~2h tip lag.
                // dump220: tip64 missed deposit once tip advanced to tip-108
                // while black-FB/UTM soft-restarts burned wall time.
                // tip768: Cake get_info height can be 1 ahead of get_block tip
                // ("Requested block height N greater than current top N-1") →
                // DONE_NO_BLOB forever on tip. Start tip walk at height-1.
                let tip_scan_h = height.saturating_sub(1);
                // Prefer deposit hint while unfunded — tip can drift past the hunt
                // window during TCG CONNECT/RECV (tip1280 missed at tip-1299).
                // After KI_SPENT on the deposit, prefer CHANGE_HINT (unspent
                // change at H3804540) before tip-walk.
                let tip_or_deposit = if self.state.balance_atomic == 0
                    && self.deposit_hint_spent
                    && tip_scan_h >= CHANGE_HINT_HEIGHT
                {
                    debug_println!(
                        "UIAPP|WALLET|SYNC|CHANGE_HINT tip={} h={}",
                        height,
                        CHANGE_HINT_HEIGHT
                    );
                    CHANGE_HINT_HEIGHT
                } else if self.state.balance_atomic == 0
                    && !self.deposit_hint_spent
                    && tip_scan_h >= DEPOSIT_HINT_HEIGHT
                {
                    debug_println!(
                        "UIAPP|WALLET|SYNC|DEPOSIT_HINT tip={} h={}",
                        height,
                        DEPOSIT_HINT_HEIGHT
                    );
                    DEPOSIT_HINT_HEIGHT
                } else {
                    tip_scan_h
                };
                let (scan_h, test_blocks_left) = if test_wallet {
                    if self.test_scan_i > TEST_SCAN_HEIGHTS.len() {
                        debug_println!(
                            "UIAPP|WALLET|SYNC|TIP_ONLY tip={} scan={}",
                            height,
                            tip_or_deposit
                        );
                        (tip_or_deposit, TIP_HUNT_BLOCKS_LEFT)
                    } else if self.test_scan_i > 0
                        && self.test_scan_i < TEST_SCAN_HEIGHTS.len()
                    {
                        let h = TEST_SCAN_HEIGHTS[self.test_scan_i]
                            .min(tip_scan_h);
                        debug_println!(
                            "UIAPP|WALLET|SYNC|TEST_RESUME i={} h={} tip={}",
                            self.test_scan_i,
                            h,
                            height
                        );
                        (h, 0u64)
                    } else if self.deposit_hint_spent {
                        // After spent deposit: do not TIP_FIRST — arm change/test
                        // heights (GetInfo used to wipe TEST_AFTER_SPENT).
                        let h = CHANGE_HINT_HEIGHT.min(tip_scan_h);
                        self.test_scan_i = 1;
                        debug_println!(
                            "UIAPP|WALLET|SYNC|CHANGE_FIRST tip={} scan={}",
                            height,
                            h
                        );
                        (h, 0u64)
                    } else {
                        // Fresh boot: tip/deposit hint first (historical TEST
                        // heights hang RECV on TCG when unspent deposit is near tip).
                        self.test_scan_i = TEST_SCAN_HEIGHTS.len().saturating_add(1);
                        debug_println!(
                            "UIAPP|WALLET|SYNC|TIP_FIRST tip={} scan={}",
                            height,
                            tip_or_deposit
                        );
                        (tip_or_deposit, TIP_HUNT_BLOCKS_LEFT)
                    }
                } else {
                    (
                        tip_or_deposit,
                        SCAN_BLOCKS.saturating_sub(1) as u64,
                    )
                };
                self.daemon_tip_height = height;
                self.publish_tip_scan(height, scan_h, &[]);
                // Keep any outs already found (deposit) across tip re-arm.
                if self.state.balance_atomic == 0 {
                    self.state.clear_scan_results();
                }
                self.scan_hashes.clear();
                self.scan_txs.clear();
                self.scan_tx_first_idx.clear();
                self.scan_batch = 0;
                self.scan_block = None;
                self.scan_first_rct = None;
                // SCAN_BLOCKS tip window: current block + (N-1) predecessors.
                // Test wallet walks TEST_SCAN_HEIGHTS instead of tip predecessors.
                self.scan_blocks_left = test_blocks_left;
                // Skip parallel worker on TCG — slot-3 TCP contended with wallet
                // slot-2 and left TIP·SCAN frozen mid-recv (dump101).
                self.arm_get_block(scan_h);
            }
            SyncRpcKind::GetBlock => {
                let tip_window = !TEST_SCAN_HEIGHTS.contains(&self.scan_height);
                let Some(blob_hex) = json_string_value(json, b"blob") else {
                    // Truncated keep-alive body often has neither blob nor hashes.
                    // Retry tip get_block; do not sync_fail→CONNECTING thrash.
                    // tip768: after retries, re-arm SAME height (not hang-skip) so
                    // H3803776 is not walked past unread.
                    if tip_window {
                        if self.sync_retry_recv("NO_BLOB") {
                            let _ = self.publish_progress(b"NEXAWAL | SCAN RETRY BLOB");
                            return;
                        }
                        let h = self.scan_height;
                        debug_println!(
                            "UIAPP|WALLET|SCAN|NO_BLOB_REARM h={} left={}",
                            h,
                            self.scan_blocks_left
                        );
                        self.sync_recv_retries = 0;
                        self.arm_get_block(h);
                        return;
                    }
                    self.sync_fail(b"NEXAWAL | SCAN NO BLOB");
                    return;
                };
                let Some(block) = decode_block_blob(blob_hex) else {
                    if tip_window {
                        if self.sync_retry_recv("BAD_BLOB") {
                            let _ = self.publish_progress(b"NEXAWAL | SCAN RETRY BLOB");
                            return;
                        }
                        if self.tip_abandon_peers_and_finish() {
                            return;
                        }
                    }
                    self.sync_fail(b"NEXAWAL | SCAN BAD BLOB");
                    return;
                };
                if !self.is_mainnet_fixture() {
                    if u64::try_from(block.number()).ok() != Some(self.scan_height) {
                        self.sync_fail(b"NEXAWAL | SCAN FAIL | WRONG BLOCK HEIGHT"); return;
                    }
                    let hash = block.hash();
                    if self.seed_restore_anchor {
                        self.seed_restore_anchor = false;
                        if !self.restore_chain.as_mut().is_some_and(|chain| chain.seed_preceding_anchor(self.scan_height, hash)) {
                            self.sync_fail(b"NEXAWAL | SCAN FAIL | BAD CHAIN ANCHOR"); return;
                        }
                        self.begin_restore_spent_checks();
                        return;
                    }
                    if self.verify_restore_anchor {
                        self.verify_restore_anchor = false;
                        if !self.restore_chain.as_ref().is_some_and(|chain| chain.anchor_matches(self.scan_height, hash)) {
                            self.reset_restore_chain();
                            self.sync_fail(b"NEXAWAL | SCAN FAIL | CHAIN CHANGED"); return;
                        }
                        if self.anchor_after_spent_checks {
                            self.anchor_after_spent_checks = false;
                            if !self.restore_chain.as_mut().is_some_and(|chain| chain.verify_tip(self.scan_height, hash, self.daemon_tip_height)) {
                                self.reset_restore_chain();
                                self.sync_fail(b"NEXAWAL | SCAN FAIL | CHAIN CHANGED"); return;
                            }
                            self.finish_restore_spent_checks();
                        } else {
                            let next = self.restore_chain.as_ref().unwrap().next();
                            if next == self.daemon_tip_height { self.begin_restore_spent_checks(); }
                            else { self.arm_history_scan(next); }
                        }
                        return;
                    }
                    if !self.restore_chain.as_ref().is_some_and(|chain| chain.accepts(self.scan_height, block.header.previous)) {
                        self.reset_restore_chain();
                        self.sync_fail(b"NEXAWAL | SCAN FAIL | CHAIN CHANGED"); return;
                    }
                }
                // Peer txs only — miner lives inside Block and is scanned from the blob.
                let mut hashes = json_string_array(json, b"tx_hashes").unwrap_or_default();
                let miner = json_string_value(json, b"miner_tx_hash")
                    .map(|h| String::from_utf8_lossy(h).into_owned());
                let raw_txs = hashes.len();
                if !self.is_mainnet_fixture() {
                    let Some(miner_hash) = miner else { self.sync_fail(b"NEXAWAL | SCAN NO MINER HASH"); return; };
                    if decode_hex32(miner_hash.as_bytes()) != Some(block.miner_transaction().hash())
                        || hashes.len() != block.transactions.len()
                        || hashes.iter().zip(&block.transactions).any(|(hex, hash)| decode_hex32(hex.as_bytes()) != Some(*hash)) {
                        self.sync_fail(b"NEXAWAL | SCAN BAD HASHES"); return;
                    }
                    self.scan_miner_hash = Some(miner_hash.clone());
                    self.scan_block = Some(block);
                    self.scan_hashes = hashes;
                    self.scan_batch = 0;
                    self.scan_txs.clear();
                    self.scan_tx_first_idx.clear();
                    self.scan_first_rct = None;
                    self.arm_miner_indices(&miner_hash);
                    return;
                }
                // Historical TEST_SCAN_HEIGHTS: miner-only (peers hung RECV; outs
                // spent). Tip-window: keep peer hashes and walk them one batch at
                // a time; hung batches skip via tip_abandon_peers_and_finish so a
                // deposit not in tx[0] can still fund prepare/send_raw.
                // Cap list length only for memory — fetch still uses TX_BATCH.
                const MAX_PEER_TXS: usize = 48;
                let mut block = block;
                // CHANGE_HINT must keep peers (not in TEST_SCAN_HEIGHTS).
                let tip_window = !TEST_SCAN_HEIGHTS.contains(&self.scan_height)
                    || self.scan_height == CHANGE_HINT_HEIGHT;
                // Host deposit tx is peer index 8 at H3803776 — fetch first so
                // EARLY_FUNDED can stop before TCG walks the other 20 hashes.
                if self.scan_height == DEPOSIT_HINT_HEIGHT && hashes.len() > 8 {
                    hashes.swap(0, 8);
                    debug_println!("UIAPP|WALLET|SCAN|DEPOSIT_TX_FIRST");
                }
                if self.scan_height == CHANGE_HINT_HEIGHT
                    && hashes.len() > CHANGE_TX_PEER_INDEX
                {
                    hashes.swap(0, CHANGE_TX_PEER_INDEX);
                    debug_println!("UIAPP|WALLET|SCAN|CHANGE_TX_FIRST");
                }
                if self.scan_height == LATER_HINT_HEIGHT
                    && hashes.len() > LATER_TX_PEER_INDEX
                {
                    hashes.swap(0, LATER_TX_PEER_INDEX);
                    debug_println!("UIAPP|WALLET|SCAN|LATER_TX_FIRST");
                }
                if self.scan_height == POST_HINT_HEIGHT
                    && hashes.len() > POST_TX_PEER_INDEX
                {
                    hashes.swap(0, POST_TX_PEER_INDEX);
                    debug_println!("UIAPP|WALLET|SCAN|POST_TX_FIRST");
                }
                if tip_window {
                    if hashes.len() > MAX_PEER_TXS {
                        debug_println!(
                            "UIAPP|WALLET|SCAN|TIP_PEER cap {} -> {}",
                            hashes.len(),
                            MAX_PEER_TXS
                        );
                        hashes.truncate(MAX_PEER_TXS);
                    }
                } else if !hashes.is_empty() {
                    debug_println!(
                        "UIAPP|WALLET|SCAN|MINER_ONLY drop_peers={}",
                        hashes.len()
                    );
                    hashes.clear();
                    block.transactions.clear();
                }
                debug_println!(
                    "UIAPP|WALLET|SCAN|BLOCK_OK h={} txs={} (raw={})",
                    self.scan_height,
                    hashes.len(),
                    raw_txs
                );
                self.scan_block = Some(block);
                self.scan_hashes = hashes;
                self.scan_batch = 0;
                self.scan_txs.clear();
                self.scan_tx_first_idx.clear();
                self.scan_first_rct = None;
                if let Some(miner_hash) = miner {
                    self.arm_miner_indices(&miner_hash);
                } else {
                    self.arm_tx_batch();
                }
            }
            SyncRpcKind::GetMinerIndices => {
                if !self.is_mainnet_fixture() {
                    let Some(hash) = self.scan_miner_hash.as_ref() else { self.sync_fail(b"NEXAWAL | SCAN NO MINER HASH"); return; };
                    let Some(parsed) = parse_tx_batch_verified(json, core::slice::from_ref(hash)) else { self.sync_fail(b"NEXAWAL | SCAN BAD MINER"); return; };
                    self.scan_first_rct = parsed.first().and_then(|(tx, indices)| {
                        if matches!(tx, Transaction::V2 { .. }) { indices.first().copied() } else { None }
                    });
                    self.arm_tx_batch();
                    return;
                }
                let lists = json_output_index_lists(json);
                self.scan_first_rct = lists.first().and_then(|l| l.first()).copied();
                debug_println!(
                    "UIAPP|WALLET|SCAN|MINER_OK first_rct={:?}",
                    self.scan_first_rct
                );
                self.arm_tx_batch();
            }
            SyncRpcKind::GetTxBatch => {
                let strict = !self.is_mainnet_fixture();
                let parsed = if strict {
                    let Some((start, end)) = strict_tx_request_range(self.scan_batch, self.scan_hashes.len(), self.scan_full_fallback.then_some(self.scan_full_fallback_offset)) else {
                        self.sync_fail(b"NEXAWAL | SCAN BAD TX RANGE"); return;
                    };
                    parse_tx_batch_verified(json, &self.scan_hashes[start..end])
                } else { parse_tx_batch(json) };
                let Some(parsed) = parsed else {
                    if strict && self.fallback_to_single_full_transactions() {
                        return;
                    }
                    self.sync_fail(b"NEXAWAL | SCAN BAD TXS");
                    return;
                };
                for (tx, indices) in parsed {
                    if self.scan_first_rct.is_none() && (!strict || matches!(tx, Transaction::V2 { .. })) {
                        self.scan_first_rct = indices.first().copied();
                    }
                    // Per-tx global index 0 — required when DEPOSIT_TX_FIRST
                    // reorders so miner first_rct is not this tx's base.
                    self.scan_tx_first_idx
                        .push(indices.first().copied().unwrap_or(0));
                    self.scan_txs.push(tx);
                }
                debug_println!(
                    "UIAPP|WALLET|SCAN|TX_OK batch={} have={}",
                    self.scan_batch,
                    self.scan_txs.len()
                );
                self.sync_recv_retries = 0;
                if strict && self.scan_full_fallback {
                    let next = self.scan_full_fallback_offset.saturating_add(1);
                    if strict_tx_request_range(self.scan_batch, self.scan_hashes.len(), Some(next)).is_some() {
                        self.scan_full_fallback_offset = next;
                        self.arm_tx_batch();
                        return;
                    }
                }
                self.scan_full_fallback = false;
                self.scan_full_fallback_offset = 0;
                self.scan_batch = self.scan_batch.saturating_add(1);
                // Deposit H3803776: owned tx is peer index 8 — don't wait for all
                // 21 peer fetches (TCG TIP_SKIP storms) before scanning. Try
                // assemble on the txs we already have; FUNDED_STOP if outs land.
                // Also early-try CHANGE_HINT (H3804540 unspent change) — same
                // TCG peer-fetch storm risk as the original deposit block.
                if !strict && (self.scan_height == DEPOSIT_HINT_HEIGHT && !self.deposit_hint_spent
                    || self.scan_height == CHANGE_HINT_HEIGHT
                    || self.scan_height == LATER_HINT_HEIGHT
                    || self.scan_height == POST_HINT_HEIGHT)
                    && self.state.balance_atomic == 0
                    && !self.scan_txs.is_empty()
                    && self.scan_block.is_some()
                {
                    if let Some(view) = self.state.view_pair().cloned() {
                        let block = self.scan_block.as_ref().unwrap().clone();
                        let txs = self.scan_txs.clone();
                        // DEPOSIT_TX_FIRST: Scanner always walks miner tx first,
                        // then peer txs[0]. So first_rct must be
                        // peer_out0 - miner_vout_count (not peer_out0 alone —
                        // that off-by-one made key≠chain → SEND invalid_input).
                        let peer_first = self
                            .scan_tx_first_idx
                            .first()
                            .copied()
                            .filter(|&x| x > 0);
                        let miner_outs = self
                            .scan_block
                            .as_ref()
                            .map(|b| b.miner_transaction().prefix().outputs.len() as u64)
                            .unwrap_or(0);
                        let first = peer_first
                            .map(|p| p.saturating_sub(miner_outs))
                            .or(self.scan_first_rct);
                        debug_println!(
                            "UIAPP|WALLET|SCAN|EARLY_FIRST first={:?} peer={:?} miner_outs={} miner_first={:?}",
                            first,
                            peer_first,
                            miner_outs,
                            self.scan_first_rct
                        );
                        if let Ok(outputs) = scan_assembled(&view, block, txs, first) {
                            if !outputs.is_empty() {
                                let n = outputs.len();
                                self.state.apply_scan_outputs(&outputs);
                                debug_println!(
                                    "UIAPP|WALLET|SCAN|EARLY_FUNDED h={} outs={} bal={} have={}",
                                    self.scan_height,
                                    n,
                                    self.state.balance_atomic,
                                    self.scan_txs.len()
                                );
                                self.tip_locked = false;
                                self.scan_blocks_left = 0;
                                self.state.scanned = true;
                                // Short BAL line first so shell face parse cannot
                                // miss it inside a failed full ADDR status / TIP fold.
                                {
                                    let mut line = [0u8; 48];
                                    let p = b"NEXAWAL | OPEN | BAL ";
                                    line[..p.len()].copy_from_slice(p);
                                    let mut n = p.len();
                                    let mut v = self.state.balance_atomic;
                                    let mut dig = [0u8; 20];
                                    let mut i = 0usize;
                                    if v == 0 {
                                        dig[0] = b'0';
                                        i = 1;
                                    } else {
                                        while v > 0 && i < dig.len() {
                                            dig[i] = b'0' + (v % 10) as u8;
                                            v /= 10;
                                            i += 1;
                                        }
                                    }
                                    while i > 0 && n < line.len() {
                                        i -= 1;
                                        line[n] = dig[i];
                                        n += 1;
                                    }
                                    let _ = self.publish_bytes(&line[..n]);
                                }
                                let _ = self.publish_status();
                                self.scan_block = None;
                                self.scan_hashes.clear();
                                self.scan_txs.clear();
                                self.scan_tx_first_idx.clear();
                                // Deposit outs already applied — skip parallel
                                // scan_worker (256KiB bump heap OOMs on 512KiB
                                // get_block). Check KI *before* 15× get_outs so
                                // a spent deposit clears the face under TCG load
                                // without stalling on decoy fetch.
                                self.arm_key_image_spent_check();
                                return;
                            }
                        }
                    }
                }
                self.arm_tx_batch();
            }
            SyncRpcKind::GetDistribution => {
                let valid = self.distribution_download.as_mut().is_some_and(|download| {
                    download.next_range().is_some_and(|(from, to)| download.accept(from, to, json).is_some())
                });
                if !valid { self.sync_fail(b"NEXAWAL | DISTRIBUTION INVALID"); return; }
                if self.distribution_download.as_ref().and_then(DistributionDownload::next_range).is_some() {
                    self.arm_distribution_chunk();
                } else {
                    self.distribution = self.distribution_download.take().and_then(DistributionDownload::finish);
                    if self.distribution.is_none() { self.sync_fail(b"NEXAWAL | DISTRIBUTION EMPTY"); return; }
                    self.arm_distribution_anchor();
                }
            }
            SyncRpcKind::GetDistributionAnchor => {
                let valid = self.restore_chain.as_ref().is_some_and(|chain| {
                    chain.is_verified() && chain.next() == self.daemon_tip_height && chain.anchor().is_some_and(|(height, hash)| {
                        json_string_value(json, b"blob").and_then(decode_block_blob)
                            .is_some_and(|block| u64::try_from(block.number()).ok() == Some(height) && block.hash() == hash)
                    })
                });
                if !valid { self.sync_fail(b"NEXAWAL | DISTRIBUTION ANCHOR CHANGED"); return; }
                self.start_restored_ring();
            }
            SyncRpcKind::GetRingBatch => {
                if !self.ring_session.as_mut().is_some_and(|session| session.accept(json).is_some()) {
                    self.sync_fail(b"NEXAWAL | RING RESPONSE INVALID"); return;
                }
                if self.ring_session.as_ref().is_some_and(DecoyRingSession::is_complete) {
                    let ring = self.ring_session.take().and_then(DecoyRingSession::finish);
                    let Some(ring) = ring else { self.sync_fail(b"NEXAWAL | RING RESPONSE INVALID"); return; };
                    self.restored_rings.push((self.ring_input_index, ring));
                    self.ring_target_cursor += 1;
                    if self.ring_target_cursor < self.ring_targets.len() { self.start_restored_ring(); }
                    else { self.check_restored_ring_fees(); }
                } else {
                    let valid = match (self.ring_session.as_mut(), self.distribution.as_ref()) {
                        (Some(session), Some(distribution)) => session.request_replacements(distribution).is_some(),
                        _ => false,
                    };
                    if !valid { self.sync_fail(b"NEXAWAL | RING SELECTION FAILED"); return; }
                    self.arm_restored_ring_batch();
                }
            }
            SyncRpcKind::ValidateInfo => {
                if !self.payment_validation.as_mut().is_some_and(|validation| validation.observe_info_response(json, self.state.network)) {
                    self.sync_fail(b"NEXAWAL | VALIDATION CHAIN CHANGED"); return;
                }
                self.arm_validation_anchor(false);
            }
            SyncRpcKind::ValidateAnchorBefore | SyncRpcKind::ValidateAnchorAfter => {
                let after = self.sync_rpc == SyncRpcKind::ValidateAnchorAfter;
                if !self.payment_validation.as_mut().is_some_and(|validation| validation.observe_anchor_response(json)) {
                    self.sync_fail(b"NEXAWAL | VALIDATION CHAIN CHANGED"); return;
                }
                if after {
                    let mut body = [0u8; 128];
                    let Some(n) = build_fee_estimate_body(&mut body) else { self.sync_fail(b"NEXAWAL | VALIDATION FEE FAILED"); return; };
                    self.arm_validation_rpc(SyncRpcKind::ValidateFee, &body[..n], "/json_rpc");
                } else { self.arm_validation_spent(); }
            }
            SyncRpcKind::ValidateSpent => {
                let Some(statuses) = parse_spent_statuses(json, self.validation_batch_count) else { self.sync_fail(b"NEXAWAL | VALIDATION SPENT FAILED"); return; };
                self.validation_statuses.extend_from_slice(&statuses);
                let Some(validation) = self.payment_validation.as_mut() else { self.sync_fail(b"NEXAWAL | VALIDATION STATE"); return; };
                if self.validation_statuses.len() == validation.key_images().len() {
                    if !validation.observe_spent(&self.validation_statuses) { self.sync_fail(b"NEXAWAL | VALIDATION INPUT SPENT"); return; }
                    self.arm_validation_anchor(true);
                } else { self.arm_validation_spent(); }
            }
            SyncRpcKind::ValidateFee => {
                let Some(proof) = self.payment_validation.take().and_then(|validation| validation.finish_fee_response(json)) else {
                    self.sync_fail(b"NEXAWAL | VALIDATION FEE FAILED"); return;
                };
                let sign_after = core::mem::take(&mut self.validation_sign_after);
                if sign_after {
                    let mut entropy = Zeroizing::new([0u8; 32]);
                    for chunk in entropy.chunks_mut(entropy_protocol::SAMPLE_BYTES) {
                        let reply = Channel::new(4).pp_call(MessageInfo::new(entropy_protocol::REQUEST_BYTES, 0));
                        if reply.label() != entropy_protocol::REPLY_OK || reply.count() != 2 {
                            self.sync_fail(b"NEXAWAL | ENTROPY UNAVAILABLE"); return;
                        }
                        chunk[..8].copy_from_slice(&get_mr(0).to_le_bytes());
                        chunk[8..].copy_from_slice(&get_mr(1).to_le_bytes());
                    }
                    let signed = match (self.payment_preview.as_ref(), self.restore_chain.as_ref()) {
                        (Some(preview), Some(chain)) => sign_revalidated_preview(&mut self.state, &self.restored_outputs,
                            chain, &self.restored_rings, preview, proof, entropy),
                        _ => Err(PrepareError::NeedScan),
                    };
                    if signed.is_err() { self.sync_fail(b"NEXAWAL | PREVIEW CHANGED"); return; }
                    if !self.persist_signed_transaction() {
                        self.pending_durable = false;
                        self.sync_phase = SyncPhase::Idle;
                        let _ = self.publish_bytes(b"NEXAWAL | SAVE FAILED"); return;
                    }
                    self.pending_durable = true;
                    self.payment_preview = None;
                    self.sync_phase = SyncPhase::Idle;
                    // The user's Send action authorized relay; it is armed
                    // only after the signed bytes were durably committed.
                    let _ = self.send_tx_stub();
                    return;
                }
                self.sync_phase = SyncPhase::Idle;
                let _ = self.publish_bytes(b"NEXAWAL | VALIDATION COMPLETE | SEND NOT READY");
            }
            SyncRpcKind::GetFeeEstimate => {
                self.fee_rate = parse_normal_fee_rate(json);
                debug_println!("UIAPP|WALLET|FEE|VALID {}", u32::from(self.fee_rate.is_some()));
                self.sync_phase = SyncPhase::Idle;
                if self.fee_rate.is_none() {
                    let _ = self.publish_status_tag(b" | FEE UNAVAILABLE | SCAN OK | SEND NOT READY");
                }
            }
            SyncRpcKind::GetHistogram => {
                let Some(total) = parse_histogram_total(json) else {
                    debug_println!("UIAPP|WALLET|DECOY|HIST_FAIL");
                    let _ = self.publish_status();
                    self.sync_phase = SyncPhase::Idle;
                    return;
                };
                self.decoy_rct_total = total;
                debug_println!("UIAPP|WALLET|DECOY|HIST total={}", total);
                self.decoy_ring.clear();
                self.decoy_queue.clear();
                // Prefer largest out — change txs often include a 1-piconero
                // sibling first; spending dust → PREPARE SIGN FAIL (no fee).
                let real = walletcore_core::primary_spend_output(&self.state)
                    .map(|o| o.index_on_blockchain())
                    .unwrap_or(0);
                // r34: skip ±16 of real (same-tx sibling / neighbors →
                // double_spend+invalid_input). r58: tip-only decoys still got
                // invalid_input after PREPARE OK — sample primarily from the
                // age of the real out, then fall back toward tip.
                let need = usize::from(RING_LEN.saturating_sub(1));
                let mut cursor = real.saturating_sub(64);
                while self.decoy_queue.len() < need && cursor > 0 {
                    if cursor.abs_diff(real) >= 16 && !self.decoy_queue.contains(&cursor) {
                        self.decoy_queue.push(cursor);
                    }
                    cursor = cursor.saturating_sub(17);
                }
                let mut tip = total.saturating_sub(16);
                while self.decoy_queue.len() < need && tip > 0 {
                    let near_real = tip.abs_diff(real) < 16;
                    if tip != real && !near_real && !self.decoy_queue.contains(&tip) {
                        self.decoy_queue.push(tip);
                    }
                    tip = tip.saturating_sub(17);
                }
                debug_println!(
                    "UIAPP|WALLET|DECOY|QUEUE n={} real={}",
                    self.decoy_queue.len(),
                    real
                );
                if let Some(idx) = self.decoy_queue.pop() {
                    self.arm_decoy_get_outs(idx);
                } else {
                    let _ = self.publish_status();
                    self.sync_phase = SyncPhase::Idle;
                }
            }
            SyncRpcKind::GetOuts => {
                if let Some((key, mask)) = parse_get_outs_key_mask(json) {
                    if let Some(out) = walletcore_core::primary_spend_output(&self.state) {
                        if out.index_on_blockchain() == self.decoy_out_index {
                            let local = out.commitment().commit().compress().to_bytes();
                            let match_c = local == mask;
                            let match_k = out.key().compress().to_bytes() == key;
                            debug_println!(
                                "UIAPP|WALLET|DECOY|REAL_CMP key={} commit={}",
                                match_k as u32,
                                match_c as u32
                            );
                        }
                    }
                    self.decoy_ring
                        .push((self.decoy_out_index, key, mask));
                    debug_println!(
                        "UIAPP|WALLET|DECOY|OUT_OK idx={} have={}",
                        self.decoy_out_index,
                        self.decoy_ring.len()
                    );
                } else {
                    debug_println!(
                        "UIAPP|WALLET|DECOY|OUT_FAIL idx={}",
                        self.decoy_out_index
                    );
                }
                if let Some(idx) = self.decoy_queue.pop() {
                    self.arm_decoy_get_outs(idx);
                } else if let Some(out) = walletcore_core::primary_spend_output(&self.state) {
                    let real = out.index_on_blockchain();
                    let amt = out.commitment().amount;
                    // Fetch real from Cake so ring commitment matches chain
                    // (local commit().compress() caused invalid_input).
                    if !self.decoy_ring.iter().any(|(i, _, _)| *i == real) {
                        debug_println!(
                            "UIAPP|WALLET|DECOY|REAL_FETCH idx={} amt={}",
                            real,
                            amt
                        );
                        self.arm_decoy_get_outs(real);
                    } else {
                        let key = self
                            .decoy_ring
                            .iter()
                            .find(|(i, _, _)| *i == real)
                            .map(|(_, k, _)| *k)
                            .unwrap_or([0; 32]);
                        debug_println!(
                            "UIAPP|WALLET|DECOY|REAL idx={} amt={} key[:4]={:02x}{:02x}{:02x}{:02x}",
                            real,
                            amt,
                            key[0],
                            key[1],
                            key[2],
                            key[3]
                        );
                        debug_println!(
                            "UIAPP|WALLET|DECOY|RING_READY n={}",
                            self.decoy_ring.len()
                        );
                        self.sync_phase = SyncPhase::Idle;
                        if self.state.balance_atomic > 0
                            && !self.state.outputs.is_empty()
                            && self.decoy_ring.len() >= usize::from(RING_LEN)
                        {
                            let _ = self.publish_bytes(b"NEXAWAL | SCAN OK");
                            self.arm_key_image_spent_check();
                        } else {
                            let _ = self.publish_status();
                        }
                    }
                } else {
                    let _ = self.publish_status();
                    self.sync_phase = SyncPhase::Idle;
                }
            }
            SyncRpcKind::RestoreSpent => {
                if self.restore_check_count == 0 || self.restore_check_count > 16 { self.sync_fail(b"NEXAWAL | SCAN FAIL | OUTPUT STATE"); return; }
                let Some(statuses) = parse_spent_statuses(json, self.restore_check_count) else {
                    self.sync_fail(b"NEXAWAL | SCAN FAIL | BAD SPENT STATUS"); return;
                };
                for status in statuses {
                    let Some(output) = self.restored_outputs.get_mut(self.restore_check_index) else { self.sync_fail(b"NEXAWAL | SCAN FAIL | OUTPUT STATE"); return; };
                    output.spent_status = Some(status);
                    self.restore_check_index += 1;
                }
                self.restore_check_count = 0;
                self.arm_restore_spent_check();
            }
            SyncRpcKind::IsKeyImageSpent => {
                let Some(statuses) = parse_spent_statuses(json, 1) else {
                    self.sync_fail(b"NEXAWAL | SCAN FAIL | BAD SPENT STATUS");
                    return;
                };
                // Both chain-spent and pool-spent outputs are unavailable.
                let spent = statuses[0] != 0;
                debug_println!("UIAPP|WALLET|KI_SPENT {}", spent as u32);
                if spent {
                    let on_change = self.scan_height == CHANGE_HINT_HEIGHT;
                    let on_later = self.scan_height == LATER_HINT_HEIGHT;
                    let on_post = self.scan_height == POST_HINT_HEIGHT;
                    if let Some(out) = walletcore_core::primary_spend_output(&self.state) {
                        self.state.note_activity(false, out.commitment().amount);
                    }
                    self.state.clear_scan_results();
                    self.decoy_ring.clear();
                    self.deposit_hint_spent = true;
                    debug_println!("UIAPP|WALLET|NEED DEPOSIT");
                    // Explicit BAL 0 so shell clears the stale funded face.
                    let _ = self.publish_bytes(b"NEXAWAL | NEED DEPOSIT | BAL 0");
                    if on_post {
                        // Newest change is spent too. Stop; do not refetch.
                        self.later_hint_armed = false;
                        self.change_hint_spent = true;
                        self.state.mark_scanned_empty();
                        self.sync_phase = SyncPhase::Idle;
                        let _ = self.publish_status();
                        debug_println!("UIAPP|WALLET|SCAN|POST_SPENT_STOP");
                    } else if on_later {
                        // H3805635 was spent by the confirmed 0.001 send.
                        // The change and the 0.001 receive are at H3805838.
                        self.later_hint_armed = true;
                        self.test_scan_i = 1;
                        self.scan_blocks_left = 0;
                        self.scan_height = POST_HINT_HEIGHT;
                        debug_println!(
                            "UIAPP|WALLET|SCAN|POST_AFTER_SPENT h={}",
                            POST_HINT_HEIGHT
                        );
                        self.arm_get_block(POST_HINT_HEIGHT);
                    } else if on_change {
                        // H3804540's large out was spent at H3805635. That newer
                        // change is still unspent (host 12337358818). One fetch.
                        self.later_hint_armed = true;
                        self.test_scan_i = 1;
                        self.scan_blocks_left = 0;
                        self.scan_height = LATER_HINT_HEIGHT;
                        debug_println!(
                            "UIAPP|WALLET|SCAN|LATER_AFTER_SPENT h={}",
                            LATER_HINT_HEIGHT
                        );
                        self.arm_get_block(LATER_HINT_HEIGHT);
                    } else {
                        self.later_hint_armed = false;
                        self.test_scan_i = 1;
                        self.scan_blocks_left = 0;
                        self.scan_height = CHANGE_HINT_HEIGHT;
                        debug_println!(
                            "UIAPP|WALLET|SCAN|CHANGE_AFTER_SPENT h={}",
                            CHANGE_HINT_HEIGHT
                        );
                        self.arm_get_block(CHANGE_HINT_HEIGHT);
                    }
                } else if self.decoy_ring.len() >= usize::from(RING_LEN) {
                    let _ = self.publish_status();
                    self.sync_phase = SyncPhase::Idle;
                } else {
                    // Pre-decoy KI check passed — gather ring then prepare.
                    self.later_hint_armed = false;
                    self.arm_decoy_histogram();
                }
            }
            SyncRpcKind::Reconcile => {
                let expected = PendingTransaction {
                    wallet_address: String::from(self.state.address()),
                    tx_hex: Zeroizing::new(self.state.pending_tx_hex.clone()),
                    fee: self.state.pending_fee_atomic,
                    amount: self.state.pending_send_amount,
                };
                match observe_pending_transaction(&expected, json) {
                    Some(PendingObservation::Missing) => {
                        let _ = self.publish_bytes(b"NEXAWAL | TRANSACTION NOT FOUND");
                    }
                    Some(PendingObservation::InPool) => {
                        let _ = self.publish_bytes(b"NEXAWAL | TRANSACTION IN POOL");
                    }
                    Some(PendingObservation::InBlock(height)) => {
                        let status = alloc::format!("NEXAWAL | TRANSACTION IN BLOCK {height}");
                        let _ = self.publish_bytes(status.as_bytes());
                    }
                    None => {
                        let _ = self.publish_bytes(b"NEXAWAL | TRANSACTION CHECK FAILED");
                    }
                }
                // Retain the durable record until verified chain inclusion and
                // a durable history update can be committed together.
                self.sync_phase = SyncPhase::Idle;
            }
            SyncRpcKind::SendRaw => {
                let ok = parse_send_raw_ok(json);
                debug_println!("UIAPP|WALLET|SEND|RAW ok={}", ok);
                // Cake rejection reasons (double-spend / low pref / parse) live
                // in the JSON body — print a short ASCII prefix for host OCR.
                {
                    let n = json.len().min(400);
                    let mut buf = [0u8; 400];
                    let mut w = 0usize;
                    for &b in &json[..n] {
                        buf[w] = if b.is_ascii_graphic() || b == b' ' { b } else { b'.' };
                        w += 1;
                    }
                    if let Ok(s) = core::str::from_utf8(&buf[..w]) {
                        debug_println!("UIAPP|WALLET|SEND|RAW body={}", s);
                    }
                }
                if ok {
                    if !self.publish_status_tag(b" | SEND OK") {
                        let _ = self.publish_bytes(b"NEXAWAL | SEND OK");
                    }
                    // Keep the durable transaction until its chain status is reconciled.
                } else if !self.publish_status_tag(b" | SEND UNCONFIRMED") {
                    let _ = self.publish_bytes(b"NEXAWAL | SEND UNCONFIRMED");
                }
                self.sync_phase = SyncPhase::Idle;
            }
        }
    }

    fn sync_advance(&mut self) {
        match self.sync_phase {
            SyncPhase::Idle => {}
            SyncPhase::Start => self.sync_step_start(),
            SyncPhase::WaitConnect => self.sync_step_wait_connect(),
            SyncPhase::Send => self.sync_step_send(),
            SyncPhase::Recv => self.sync_step_recv(),
            SyncPhase::WaitWorker => self.sync_step_wait_worker(),
            #[cfg(feature = "wallet-sync-timer")]
            SyncPhase::WaitTimer => {},
            #[cfg(feature = "wallet-sync-timer")]
            SyncPhase::Range => return,
        }
        // Keep the worker TCP state machine moving alongside wallet RPC.
        if self.sync_phase != SyncPhase::Idle && self.sync_phase != SyncPhase::WaitWorker {
            self.maybe_nudge_worker();
        }
    }

    /// Ensure signing prerequisites exist without discarding the wallet's
    /// other outputs or transaction history to reclaim allocator memory.
    fn ready_for_sign(&self) -> bool {
        walletcore_core::primary_spend_output(&self.state).is_some()
            && self.state.spend_secret().is_some()
            && self.state.view_pair().is_some()
            && self.decoy_ring.len() >= usize::from(RING_LEN)
    }

    /// Prepare path: transfer carries `DEST <addr>` plus `AMOUNT <atomic>` or `MAX`.
    /// Does not broadcast. Send is a separate confirm.
    fn prepare_send_stub(&mut self) -> MessageInfo {
        self.clear_payment_validation();
        self.payment_preview = None;
        if !self.state.pending_tx_hex.is_empty() {
            if !self.pending_durable {
                self.pending_durable = self.persist_signed_transaction();
            }
            let status: &[u8] = if self.pending_durable {
                b"NEXAWAL | PENDING SEND"
            } else {
                b"NEXAWAL | SAVE FAILED"
            };
            let _ = self.publish_bytes(status);
            return MessageInfo::new(REPLY_OK, 0);
        }
        if !self.open_wallet() {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let mut dest_buf = [0u8; 128];
        let mut dest_len = 0usize;
        let mut amount = 0u64;
        let mut send_max = false;
        if let Some(payload) = unsafe { self.transfer.as_ref().payload() } {
            if let Ok(text) = core::str::from_utf8(payload) {
                let mut parts = text.split_whitespace();
                while let Some(tok) = parts.next() {
                    if tok == "DEST" {
                        if let Some(addr) = parts.next() {
                            if addr.len() < dest_buf.len() {
                                dest_buf[..addr.len()].copy_from_slice(addr.as_bytes());
                                dest_len = addr.len();
                            }
                        }
                    } else if tok == "AMOUNT" {
                        if let Some(a) = parts.next() {
                            if let Ok(v) = a.parse::<u64>() {
                                amount = v;
                            }
                        }
                    } else if tok == "MAX" {
                        send_max = true;
                    }
                }
            }
        }
        if dest_len == 0 {
            let _ = self.publish_bytes(b"NEXAWAL | PREPARE BAD ADDR");
            return MessageInfo::new(REPLY_OK, 0);
        }
        if self.decoy_ring.len() >= usize::from(RING_LEN) && !self.ready_for_sign() {
            let _ = self.publish_bytes(b"NEXAWAL | PREPARE NEED SCAN");
            return MessageInfo::new(REPLY_OK, 0);
        }
        let dest = core::str::from_utf8(&dest_buf[..dest_len]).unwrap_or("");
        if !self.is_mainnet_fixture() {
            if walletcore_core::MoneroAddress::from_str(self.state.network, dest).is_err() {
                let _ = self.publish_bytes(b"NEXAWAL | PREPARE BAD ADDR"); return MessageInfo::new(REPLY_OK, 0);
            }
            if !send_max && amount == 0 {
                let _ = self.publish_bytes(b"NEXAWAL | PREPARE INSUFFICIENT"); return MessageInfo::new(REPLY_OK, 0);
            }
            if self.sync_phase == SyncPhase::Idle && self.node_verified && self.fee_rate.is_some()
                && self.state.unlocked_atomic > 0 && self.restore_chain.as_ref().is_some_and(RestoreChain::is_verified) {
                let required = if send_max { self.state.unlocked_atomic } else { amount };
                let Some(targets) = select_restored_inputs(&self.restored_outputs, self.daemon_tip_height, required) else {
                    let _ = self.publish_bytes(b"NEXAWAL | PREPARE INSUFFICIENT"); return MessageInfo::new(REPLY_OK, 0);
                };
                self.ring_dest = String::from(dest);
                self.ring_amount = amount;
                self.ring_max = send_max;
                if !self.ring_targets.starts_with(&targets) || self.restored_rings.len() != self.ring_targets.len() {
                    self.ring_targets = targets;
                    self.restored_rings.clear();
                    self.ring_target_cursor = 0;
                    if self.distribution.is_some() { self.ring_request_pending = true; }
                    else { self.distribution_request_pending = true; }
                    let _ = self.publish_bytes(b"NEXAWAL | PREPARING DECOYS");
                    WALLET_SHELL_CHANNEL.notify();
                    return MessageInfo::new(REPLY_OK, 0);
                }
                let result = self.restore_chain.as_ref().and_then(|chain| self.fee_rate.map(|fee| (chain, fee)))
                    .map(|(chain, fee)| create_payment_preview(
                        &mut self.state, &self.restored_outputs, chain, self.daemon_tip_height,
                        &self.restored_rings, dest, amount, send_max, fee));
                match result {
                    Some(Ok(preview)) => {
                        let readiness = if self.state.network == Network::Mainnet { "SEND NOT READY" } else { "TEST SEND READY" };
                        let status = alloc::format!("NEXAWAL | PREVIEW FEE {} AMOUNT {} | {}", preview.fee(), preview.amount(), readiness);
                        self.payment_preview = Some(preview);
                        let _ = self.publish_bytes(status.as_bytes());
                    }
                    Some(Err(PrepareError::Insufficient)) => { let _ = self.publish_bytes(b"NEXAWAL | PREPARE INSUFFICIENT"); }
                    _ => { let _ = self.publish_bytes(b"NEXAWAL | PREPARE NEED SCAN"); }
                }
                return MessageInfo::new(REPLY_OK, 0);
            }
        }
        let mut fee_buf = [0u8; 48];
        let msg: &[u8] = if self.decoy_ring.len() >= usize::from(RING_LEN) {
            if !self.journal.as_ref().is_some_and(WalletJournal::can_record_new_transaction) {
                let _ = self.publish_bytes(b"NEXAWAL | PAYMENT HISTORY FULL");
                return MessageInfo::new(REPLY_OK, 0);
            }
            let mut entropy = Zeroizing::new([0u8; 32]);
            let mut available = true;
            for chunk in entropy.chunks_mut(entropy_protocol::SAMPLE_BYTES) {
                let reply = Channel::new(4).pp_call(MessageInfo::new(entropy_protocol::REQUEST_BYTES, 0));
                if reply.label() != entropy_protocol::REPLY_OK || reply.count() != 2 {
                    available = false;
                    break;
                }
                chunk[..8].copy_from_slice(&get_mr(0).to_le_bytes());
                chunk[8..].copy_from_slice(&get_mr(1).to_le_bytes());
            }
            if !available {
                let _ = self.publish_bytes(b"NEXAWAL | ENTROPY UNAVAILABLE");
                return MessageInfo::new(REPLY_OK, 0);
            }
            match prepare_and_sign(&mut self.state, dest, amount, &self.decoy_ring, send_max, entropy) {
                Ok(()) => {
                    if !self.persist_signed_transaction() {
                        self.pending_durable = false;
                        let _ = self.publish_bytes(b"NEXAWAL | SAVE FAILED");
                        return MessageInfo::new(REPLY_OK, 0);
                    }
                    self.pending_durable = true;
                    let n = Self::format_prepare_ok(&mut fee_buf, self.state.pending_fee_atomic);
                    if n == 0 {
                        &b"NEXAWAL | PREPARE OK"[..]
                    } else {
                        &fee_buf[..n]
                    }
                }
                Err(PrepareError::Closed) => b"NEXAWAL | PREPARE CLOSED",
                Err(PrepareError::BadAddress) => b"NEXAWAL | PREPARE BAD ADDR",
                Err(PrepareError::Pending) => b"NEXAWAL | PENDING SEND",
                Err(PrepareError::Insufficient) => b"NEXAWAL | PREPARE INSUFFICIENT",
                Err(PrepareError::NeedScan) => b"NEXAWAL | PREPARE NEED SCAN",
                Err(PrepareError::NeedDecoys) => b"NEXAWAL | PREPARE NEED DECOYS",
                Err(PrepareError::SignFail) => b"NEXAWAL | PREPARE SIGN FAIL",
            }
        } else {
            let probe = if send_max { 1 } else { amount };
            match prepare_send(&self.state, dest, probe) {
                Ok(()) => b"NEXAWAL | PREPARE OK",
                Err(PrepareError::Closed) => b"NEXAWAL | PREPARE CLOSED",
                Err(PrepareError::BadAddress) => b"NEXAWAL | PREPARE BAD ADDR",
                Err(PrepareError::Pending) => b"NEXAWAL | PENDING SEND",
                Err(PrepareError::Insufficient) => b"NEXAWAL | PREPARE INSUFFICIENT",
                Err(PrepareError::NeedScan) => b"NEXAWAL | PREPARE NEED SCAN",
                Err(PrepareError::NeedDecoys) => b"NEXAWAL | PREPARE NEED DECOYS",
                Err(PrepareError::SignFail) => b"NEXAWAL | PREPARE SIGN FAIL",
            }
        };
        debug_println!(
            "UIAPP|WALLET|PREPARE|{}",
            core::str::from_utf8(msg).unwrap_or("?")
        );
        if !self.publish_bytes(msg) {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        MessageInfo::new(REPLY_OK, 0)
    }

    /// Broadcast path: pending hex arms send_raw HTTP and starts TCP.
    fn reconcile_pending(&mut self) -> MessageInfo {
        if !self.node_verified {
            let _ = self.publish_bytes(b"NEXAWAL | VERIFY NODE FIRST");
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        if self.sync_phase != SyncPhase::Idle || !self.pending_durable || !self.state.open {
            return MessageInfo::new(wallet_protocol::REPLY_DENIED, 0);
        }
        if let (Some(evidence), Some(chain)) = (self.pending_evidence.as_ref(), self.restore_chain.as_ref()) {
            if let Some(height) = evidence.location(chain, self.daemon_tip_height) {
                let status = alloc::format!("NEXAWAL | TRANSACTION SCANNED BLOCK {height}");
                let _ = self.publish_bytes(status.as_bytes());
                return MessageInfo::new(REPLY_OK, 0);
            }
        }
        let record = PendingTransaction {
            wallet_address: String::from(self.state.address()),
            tx_hex: Zeroizing::new(self.state.pending_tx_hex.clone()),
            fee: self.state.pending_fee_atomic,
            amount: self.state.pending_send_amount,
        };
        let Some(hash) = record.signed_payment_id() else {
            let _ = self.publish_bytes(b"NEXAWAL | INVALID PENDING TRANSACTION");
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        };
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut hash_hex = [0u8; 64];
        for (index, byte) in hash.into_iter().enumerate() {
            hash_hex[index * 2] = HEX[(byte >> 4) as usize];
            hash_hex[index * 2 + 1] = HEX[(byte & 15) as usize];
        }
        let hash_text = core::str::from_utf8(&hash_hex).unwrap();
        let mut body = [0u8; 512];
        let Some(length) = build_get_transactions_body(&[hash_text], &mut body) else {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        };
        let Some(length) = build_http_post(self.node.authority(), "/get_transactions", &body[..length], &mut self.sync_req) else {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        };
        self.sync_req_len = length;
        self.sync_rpc = SyncRpcKind::Reconcile;
        let _ = self.publish_bytes(b"NEXAWAL | CHECKING TRANSACTION");
        self.begin_rpc_connect();
        MessageInfo::new(REPLY_OK, 0)
    }

    fn send_tx_stub(&mut self) -> MessageInfo {
        if !self.node_verified {
            let _ = self.publish_bytes(b"NEXAWAL | VERIFY NODE FIRST");
            return MessageInfo::new(REPLY_OK, 0);
        }
        if !self.pending_durable {
            let _ = self.publish_bytes(b"NEXAWAL | SEND NOT SAVED");
            return MessageInfo::new(REPLY_OK, 0);
        }
        let record = PendingTransaction {
            wallet_address: String::from(self.state.address()),
            tx_hex: Zeroizing::new(self.state.pending_tx_hex.clone()),
            fee: self.state.pending_fee_atomic,
            amount: self.state.pending_send_amount,
        };
        if !record.belongs_to(&self.state) || record.signed_payment_id().is_none() {
            let _ = self.publish_bytes(b"NEXAWAL | SEND INVALID PENDING");
            return MessageInfo::new(REPLY_OK, 0);
        }
        // Body scratch lives in BSS — wallet PD stack is only 128KiB.
        static mut SEND_BODY: [u8; 12288] = [0; 12288];
        let msg: &[u8] = match send_prepared(&self.state) {
            Ok(tx_hex) => {
                let mut armed = false;
                if self.sync_phase != SyncPhase::Idle {
                    b"NEXAWAL | SEND BUSY"
                } else {
                    let body = unsafe { &mut *core::ptr::addr_of_mut!(SEND_BODY) };
                    if let Some(body_len) = build_send_raw_body(tx_hex, body) {
                        if let Some(http_len) = build_http_post(
                            self.node.authority(),
                            "/send_raw_transaction",
                            &body[..body_len],
                            &mut self.sync_req,
                        ) {
                            self.sync_req_len = http_len;
                            self.sync_rpc = SyncRpcKind::SendRaw;
                            armed = true;
                            debug_println!("UIAPP|WALLET|SEND|ARMED len={}", http_len);
                            let _ = self.publish_bytes(b"NEXAWAL | SEND QUEUED");
                            self.begin_rpc_connect();
                        }
                    }
                    if armed {
                        b"NEXAWAL | SEND QUEUED"
                    } else {
                        b"NEXAWAL | SEND BAD REQ"
                    }
                }
            }
            Err(SendError::Closed) => b"NEXAWAL | SEND CLOSED",
            Err(SendError::NeedScan) => b"NEXAWAL | SEND NEED SCAN",
            Err(SendError::Insufficient) => b"NEXAWAL | SEND INSUFFICIENT",
            Err(SendError::NeedPrepare) => b"NEXAWAL | SEND NEED PREPARE",
        };
        debug_println!(
            "UIAPP|WALLET|SEND|{}",
            core::str::from_utf8(msg).unwrap_or("?")
        );
        if !self.publish_bytes(msg) {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        MessageInfo::new(REPLY_OK, 0)
    }

    fn balance_stub(&mut self) -> MessageInfo {
        if !self.open_wallet() {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        let bal = balance_atomic(&self.state);
        let mut buf = [0u8; 96];
        let mut n = 0;
        let prefix = b"NEXAWAL | BALANCE ";
        buf[n..n + prefix.len()].copy_from_slice(prefix);
        n += prefix.len();
        let mut value = bal;
        let mut digits = [0u8; 20];
        let mut i = digits.len();
        if value == 0 {
            i -= 1;
            digits[i] = b'0';
        } else {
            while value > 0 {
                i -= 1;
                digits[i] = b'0' + (value % 10) as u8;
                value /= 10;
            }
        }
        let dig = &digits[i..];
        buf[n..n + dig.len()].copy_from_slice(dig);
        n += dig.len();
        if self.state.open {
            let mid = b" | ";
            if n + mid.len() + 12 <= buf.len() {
                buf[n..n + mid.len()].copy_from_slice(mid);
                n += mid.len();
                let addr = self.state.address().as_bytes();
                let take = addr.len().min(12);
                buf[n..n + take].copy_from_slice(&addr[..take]);
                n += take;
            }
        }
        if !self.publish_bytes(&buf[..n]) {
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        MessageInfo::new(REPLY_OK, 0)
    }
}

impl Handler for WalletService {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        let _ = channels;
        #[cfg(feature = "wallet-sync-timer")]
        if self.bulk.is_some() && self.sync_phase != SyncPhase::Range {
            self.cancel_bulk_scan();
        }
        #[cfg(feature = "wallet-sync-timer-test")]
        if TIMER_TEST_START.load(Ordering::Relaxed) == 0 {
            self.sync_rpc = SyncRpcKind::GetBlock;
            let _ = SYNC_TIMER_CHANNEL.pp_call(MessageInfo::new(0, 0));
            TIMER_TEST_START.store(get_mr(0) as usize, Ordering::Relaxed);
            self.schedule_scan_request(true);
            debug_println!("UIAPP|WALLET|TIMER TEST ARMED");
            return Ok(());
        }
        #[cfg(feature = "wallet-sync-timer")]
        if channels.contains(SYNC_TIMER_CHANNEL) && self.sync_phase == SyncPhase::WaitTimer {
            #[cfg(feature = "wallet-sync-timer-test")]
            {
                let _ = SYNC_TIMER_CHANNEL.pp_call(MessageInfo::new(0, 0));
                let elapsed = (get_mr(0) as usize).saturating_sub(TIMER_TEST_START.load(Ordering::Relaxed));
                if TIMER_TEST_PHASE.load(Ordering::Relaxed) == 0 {
                    debug_println!("UIAPP|WALLET|BACKOFF TIMER {} elapsed_ns={}", if elapsed >= 1_000_000_000 { "PASS" } else { "FAIL" }, elapsed);
                    TIMER_TEST_PHASE.store(1, Ordering::Relaxed);
                    TIMER_TEST_START.store(get_mr(0) as usize, Ordering::Relaxed);
                    self.sync_rate_retries = 0;
                    self.schedule_scan_request(false);
                } else {
                    debug_println!("UIAPP|WALLET|PACE TIMER {} elapsed_ns={}", if elapsed >= 250_000_000 { "PASS" } else { "FAIL" }, elapsed);
                    self.sync_phase = SyncPhase::Idle;
                }
                return Ok(());
            }
            #[cfg(not(feature = "wallet-sync-timer-test"))]
            {
            self.sync_phase = if self.sync_timer_restart { SyncPhase::Start } else { SyncPhase::WaitConnect };
            self.sync_timer_restart = false;
            self.sync_polls = 0;
            self.tcp_open_sent = false;
            let _ = self.publish_progress(b"NEXAWAL | SYNC CONNECTING");
            }
        }
        if let Some(endpoint) = self.pending_node.take() {
            if self.persist_node(endpoint) {
                self.node = endpoint;
                self.clear_payment_validation();
                self.node_verified = false;
                self.fee_rate = None;
                self.payment_preview = None;
                self.distribution_request_pending = false;
                self.ring_request_pending = false;
                self.distribution_download = None;
                self.distribution = None;
                self.ring_session = None;
                self.restored_rings.clear();
                self.ring_targets.clear();
                if let Some(evidence) = self.pending_evidence.as_mut() { evidence.invalidate(); }
                self.tip_locked = false;
                let _ = self.publish_bytes(b"NEXAWAL | NODE SAVED");
            } else {
                let _ = self.publish_bytes(b"NEXAWAL | NODE SAVE FAILED");
            }
            WALLET_SHELL_CHANNEL.notify();
        }
        if let Some(words) = self.unlock_request.take() {
            if self.unlock_identity(words) {
                let _ = self.publish_status();
            } else {
                let _ = self.publish_bytes(b"NEXAWAL | UNLOCK FAILED");
            }
            WALLET_SHELL_CHANNEL.notify();
        }
        if let Some((network, height, words)) = self.restore_request.take() {
            match self.identity_slot_available() {
                Ok(false) => { let _ = self.publish_bytes(b"NEXAWAL | WALLET ALREADY EXISTS"); WALLET_SHELL_CHANNEL.notify(); }
                Err(()) => { let _ = self.publish_bytes(b"NEXAWAL | IDENTITY STORAGE FAILED"); WALLET_SHELL_CHANNEL.notify(); }
                Ok(true) => match WalletIdentity::from_mnemonic(words, height, network) {
                Ok(identity) => {
                    debug_println!("UIAPP|WALLET|RESTORE VALID");
                    self.pending_creation = Some((identity, Zeroizing::new(String::new())));
                    self.commit_creation = true;
                }
                Err(_) => {
                    debug_println!("UIAPP|WALLET|RESTORE INVALID");
                    let _ = self.publish_bytes(b"NEXAWAL | INVALID BACKUP");
                    WALLET_SHELL_CHANNEL.notify();
                }
                }
            }
        }
        if let Some((network, _height)) = self.creation_request.take() {
            match self.identity_slot_available() {
                Ok(false) => {
                    debug_println!("UIAPP|WALLET|IDENTITY ALREADY EXISTS");
                    let _ = self.publish_bytes(b"NEXAWAL | WALLET ALREADY EXISTS");
                }
                Err(()) => {
                    debug_println!("UIAPP|WALLET|IDENTITY STORAGE FAILED");
                    let _ = self.publish_bytes(b"NEXAWAL | IDENTITY STORAGE FAILED");
                }
                Ok(true) => {
                    self.creation_height_lookup = Some(network);
                    self.sync_rpc = SyncRpcKind::GetInfo;
                    self.sync_phase = SyncPhase::Start;
                    self.sync_polls = 0;
                    self.sync_recv_retries = 0;
                    let _ = self.publish_bytes(b"NEXAWAL | FETCHING CREATION HEIGHT");
                }
            }
            WALLET_SHELL_CHANNEL.notify();
        }
        if self.commit_creation {
            self.commit_creation = false;
            let saved = self.persist_pending_identity();
            debug_println!("UIAPP|WALLET|IDENTITY SAVE {}", if saved.is_ok() { "OK" } else { "FAILED" });
            let status: &[u8] = match saved {
                Ok(()) => b"NEXAWAL | WALLET SAVED",
                Err(true) => b"NEXAWAL | IDENTITY CONFLICT",
                Err(false) => b"NEXAWAL | SAVE FAILED",
            };
            let _ = self.publish_bytes(status);
            WALLET_SHELL_CHANNEL.notify();
        }
        if self.open_pending {
            self.open_pending = false;
            debug_println!("UIAPP|WALLET|OPEN|DERIVE");
            if self.open_wallet() {
                let _ = self.publish_status();
            } else {
                let _ = self.publish_bytes(b"NEXAWAL | SETUP REQUIRED");
            }
            WALLET_SHELL_CHANNEL.notify();
        }
        self.apply_refresh_pending();
        if self.distribution_request_pending && self.sync_phase == SyncPhase::Idle {
            self.distribution_request_pending = false;
            let budget = HEAP_BYTES.saturating_sub(heap_used_bytes()).saturating_sub(1_500_000) / 2;
            self.distribution_download = if self.state.open && self.node_verified && self.fee_rate.is_some()
                && self.restore_chain.as_ref().is_some_and(|chain| chain.is_verified() && chain.next() == self.daemon_tip_height) {
                self.daemon_tip_height.checked_sub(1).and_then(|height| DistributionDownload::new(height, budget))
            } else { None };
            self.sync_rpc = SyncRpcKind::GetDistribution;
            if self.distribution_download.is_some() { self.arm_distribution_chunk(); }
            else { self.sync_fail(b"NEXAWAL | DISTRIBUTION MEMORY"); }
        }
        if self.ring_request_pending && self.sync_phase == SyncPhase::Idle {
            self.ring_request_pending = false;
            if self.state.open && self.node_verified && self.distribution.is_some()
                && self.restore_chain.as_ref().is_some_and(|chain| chain.is_verified() && chain.next() == self.daemon_tip_height) {
                self.start_restored_ring();
            } else {
                self.sync_rpc = SyncRpcKind::GetRingBatch;
                self.sync_fail(b"NEXAWAL | RING STATE CHANGED");
            }
        }
        if self.validation_pending && self.sync_phase == SyncPhase::Idle {
            self.validation_pending = false;
            self.payment_validation = self.payment_preview.as_ref()
                .and_then(|preview| PaymentRevalidation::new(&self.state, &self.restored_rings, preview));
            self.validation_statuses.clear();
            self.sync_rpc = SyncRpcKind::ValidateInfo;
            if self.payment_validation.is_some() {
                self.arm_validation_rpc(SyncRpcKind::ValidateInfo, get_info_request_body(), "/json_rpc");
            } else { self.sync_fail(b"NEXAWAL | VALIDATION STATE"); }
        }
        // The range pump owns a one-shot timer. It also accepts shell wakes
        // while a response is flowing so prefetch continues between scan steps.
        #[cfg(feature = "wallet-sync-timer")]
        if self.sync_phase == SyncPhase::Range {
            #[cfg(feature = "wallet-range-profile")]
            if channels.contains(wallet_tcp::WALLET_DHCP_CHANNEL) {
                self.range_profile.socket_signals += 1;
            }
            self.pump_bulk_scan();
            return Ok(());
        }
        // Pump on any wake while syncing (shell bounce, worker, or spurious).
        if self.sync_phase != SyncPhase::Idle {
            #[cfg(feature = "wallet-sync-timer")]
            if self.sync_phase == SyncPhase::WaitTimer { return Ok(()); }
            // Recv burst: 8+ starved dhcp into BAD HTTP (dump106/115). Stay at 4
            // and rely on shell notify bounce + tip-abandon for peer hangs.
            let steps = 4;
            for _ in 0..steps {
                if self.sync_phase == SyncPhase::Idle {
                    break;
                }
                self.sync_advance();
            }
            // dump185: always wake shell after a pump — CONNECT FAIL / tip
            // abandon leave Idle and used to skip this notify, so the shell
            // never re-peeked and the FB froze on SYNC CONNECTING (dump184).
            WALLET_SHELL_CHANNEL.notify();
        }
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel != WALLET_SHELL_CHANNEL {
            return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
        }
        Ok(match request.label() {
            REQUEST_STATUS if request.count() == 0 => self.status(),
            wallet_protocol::REQUEST_VALIDATE_PREVIEW if request.count() == 0 => self.request_preview_validation(false),
            wallet_protocol::REQUEST_SIGN_TEST_PREVIEW if request.count() == 0 => self.request_preview_validation(true),
            wallet_protocol::REQUEST_SET_NODE if request.count() == 0 => self.set_node(),
            wallet_protocol::REQUEST_GET_NODE if request.count() == 0 && self.state.open => {
                let endpoint = self.node;
                if self.publish_bytes(endpoint.authority().as_bytes()) { MessageInfo::new(REPLY_OK, 0) }
                else { MessageInfo::new(REPLY_BAD_REQUEST, 0) }
            },
            REQUEST_CONFIRM_BACKUP if request.count() == 0 => self.confirm_backup(),
            REQUEST_LOCK if request.count() == 0 => self.lock_wallet(),
            REQUEST_UNLOCK if request.count() == 0 => self.request_unlock(),
            REQUEST_CANCEL_CREATE if request.count() == 0 && !self.commit_creation => {
                self.creation_request = None;
                if self.creation_height_lookup.take().is_some() {
                    self.tcp_mut().close();
                    self.sync_phase = SyncPhase::Idle;
                    self.tcp_open_sent = false;
                }
                self.restore_request = None;
                self.pending_creation = None;
                unsafe { self.transfer.as_mut() }.set_payload(&[]);
                MessageInfo::new(REPLY_OK, 0)
            },
            REQUEST_OPEN if request.count() == 0 => self.open_request(),
            REQUEST_RESTORE if request.count() == 2 => {
                let network = match get_mr(0) {
                    0 => Some(Network::Mainnet),
                    1 => Some(Network::Stagenet),
                    2 => Some(Network::Testnet),
                    _ => None,
                };
                match network {
                    Some(network) => self.request_restore(network, get_mr(1)),
                    None => {
                        unsafe { self.transfer.as_mut() }.set_payload(&[]);
                        MessageInfo::new(REPLY_BAD_REQUEST, 0)
                    }
                }
            },
            REQUEST_CREATE if request.count() == 2 => {
                let network = match get_mr(0) {
                    0 => Some(Network::Mainnet),
                    1 => Some(Network::Stagenet),
                    2 => Some(Network::Testnet),
                    _ => None,
                };
                match network {
                    Some(network) => self.request_creation(network, get_mr(1)),
                    None => MessageInfo::new(REPLY_BAD_REQUEST, 0),
                }
            },
            REQUEST_BALANCE if request.count() == 0 => self.balance_stub(),
            REQUEST_REFRESH if request.count() == 0 => self.queue_refresh(),
            REQUEST_PREPARE_SEND if request.count() == 0 => self.prepare_send_stub(),
            REQUEST_SEND if request.count() == 0 => self.send_tx_stub(),
            REQUEST_RECONCILE if request.count() == 0 => self.reconcile_pending(),
            _ => MessageInfo::new(REPLY_BAD_REQUEST, 0),
        })
    }
}
