#![no_std]
#![no_main]

extern crate alloc;

use alloc::{rc::Rc, string::String, string::ToString, vec, vec::Vec};
use core::cell::{Cell, RefCell};
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU16, AtomicU64, AtomicUsize, Ordering},
};
mod skia_guest;
#[cfg(feature = "desktop-real-app")]
#[path = "../../desktop-apps/input.rs"]
mod desktop_window;
#[cfg(feature = "desktop-apps-observe")]
#[path = "../../desktop-apps/observe.rs"]
mod desktop_observation;
#[cfg(all(feature = "desktop-simplex-frontend", not(feature = "desktop-real-app")))]
#[path = "../../desktop-simplex/frontend_probe.rs"]
mod frontend_probe;

use personal_os_rust_server::messages::{decode_direct_text, split_direct_text};
use personal_os_rust_server::messages::{photo_reference, split_file_reference, split_message_time, split_photo_reference};
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use personal_os_slint_notes_spike::{
    NotesWindow,
    frame::{FRAME_HEIGHT, FRAME_PIXEL_BYTES, FRAME_STRIDE_BYTES, FRAME_WIDTH, FrameSurfaceHeader},
    wallet_status::{scan_complete, scan_progress, waiting_for_deposit, wallet_node_authority},
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr,
    memory_region_symbol, protection_domain, set_mr,
};
use slint::{
    ComponentHandle, Model, ModelRc, PhysicalSize, VecModel,
    platform::{
        WindowAdapter,
        software_renderer::{LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType},
    },
};

const WIDTH: usize = FRAME_WIDTH as usize;
const HEIGHT: usize = FRAME_HEIGHT as usize;
const HEAP_BYTES: usize = 0x2000000;
// Match the Pixel 4a panel: 1080x2340 at the same scale as the Pixel guest.
const SCALE: f32 = if cfg!(feature = "desktop-real-app") { 2.0 } else { 2.6086957 };

fn clock_view_address() -> usize {
    *sel4_microkit::var!(slint_clock_view: usize = 0)
}

#[cfg(feature = "utm-perf-trace")]
fn perf_time_us() -> u64 {
    let ticks: u64;
    let frequency: u64;
    unsafe {
        core::arch::asm!("isb", "mrs {v}, cntpct_el0", v = out(reg) ticks, options(nostack, preserves_flags));
        core::arch::asm!("mrs {v}, cntfrq_el0", v = out(reg) frequency, options(nostack, preserves_flags));
    }
    ticks / (frequency / 1_000_000).max(1)
}

#[cfg(feature = "simplex-agent-ui")]
macro_rules! profile_live_call {
    ($name:literal, $operation:expr) => {{
        #[cfg(feature = "utm-perf-trace")]
        let started = perf_time_us();
        let result = $operation;
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|LIVE_STAGE|{}|{}|{}", $name, started, perf_time_us());
        result
    }};
}
const CHAT_SLOTS: usize = personal_os_rust_server::messages::CHAT_SLOT_COUNT;
const SLINT_INPUT: Channel = Channel::new(1);
const NOTES_SERVICE: Channel = Channel::new(2);
const CONTACTS_SERVICE: Channel = Channel::new(3);
const CALENDAR_SERVICE: Channel = Channel::new(4);
const MESSAGES_SERVICE: Channel = Channel::new(5);
const WALLET_SERVICE: Channel = Channel::new(6);
#[cfg(feature = "utm-photo-worker")]
const PHOTO_WORKER: Channel = Channel::new(9);
#[cfg(feature = "utm-photo-worker")]
const PHOTO_READER: Channel = Channel::new(10);
#[cfg(feature = "utm-transcript-worker")]
const TRANSCRIPT_WORKER: Channel = Channel::new(11);
#[cfg(feature = "utm-transcript-worker")]
const LIVE_RECEIVE_WORKER: Channel = Channel::new(12);
#[cfg(feature = "utm-transcript-worker")]
const NAVIGATION_WORKER: Channel = Channel::new(13);
#[cfg(feature = "utm-transcript-worker")]
const SNAPSHOT_WORKER: Channel = Channel::new(14);
#[cfg(feature = "utm-transcript-worker")]
const _: () = assert!(core::mem::size_of::<personal_os_slint_notes_spike::photo_mailbox::Mailbox<1, 4096>>() <= 0x2000);
#[cfg(feature = "utm-transcript-worker")]
const _: () = assert!(core::mem::size_of::<personal_os_slint_notes_spike::photo_mailbox::Mailbox<8, 1>>() <= 0x1000);
/// Diagnostic: shell→dhcp STATUS (channel 7). Proves whether dhcp PPC replies to anyone.
const DHCP_TCP_PROBE: Channel = Channel::new(7);
const REQUEST_WALLET_STATUS: u64 = 0x501;
const REQUEST_WALLET_OPEN: u64 = 0x502;
const REQUEST_WALLET_BALANCE: u64 = 0x503;
const REQUEST_WALLET_REFRESH: u64 = 0x506;
const REQUEST_WALLET_PREPARE_SEND: u64 = 0x507;
const REQUEST_WALLET_SEND: u64 = 0x508;
const REPLY_WALLET_OK: u64 = 0x500;
const REQUEST_SLINT_EVENT: u64 = 0x530;
const REPLY_SLINT_EVENT_OK: u64 = REQUEST_SLINT_EVENT + 1;
const REQUEST_NOTES_LOAD: u64 = 0x320;
const REQUEST_NOTES_SAVE: u64 = 0x321;
const REPLY_NOTES_LOAD: u64 = 0x322;
const REPLY_NOTES_SAVE: u64 = 0x323;
const REQUEST_CONTACTS_LOAD: u64 = 0x330;
const REQUEST_CONTACTS_SAVE: u64 = 0x331;
const REPLY_CONTACTS_LOAD: u64 = 0x332;
const REPLY_CONTACTS_SAVE: u64 = 0x333;
const REQUEST_CALENDAR_LOAD: u64 = 0x340;
const REQUEST_CALENDAR_SAVE: u64 = 0x341;
const REPLY_CALENDAR_LOAD: u64 = 0x342;
const REPLY_CALENDAR_SAVE: u64 = 0x343;
const REQUEST_MESSAGES_LOAD: u64 = 0x350;
const REPLY_MESSAGES_LOAD: u64 = 0x352;
const REQUEST_MESSAGES_CREATE_INVITATION: u64 = 0x353;
const REPLY_MESSAGES_CREATE_INVITATION: u64 = 0x354;
const REQUEST_MESSAGES_INVITATION: u64 = 0x355;
const REPLY_MESSAGES_INVITATION: u64 = 0x356;
const REQUEST_MESSAGES_JOIN_INVITATION: u64 = 0x357;
const REPLY_MESSAGES_JOIN_INVITATION: u64 = 0x358;
const REQUEST_MESSAGES_JOIN_STATUS: u64 = 0x359;
const REPLY_MESSAGES_JOIN_STATUS: u64 = 0x35a;
const REQUEST_MESSAGES_SEND: u64 = 0x35b;
const REPLY_MESSAGES_SEND: u64 = 0x35c;
const REQUEST_MESSAGES_SEND_STATUS: u64 = 0x35d;
const REPLY_MESSAGES_SEND_STATUS: u64 = 0x35e;
const REQUEST_MESSAGES_RECEIVE: u64 = 0x35f;
const REPLY_MESSAGES_RECEIVE: u64 = 0x360;
const REQUEST_MESSAGES_RECEIVE_STATUS: u64 = 0x361;
const REPLY_MESSAGES_RECEIVE_STATUS: u64 = 0x362;
const REQUEST_MESSAGES_ACK: u64 = 0x363;
const REPLY_MESSAGES_ACK: u64 = 0x364;
const REQUEST_MESSAGES_ACK_STATUS: u64 = 0x365;
const REPLY_MESSAGES_ACK_STATUS: u64 = 0x366;
const REQUEST_MESSAGES_SELECT_CHAT: u64 = 0x367;
const REPLY_MESSAGES_SELECT_CHAT: u64 = 0x368;
const REQUEST_MESSAGES_TRANSCRIPT: u64 = 0x369;
const REPLY_MESSAGES_TRANSCRIPT: u64 = 0x36a;
const REQUEST_MESSAGES_IMAGE: u64 = 0x36b;
const REPLY_MESSAGES_IMAGE: u64 = 0x36c;
const REQUEST_MESSAGES_IMAGE_CHUNK: u64 = 0x36d;
const REPLY_MESSAGES_IMAGE_CHUNK: u64 = 0x36e;
const REQUEST_MESSAGES_GROUP: u64 = 0x36f;
const REPLY_MESSAGES_GROUP: u64 = 0x370;
const REQUEST_MESSAGES_DELETE: u64 = 0x371;
const REPLY_MESSAGES_DELETE: u64 = 0x372;
const REQUEST_MESSAGES_ARCHIVE: u64 = 0x373;
const REPLY_MESSAGES_ARCHIVE: u64 = 0x374;
const REQUEST_MESSAGES_ARCHIVED_MASK: u64 = 0x375;
const REPLY_MESSAGES_ARCHIVED_MASK: u64 = 0x376;
const REQUEST_MESSAGES_PEER_PHOTO: u64 = 0x377;
const REPLY_MESSAGES_PEER_PHOTO: u64 = 0x378;
const REQUEST_MESSAGES_PEER_PHOTO_CHUNK: u64 = 0x379;
const REQUEST_MESSAGES_SET_PROFILE: u64 = 0x37b;
const REPLY_MESSAGES_SET_PROFILE: u64 = 0x37c;
const REQUEST_MESSAGES_CREATE_GROUP: u64 = 0x37d;
const REPLY_MESSAGES_CREATE_GROUP: u64 = 0x37e;
const REQUEST_MESSAGES_CREATE_ADDRESS: u64 = 0x37f;
const REPLY_MESSAGES_CREATE_ADDRESS: u64 = 0x380;
const REQUEST_MESSAGES_ADDRESS: u64 = 0x381;
const REPLY_MESSAGES_ADDRESS: u64 = 0x382;
#[cfg(feature = "simplex-agent-ui")]
const REQUEST_MESSAGES_HISTORY_INFO: u64 = 0x383;
#[cfg(feature = "simplex-agent-ui")]
const REPLY_MESSAGES_HISTORY_INFO: u64 = 0x384;
#[cfg(feature = "simplex-agent-ui")]
const REQUEST_MESSAGES_HISTORY_PAGE: u64 = 0x385;
const REQUEST_MESSAGES_SEND_SAVED_PHOTO: u64 = 0x387;
const REPLY_MESSAGES_SEND_SAVED_PHOTO: u64 = 0x388;
const REQUEST_LIBRARY_CLONE: u64 = 0x389;
const REPLY_LIBRARY_CLONE: u64 = 0x38a;
const REQUEST_LIBRARY_NEXT: u64 = 0x38b;
const REPLY_LIBRARY_NEXT: u64 = 0x38c;
const REQUEST_LIBRARY_READ: u64 = 0x38d;
const REPLY_LIBRARY_READ: u64 = 0x38e;
const REQUEST_LIBRARY_DELETE: u64 = 0x38f;
const REPLY_LIBRARY_DELETE: u64 = 0x390;
#[cfg(feature = "utm-file-import-test")]
const REQUEST_LIBRARY_STAGE: u64 = 0x391;
#[cfg(feature = "utm-file-import-test")]
const REPLY_LIBRARY_STAGE: u64 = 0x392;
#[cfg(feature = "utm-file-import-test")]
const REQUEST_LIBRARY_IMPORT: u64 = 0x393;
#[cfg(feature = "utm-file-import-test")]
const REPLY_LIBRARY_IMPORT: u64 = 0x394;
#[cfg(feature = "simplex-agent-ui")]
const REPLY_MESSAGES_HISTORY_PAGE: u64 = 0x386;
const REPLY_MESSAGES_PEER_PHOTO_CHUNK: u64 = 0x37a;
const DEVICE_KEYBOARD: u64 = 0;
const DEVICE_TABLET: u64 = 1;
const EVENT_KEY: u64 = 1;
const EVENT_SYN: u64 = 0;
const EVENT_ABS: u64 = 3;
const BTN_LEFT: u64 = 272;
/// Logical-px edge for Android-style back swipe (cards start ~17px).
const NAV_EDGE_PX: f32 = 36.0;
/// Bottom band for home / recents swipe (iPhone home-bar / Android gesture).
const NAV_HOME_BAND_PX: f32 = 56.0;
const NAV_SWIPE_MIN_PX: f32 = 48.0;
const NAV_SWIPE_MAX_CROSS_PX: f32 = 96.0;
/// Longer upward swipe from the home band opens the soft switcher.
const NAV_RECENTS_MIN_PX: f32 = 160.0;
const KEY_ESC: u64 = 1;
const KEY_BACK: u64 = 158;
const KEY_HOME: u64 = 102;

#[repr(C)]
struct UiPointerState {
    revision: core::sync::atomic::AtomicU32,
    x: u32,
    y: u32,
    visible: u32,
}

#[derive(Clone, Copy)]
struct NavGesture {
    active: bool,
    /// Press started in an edge/home zone — hold Slint press until release
    /// so a swipe can become system nav instead of a UI drag.
    defer_press: bool,
    feedback_press: bool,
    start_x: f32,
    start_y: f32,
    last_x: f32,
    last_y: f32,
    from_edge: bool,
    from_home: bool,
}

static HEAP_USED: AtomicUsize = AtomicUsize::new(0);
type HeapMutex = sel4_microkit::_private::heap::RawOneShotMutex;
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<HeapMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();

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

struct RgbLines {
    output: NonNull<u8>,
}
impl LineBufferProvider for RgbLines {
    type TargetPixel = slint::Rgb8Pixel;
    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        if line >= HEIGHT || range.end > WIDTH {
            return;
        }
        // Slint's RGB8 pixel is repr(C) RGB<u8>, exactly the three bytes used
        // by the shared frame. Render into that range directly instead of
        // clearing a temporary row and copying every channel one at a time.
        const { assert!(core::mem::size_of::<slint::Rgb8Pixel>() == 3); }
        const { assert!(core::mem::align_of::<slint::Rgb8Pixel>() == 1); }
        let offset = line * FRAME_STRIDE_BYTES as usize + range.start * 3;
        let pixels = unsafe {
            core::slice::from_raw_parts_mut(
                self.output.as_ptr().add(offset).cast::<slint::Rgb8Pixel>(),
                range.len(),
            )
        };
        render_fn(pixels);
    }
}

#[cfg(feature = "utm-photo-worker")]
#[derive(Clone, Copy)]
struct PhotoTicket {
    stamp: u64,
    chat: u64,
    row: usize,
    id: u64,
    owner: u64,
    cache_generation: u64,
}

#[cfg(feature = "utm-device-lock")]
mod device_lock_utm;

struct SlintService {
    #[cfg(feature = "utm-device-lock")]
    device_lock: Rc<RefCell<device_lock_utm::DeviceLock>>,
    frame_timer: Option<Channel>,
    frame_deadline: Option<core::time::Duration>,
    _ui: NotesWindow,
    window: Rc<MinimalSoftwareWindow>,
    header: NonNull<FrameSurfaceHeader>,
    pixels: NonNull<u8>,
    rendered: alloc::vec::Vec<u8>,
    pointer: NonNull<UiPointerState>,
    shift_down: bool,
    meta_down: bool,
    ctrl_down: bool,
    notes_service: Rc<RefCell<NotesServiceClient>>,
    contacts_service: Rc<RefCell<ContactsServiceClient>>,
    calendar_service: Rc<RefCell<CalendarServiceClient>>,
    #[cfg(feature = "simplex-agent-ui")]
    messages_service: Rc<RefCell<MessagesServiceClient>>,
    #[cfg(feature = "simplex-agent-ui")]
    chat_transcripts: Rc<RefCell<[String; CHAT_SLOTS]>>,
    #[cfg(feature = "simplex-agent-ui")]
    chat_drafts: Rc<RefCell<[String; CHAT_SLOTS]>>,
    #[cfg(feature = "simplex-agent-ui")]
    chat_photo_loader: Rc<RefCell<personal_os_slint_notes_spike::chat_photo_loader::PhotoLoader>>,
    #[cfg(feature = "utm-photo-worker")]
    photo_mailbox: &'static personal_os_slint_notes_spike::photo_ipc::PhotoMailbox,
    #[cfg(feature = "utm-photo-worker")]
    pending_photo: Option<PhotoTicket>,
    #[cfg(feature = "utm-photo-worker")]
    pending_photo_read: Option<PhotoTicket>,
    #[cfg(feature = "utm-photo-worker")]
    photo_read_mailbox: &'static personal_os_slint_notes_spike::photo_reader_ipc::PhotoReadMailbox,
    #[cfg(feature = "utm-photo-worker")]
    photo_cache: Rc<RefCell<personal_os_slint_notes_spike::photo_cache::PhotoCache<slint::Image>>>,
    shown_view: i32,
    shown_stamp: u64,
    pictures_prepared: bool,
    needs_catch_up: bool,
    prepare_one: bool,
    #[cfg(feature = "simplex-agent-ui")]
    transcripts_loaded: bool,
    #[cfg(feature = "simplex-agent-ui")]
    chat_read_len: [usize; CHAT_SLOTS],
    #[cfg(feature = "utm-transcript-worker")]
    history_baselined: [bool; CHAT_SLOTS],
    #[cfg(feature = "utm-transcript-worker")]
    initial_unread: [usize; CHAT_SLOTS],
    #[cfg(feature = "simplex-agent-ui")]
    last_unread: [usize; CHAT_SLOTS],
    wallet_service: WalletServiceClient,
    /// First visit to NexaWal runs open + daemon tip refresh once.
    wallet_opened: bool,
    /// Deferred boot sync phases: 0 idle/done, 1 show, 2 open, 3 queue sync,
    /// 5 poll transfer for tip/fail, 4 vault catch-up.
    wallet_boot_phase: u8,
    wallet_boot_sync_queued: bool,
    wallet_boot_opened: alloc::string::String,
    wallet_sync_polls: u16,
    /// Skip timer tip-mid refresh for this many pulses (avoids abort thrash).
    wallet_tip_rearm_cool: u16,
    /// Keep pumping wallet TCP + status peeks without requiring tablet motion.
    wallet_sync_active: bool,
    /// Force a full paint when the wallet face balance or sync line changes.
    wallet_face_tab_dirty: bool,
    /// After a tap paints a new page, skip later publishes so gpu_client can
    /// copy one stable frame. Sync status text resumes when this hits zero.
    scanout_hold: u8,
    /// While draining a burst of keys, remember that the screen changed and
    /// paint once at the end. A full frame per character overflows the queue.
    defer_frame: bool,
    frame_dirty: bool,
    feedback_frame_dirty: bool,
    #[cfg(feature = "simplex-agent-ui")]
    chat_refresh: personal_os_slint_notes_spike::chat_refresh::ChatRefresh,
    /// CONNECT KEEP cycles with no progress. After many, soft-rearm OPEN once
    /// — notify-only can leave WaitConnect PENDING forever with no SYN on TCG.
    wallet_connect_keep_cycles: u8,
    nav_gesture: NavGesture,
    /// Last non-home app page for a simple recents jump.
    nav_last_app_page: i32,
    #[cfg(feature = "desktop-real-app")]
    desktop_held: [Option<slint::SharedString>; 256],
    #[cfg(feature = "desktop-real-app")]
    desktop_pointer: Option<slint::LogicalPosition>,
}

/// NexaWal shell client: opcodes only; payload lives on wallet_transfer.
struct WalletServiceClient {
    transfer: NonNull<BlobTransferPage>,
}

impl WalletServiceClient {
    fn call(&self, request: u64) -> Result<String, alloc::string::String> {
        let reply = WALLET_SERVICE.pp_call(MessageInfo::new(request, 0));
        if reply.label() != REPLY_WALLET_OK || reply.count() != 0 {
            return Err(alloc::format!(
                "REJECT label=0x{:x} count={}",
                reply.label(),
                reply.count()
            ));
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or_else(|| alloc::string::String::from("BAD TRANSFER LEN"))?;
        core::str::from_utf8(payload)
            .map(|text| text.to_string())
            .map_err(|_| alloc::string::String::from("TRANSFER NOT UTF8"))
    }

    fn smoke_round_trip(&self) -> String {
        // OPEN only — STATUS+BALANCE tripled the PPC cost and left ARMED on
        // screen for minutes while TCG ground Monero key derivation.
        match self.call(REQUEST_WALLET_OPEN) {
            Ok(o) => {
                debug_println!("SLINT WALLET OPEN OK");
                o
            }
            Err(detail) => {
                debug_println!("SLINT WALLET OPEN FAIL");
                self.peek_transfer().filter(|s| s.contains("SETUP REQUIRED"))
                    .unwrap_or_else(|| alloc::format!("WALLET IPC FAIL · {detail}"))
            }
        }
    }

    /// Queue daemon tip sync in the wallet PD (returns immediately). Shell must
    /// `WALLET_SERVICE.notify()` so the active wallet runs TCP outside the Call nest.
    fn queue_refresh(&self) -> String {
        match self.call(REQUEST_WALLET_REFRESH) {
            Ok(text) => {
                debug_println!("SLINT WALLET REFRESH QUEUED");
                // Caller starts the poll pump with WALLET_SERVICE.notify() once
                // the UI frame is up — notifying here from init starved gpu.
                text
            }
            Err(_) => {
                debug_println!("SLINT WALLET REFRESH FAIL");
                "WALLET SYNC FAILED".into()
            }
        }
    }

    fn peek_transfer(&self) -> Option<String> {
        let payload = unsafe { self.transfer.as_ref() }.payload()?;
        core::str::from_utf8(payload).ok().map(|t| t.to_string())
    }

    /// True when transfer shows tip height or a terminal sync failure.
    fn sync_terminal(text: &str) -> bool {
        if text.contains("PREVIEW FEE ") && (text.contains(" | SEND NOT READY") || text.contains(" | TEST SEND READY")) { return true; }
        if text.contains("CONNECT FAIL")
            || text.contains("SEND FAIL")
            || text.contains("RECV FAIL")
            || text.contains("BAD HTTP")
            || text.contains("BAD JSON")
            || text.contains("BAD REQ")
            || text.contains("OPEN FAIL")
            || text.contains("SCAN FAIL")
            || text.contains("SCAN NO ")
            || text.contains("SCAN BAD")
            || text.contains("SYNC TIMEOUT")
            || text.contains("SYNC FAIL")
            || text.contains("TLS TIMEOUT")
            || text.contains("TLS FAILED")
            || text.contains("TLS INIT FAILED")
            || text.contains("TLS STATE LOST")
            || text.contains("CLOCK UNAVAILABLE")
            || text.contains("VALIDATION CHAIN CHANGED")
            || text.contains("VALIDATION INPUT SPENT")
            || text.contains("VALIDATION SPENT FAILED")
            || text.contains("VALIDATION FEE FAILED")
            || text.contains("VALIDATION REQUEST FAILED")
            || text.contains("VALIDATION STATE")
            || text.contains("PREVIEW CHANGED")
            || text.contains("ENTROPY UNAVAILABLE")
            || text.contains("SAVE FAILED")
        {
            return true;
        }
        if scan_complete(text) {
            return true;
        }
        // Parked PREPARE INSUFFICIENT (no tip/sync mid-flight) must end the
        // poll so tip rearm can run — dump151 left wallet_sync_active stuck
        // true and tip-busy taps never refreshed. Mid-flight
        // "INSUFFICIENT · CONNECTING" stays non-terminal (dump130).
        if text.contains("PREPARE")
            && text.contains("INSUFFICIENT")
            && !text.contains("CONNECTING")
            && !text.contains("QUEUED")
            && !text.contains("TIP ")
            && !text.contains(" RX")
            && !text.contains(" NET")
            && !text.contains("FETCH")
        {
            return true;
        }
        // PREPARE OK (and hard prepare failures) still end the poll.
        if text.contains("PREPARE")
            && (text.contains("PREPARE OK")
                || text.contains("CLOSED")
                || text.contains("BAD ADDR")
                || text.contains("NEED DECOYS")
                || text.contains("SIGN FAIL")
                || text.contains("PREPARE FAIL"))
        {
            return true;
        }
        if text.contains("SEND OK")
            || text.contains("SEND UNCONFIRMED")
            || text.contains("SEND REJECTED")
            || text.contains("SEND FAIL")
            || text.contains("SEND INSUFFICIENT")
            || text.contains("SEND NEED")
        {
            return true;
        }
        false
    }

    fn refresh(&self) -> String {
        // Manual tap path: queue + notify; caller paints after a later poll.
        self.queue_refresh()
    }

    fn prepare_send(&self) -> String {
        match self.call(REQUEST_WALLET_PREPARE_SEND) {
            Ok(text) => {
                debug_println!("SLINT WALLET PREPARE OK");
                text
            }
            Err(err) => {
                debug_println!("SLINT WALLET PREPARE FAIL");
                alloc::format!("WALLET PREPARE FAIL · {err}")
            }
        }
    }

    fn send_tx(&self) -> String {
        match self.call(REQUEST_WALLET_SEND) {
            Ok(text) => {
                debug_println!("SLINT WALLET SEND OK");
                text
            }
            Err(err) => {
                debug_println!("SLINT WALLET SEND FAIL");
                alloc::format!("WALLET SEND FAIL · {err}")
            }
        }
    }

    fn confirm_test_preview(&mut self, request: &str) -> String {
        if !unsafe { self.transfer.as_mut() }.set_payload(request.as_bytes()) {
            return "NEXAWAL | PREPARE BAD REQUEST".into();
        }
        match self.call(0x513) {
            Ok(text) => text,
            Err(_) => "NEXAWAL | PREVIEW CHANGED".into(),
        }
    }
}

/// Client for the Notes app service. This UI protection domain has no direct
/// vault capability; it can only request a bounded load/save from Notes.
struct NotesServiceClient {
    transfer: NonNull<BlobTransferPage>,
}

impl NotesServiceClient {
    fn load(&self) -> Result<String, &'static str> {
        let reply = NOTES_SERVICE.pp_call(MessageInfo::new(REQUEST_NOTES_LOAD, 0));
        if reply.label() != REPLY_NOTES_LOAD || reply.count() != 1 {
            return Err("Notes service load failed");
        }
        let length = get_mr(0) as usize;
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Notes transfer length")?;
        if payload.len() != length {
            return Err("invalid Notes service payload");
        }
        String::from_utf8(payload.into()).map_err(|_| "Notes text is not UTF-8")
    }

    fn save_and_verify(&mut self, text: &[u8]) -> Result<(), &'static str> {
        if !unsafe { self.transfer.as_mut() }.set_payload(text) {
            return Err("Notes text exceeds the transfer page");
        }
        set_mr(0, text.len() as u64);
        let reply = NOTES_SERVICE.pp_call(MessageInfo::new(REQUEST_NOTES_SAVE, 1));
        if reply.label() != REPLY_NOTES_SAVE || reply.count() != 0 {
            debug_println!(
                "SLINT NOTES SAVE REPLY LABEL {} COUNT {}",
                reply.label(),
                reply.count()
            );
            return Err("Notes service save failed");
        }
        if self.load()?.as_bytes() != text {
            return Err("Notes readback did not match");
        }
        Ok(())
    }
}

/// Contacts domain mediates the selected record through its own namespace.
struct ContactsServiceClient {
    transfer: NonNull<BlobTransferPage>,
}

impl ContactsServiceClient {
    fn load(&self) -> Result<(String, String), &'static str> {
        let reply = CONTACTS_SERVICE.pp_call(MessageInfo::new(REQUEST_CONTACTS_LOAD, 0));
        if reply.label() != REPLY_CONTACTS_LOAD || reply.count() != 2 {
            return Err("Contacts service load failed");
        }
        let name_length = get_mr(0) as usize;
        let phone_length = get_mr(1) as usize;
        if name_length > 40 || phone_length > 24 {
            return Err("Contacts service returned invalid field lengths");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Contacts transfer length")?;
        if payload.len() != name_length + phone_length {
            return Err("invalid Contacts service payload");
        }
        let name = String::from_utf8(payload[..name_length].into())
            .map_err(|_| "Contact name is not UTF-8")?;
        let phone = String::from_utf8(payload[name_length..].into())
            .map_err(|_| "Contact phone is not UTF-8")?;
        Ok((name, phone))
    }

    fn save_and_verify(&mut self, name: &str, phone: &str) -> Result<(), &'static str> {
        if name.is_empty() || name.len() > 40 || phone.len() > 24 {
            return Err("Contact fields exceed their limits");
        }
        let mut payload = alloc::vec::Vec::with_capacity(name.len() + phone.len());
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(phone.as_bytes());
        if !unsafe { self.transfer.as_mut() }.set_payload(&payload) {
            return Err("Contact fields exceed the transfer page");
        }
        set_mr(0, name.len() as u64);
        set_mr(1, phone.len() as u64);
        let reply = CONTACTS_SERVICE.pp_call(MessageInfo::new(REQUEST_CONTACTS_SAVE, 2));
        if reply.label() != REPLY_CONTACTS_SAVE || reply.count() != 0 {
            return Err("Contacts service save failed");
        }
        if self.load()? != (String::from(name), String::from(phone)) {
            return Err("Contact readback did not match");
        }
        Ok(())
    }
}

struct CalendarServiceClient {
    transfer: NonNull<BlobTransferPage>,
}

/// The Messages app owns its peer/thread model and vault namespace. Slint only
/// receives the bounded display snapshot through the shared transfer page.
struct MessagesServiceClient {
    transfer: NonNull<BlobTransferPage>,
    showing_address: Cell<bool>,
    #[cfg(feature = "simplex-agent-ui")]
    history_cache: RefCell<[Option<(u64, u64, String)>; CHAT_SLOTS]>,
    #[cfg(feature = "utm-transcript-worker")]
    transcript_mailbox: &'static personal_os_slint_notes_spike::transcript_ipc::TranscriptMailbox,
    #[cfg(feature = "utm-transcript-worker")]
    transcript_queue: RefCell<personal_os_slint_notes_spike::transcript_queue::Queue>,
    #[cfg(feature = "utm-transcript-worker")]
    transcript_ticket: Cell<Option<personal_os_slint_notes_spike::transcript_queue::Ticket>>,
    #[cfg(feature = "utm-transcript-worker")]
    transcript_owners: RefCell<[u64; CHAT_SLOTS]>,
    #[cfg(feature = "utm-transcript-worker")]
    force_chat_refresh: Cell<bool>,
    #[cfg(feature = "utm-transcript-worker")]
    live_receive_mailbox: &'static personal_os_slint_notes_spike::photo_mailbox::Mailbox<1, 4096>,
    #[cfg(feature = "utm-transcript-worker")]
    pending_live_receive: RefCell<Option<(u64, u64, String)>>,
    #[cfg(feature = "utm-transcript-worker")]
    live_receive_requested: Cell<bool>,
    #[cfg(feature = "utm-transcript-worker")]
    live_receive_start_requested: Cell<bool>,
    #[cfg(feature = "utm-transcript-worker")]
    live_receive_inflight_start: Cell<bool>,
    #[cfg(feature = "utm-transcript-worker")]
    navigation_mailbox: &'static personal_os_slint_notes_spike::photo_mailbox::Mailbox<8, 1>,
    #[cfg(feature = "utm-transcript-worker")]
    navigation_queue: RefCell<personal_os_slint_notes_spike::navigation_selection::Selection>,
    #[cfg(feature = "utm-transcript-worker")]
    navigation_ticket: Cell<Option<personal_os_slint_notes_spike::navigation_selection::Ticket>>,
    #[cfg(feature = "utm-transcript-worker")]
    navigation_legacy_photo_pending: Cell<bool>,
    #[cfg(feature = "utm-transcript-worker")]
    snapshot_mailbox: &'static personal_os_slint_notes_spike::photo_mailbox::Mailbox<1, 4096>,
    #[cfg(feature = "utm-transcript-worker")]
    snapshot_refresh: RefCell<personal_os_slint_notes_spike::snapshot_refresh::Refresh>,
    #[cfg(feature = "utm-transcript-worker")]
    snapshot_ticket: Cell<Option<personal_os_slint_notes_spike::snapshot_refresh::Ticket>>,
}

#[cfg(feature = "utm-transcript-worker")]
enum TranscriptCompletion {
    Loaded(usize, String),
    Failed(usize),
}

impl MessagesServiceClient {
    #[cfg(feature = "utm-transcript-worker")]
    fn request_snapshot(&self) {
        if !self.snapshot_mailbox.available() { return; }
        let Some(ticket) = self.snapshot_refresh.borrow_mut().start() else { return; };
        if self.snapshot_mailbox.submit(|out| { out[0] = 1; Some(1) }) {
            self.snapshot_ticket.set(Some(ticket));
            SNAPSHOT_WORKER.notify();
        } else {
            self.snapshot_refresh.borrow_mut().finish(ticket);
        }
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn take_snapshot(&self) -> Option<Result<String, &'static str>> {
        use personal_os_slint_notes_spike::photo_mailbox::Completion;
        let ticket = self.snapshot_ticket.get()?;
        let result = self.snapshot_mailbox.take(|data| {
            data.strip_prefix(&[1]).and_then(|data| core::str::from_utf8(data).ok()).map(String::from)
        });
        if matches!(result, Completion::Waiting) { return None; }
        self.snapshot_ticket.set(None);
        if !self.snapshot_refresh.borrow_mut().finish(ticket) { return None; }
        match result {
            Completion::Ready(Some(text)) => { self.apply_snapshot_owners(&text); Some(Ok(text)) }
            _ => Some(Err("Messages background snapshot failed")),
        }
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn apply_snapshot_owners(&self, text: &str) {
        let owners = personal_os_slint_notes_spike::transcript_queue::owners_from_snapshot(text);
        if *self.transcript_owners.borrow() != owners {
            *self.transcript_owners.borrow_mut() = owners;
            self.transcript_queue.borrow_mut().invalidate();
            *self.history_cache.borrow_mut() = core::array::from_fn(|_| None);
        }
    }
    #[cfg(feature = "utm-transcript-worker")]
    fn start_navigation(&self) {
        let Some(ticket) = self.navigation_queue.borrow_mut().start() else { return; };
        if self.navigation_mailbox.submit(|out| {
            out.copy_from_slice(&ticket.owner.to_le_bytes());
            Some(8)
        }) {
            self.navigation_ticket.set(Some(ticket));
            NAVIGATION_WORKER.notify();
        } else {
            // No second job may own this slot. Fail closed if the state is
            // unexpectedly inconsistent rather than select the wrong peer.
            self.navigation_queue.borrow_mut().finish(ticket, false);
        }
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn request_navigation(&self, index: usize) -> bool {
        let owner = self.transcript_owners.borrow().get(index).copied().unwrap_or(0);
        if !self.navigation_queue.borrow_mut().request(owner, index) { return false; }
        self.start_navigation();
        true
    }
    #[cfg(feature = "utm-transcript-worker")]
    fn request_live_receive(&self, start: bool) {
        if start { self.live_receive_start_requested.set(true); }
        self.live_receive_requested.set(true);
        self.start_live_receive();
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn start_live_receive(&self) {
        if !self.live_receive_requested.get() { return; }
        if self.pending_live_receive.borrow().is_some() { return; }
        let start = self.live_receive_start_requested.get();
        if self.live_receive_mailbox.submit(|out| { out[0] = u8::from(start); Some(1) }) {
            self.live_receive_requested.set(false);
            self.live_receive_start_requested.set(false);
            self.live_receive_inflight_start.set(start);
            LIVE_RECEIVE_WORKER.notify();
        }
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn take_live_receive(&self) -> Option<Result<(u64, String, usize), &'static str>> {
        use personal_os_slint_notes_spike::photo_mailbox::Completion;
        if self.pending_live_receive.borrow().is_none() {
            match self.live_receive_mailbox.take(|bytes| {
                personal_os_slint_notes_spike::live_receive::decode(bytes)
                    .map(|event| (event.status, event.owner, event.text.to_string()))
            }) {
                Completion::Waiting => return None,
                Completion::Failed | Completion::Ready(None) => {
                    // Retry on the next poll, rather than spinning if the
                    // service is persistently unavailable.
                    self.live_receive_requested.set(false);
                    if self.live_receive_inflight_start.replace(false) { self.live_receive_start_requested.set(true); }
                    return Some(Err("Messages receive status is unavailable"));
                }
                Completion::Ready(Some(event)) => {
                    self.live_receive_inflight_start.set(false);
                    *self.pending_live_receive.borrow_mut() = Some(event);
                }
            }
        }
        let pending = self.pending_live_receive.borrow();
        let (status, owner, _) = pending.as_ref()?;
        let index = if *status == 2 {
            personal_os_slint_notes_spike::live_receive::chat_for_owner(&*self.transcript_owners.borrow(), *owner)?
        } else if *status == 6 {
            match personal_os_slint_notes_spike::live_receive::chat_for_owner(&*self.transcript_owners.borrow(), *owner) {
                Some(index) => index,
                None => { drop(pending); self.pending_live_receive.borrow_mut().take(); return Some(Ok((0, String::new(), 0))); }
            }
        } else { 0 };
        drop(pending);
        let (status, _, text) = self.pending_live_receive.borrow_mut().take()?;
        Some(Ok((status, text, index)))
    }

    fn load(&self) -> Result<String, &'static str> {
        #[cfg(feature = "utm-transcript-worker")]
        self.snapshot_refresh.borrow_mut().invalidate();
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_LOAD, 0));
        if reply.label() != REPLY_MESSAGES_LOAD || reply.count() != 1 {
            return Err("Messages service load failed");
        }
        let length = get_mr(0) as usize;
        #[cfg(feature = "utm-transcript-worker")]
        let limit = personal_os_rust_server::vault_blob_record::MAX_PAYLOAD_SIZE;
        #[cfg(not(feature = "utm-transcript-worker"))]
        let limit = 1024;
        if length > limit {
            return Err("Messages service returned an invalid snapshot length");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Messages transfer length")?;
        if payload.len() != length {
            return Err("invalid Messages service payload");
        }
        let text = String::from_utf8(payload.into()).map_err(|_| "Messages snapshot is not UTF-8")?;
        #[cfg(feature = "utm-transcript-worker")]
        self.apply_snapshot_owners(&text);
        Ok(text)
    }

    fn set_profile(&mut self, name: &str) -> Result<(), &'static str> {
        if name.is_empty()
            || name.len() > 48
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.'))
        {
            return Err("Profile name must be 1 to 48 letters, digits, spaces, or ._-");
        }
        if !unsafe { self.transfer.as_mut() }.set_payload(name.as_bytes()) {
            return Err("Profile name exceeds transfer capacity");
        }
        set_mr(0, name.len() as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_SET_PROFILE, 1));
        if reply.label() == REPLY_MESSAGES_SET_PROFILE && reply.count() == 1 {
            Ok(())
        } else {
            Err("Messages service rejected the profile name")
        }
    }

    fn create_group(&mut self, name: &str, contact: usize) -> Result<(), &'static str> {
        if name.is_empty()
            || name.len() > 32
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.'))
        {
            return Err("Group name must be 1 to 32 letters, digits, spaces, or ._-");
        }
        if !unsafe { self.transfer.as_mut() }.set_payload(name.as_bytes()) {
            return Err("Group name exceeds transfer capacity");
        }
        set_mr(0, name.len() as u64);
        set_mr(1, contact as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_CREATE_GROUP, 2));
        if reply.label() == REPLY_MESSAGES_CREATE_GROUP && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service could not start the group invitation")
        }
    }

    fn create_address(&self) -> Result<(), &'static str> {
        self.showing_address.set(true);
        let reply =
            MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_CREATE_ADDRESS, 0));
        if reply.label() == REPLY_MESSAGES_CREATE_ADDRESS && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service could not request an address")
        }
    }

    fn address(&self) -> Result<String, &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_ADDRESS, 0));
        if reply.label() != REPLY_MESSAGES_ADDRESS || reply.count() != 1 {
            return Err("Messages service address read failed");
        }
        let length = get_mr(0) as usize;
        if length > 2048 {
            return Err("Messages service returned an invalid address length");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Messages address transfer length")?;
        if payload.len() != length {
            return Err("invalid Messages address payload");
        }
        String::from_utf8(payload.into()).map_err(|_| "Messages address is not UTF-8")
    }

    fn create_invitation(&self) -> Result<(), &'static str> {
        self.showing_address.set(false);
        let reply =
            MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_CREATE_INVITATION, 0));
        if reply.label() == REPLY_MESSAGES_CREATE_INVITATION && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service could not request an invitation")
        }
    }

    fn invitation(&self) -> Result<String, &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_INVITATION, 0));
        if reply.label() != REPLY_MESSAGES_INVITATION || reply.count() != 1 {
            return Err("Messages service invitation read failed");
        }
        let length = get_mr(0) as usize;
        if length > 2048 {
            return Err("Messages service returned an invalid invitation length");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Messages invitation transfer length")?;
        if payload.len() != length {
            return Err("invalid Messages invitation payload");
        }
        String::from_utf8(payload.into()).map_err(|_| "Messages invitation is not UTF-8")
    }

    fn join_invitation(&mut self, uri: &str) -> Result<(), &'static str> {
        if uri.is_empty() || uri.len() > 2048 || !uri.is_ascii() {
            return Err("Invitation must be ASCII and at most 2048 bytes");
        }
        if !unsafe { self.transfer.as_mut() }.set_payload(uri.as_bytes()) {
            return Err("Invitation exceeds transfer capacity");
        }
        set_mr(0, uri.len() as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_JOIN_INVITATION, 1));
        if reply.label() == REPLY_MESSAGES_JOIN_INVITATION && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service rejected the invitation request")
        }
    }

    fn join_status(&self) -> Result<(u64, u64), &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_JOIN_STATUS, 0));
        if reply.label() != REPLY_MESSAGES_JOIN_STATUS || reply.count() != 2 {
            return Err("Messages service status is unavailable");
        }
        Ok((get_mr(0), get_mr(1)))
    }

    fn send_message(&mut self, text: &str) -> Result<(), &'static str> {
        if text.is_empty() || text.len() > 2048 {
            return Err("Message must be 1 to 2048 bytes");
        }
        if !unsafe { self.transfer.as_mut() }.set_payload(text.as_bytes()) {
            return Err("Message exceeds transfer capacity");
        }
        set_mr(0, text.len() as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_SEND, 1));
        if reply.label() == REPLY_MESSAGES_SEND && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service rejected the send request")
        }
    }

    fn send_saved_photo(&mut self, source_chat: usize, photo_id: u64, name: &str) -> Result<(), &'static str> {
        if !unsafe { self.transfer.as_mut() }.set_payload(name.as_bytes()) { return Err("Invalid filename"); }
        set_mr(0, source_chat as u64);
        set_mr(1, photo_id);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_SEND_SAVED_PHOTO, 2));
        if reply.label() == REPLY_MESSAGES_SEND_SAVED_PHOTO && reply.count() == 0 {
            Ok(())
        } else {
            Err("Could not send this saved photo")
        }
    }

    fn save_to_library(&mut self, chat: usize, photo_id: u64, name: &str) -> Result<(), &'static str> {
        if !unsafe { self.transfer.as_mut() }.set_payload(name.as_bytes()) { return Err("Invalid filename"); }
        set_mr(0, chat as u64); set_mr(1, photo_id);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_LIBRARY_CLONE, 2));
        if reply.label() == REPLY_LIBRARY_CLONE && reply.count() == 1 { Ok(()) }
        else if reply.label() == 0x307 { Err("Storage full or unavailable") }
        else { Err("Could not save file") }
    }

    fn next_library_file(&self, cursor: u64) -> Result<Option<(u64, u64, usize, String)>, &'static str> {
        set_mr(0, cursor);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_LIBRARY_NEXT, 1));
        if reply.label() == 0x30d { return Ok(None); }
        if reply.label() != REPLY_LIBRARY_NEXT || reply.count() != 3 { return Err("Could not list files"); }
        let next = get_mr(0); let id = get_mr(1); let len = get_mr(2) as usize;
        let bytes = unsafe { self.transfer.as_ref() }.payload().ok_or("Invalid filename")?;
        let name = core::str::from_utf8(bytes).map_err(|_| "Invalid filename")?.to_string();
        if next <= cursor || id == 0 || !(1..=1048576).contains(&len) { return Err("Invalid file entry"); }
        Ok(Some((next, id, len, name)))
    }

    fn delete_library_file(&self, id: u64) -> Result<(), &'static str> {
        set_mr(0, id);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_LIBRARY_DELETE, 1));
        if reply.label() == REPLY_LIBRARY_DELETE && reply.count() == 0 { Ok(()) }
        else { Err("Could not delete file") }
    }

    #[cfg(feature = "utm-file-import-test")]
    fn import_test_file(&mut self, name: &str, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.is_empty() || bytes.len() > 1048576 { return Err("Test file must be 1 byte to 1 MiB"); }
        for (part, chunk) in bytes.chunks(3072).enumerate() {
            if !unsafe { self.transfer.as_mut() }.set_payload(chunk) { return Err("Import failed"); }
            set_mr(0, (part * 3072) as u64);
            let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_LIBRARY_STAGE, 1));
            if reply.label() != REPLY_LIBRARY_STAGE { return Err("Could not stage test file"); }
        }
        if !unsafe { self.transfer.as_mut() }.set_payload(name.as_bytes()) { return Err("Invalid filename"); }
        set_mr(0, bytes.len() as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_LIBRARY_IMPORT, 1));
        if reply.label() == REPLY_LIBRARY_IMPORT && reply.count() == 1 { Ok(()) }
        else if reply.label() == 0x307 { Err("Storage full or unavailable") }
        else { Err("Could not import test file") }
    }

    fn read_library_file(&self, id: u64, total: usize) -> Option<Vec<u8>> {
        if id == 0 || !(1..=1048576).contains(&total) { return None; }
        let mut bytes = Vec::with_capacity(total);
        for part in 0..total.div_ceil(3072) {
            set_mr(0, id); set_mr(1, part as u64);
            let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_LIBRARY_READ, 2));
            if reply.label() != REPLY_LIBRARY_READ || reply.count() != 1 || get_mr(0) as usize != total { return None; }
            let chunk = unsafe { self.transfer.as_ref() }.payload()?;
            if chunk.len() != (total - bytes.len()).min(3072) { return None; }
            bytes.extend_from_slice(chunk);
        }
        Some(bytes)
    }

    fn send_status(&self) -> Result<(u64, u64), &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_SEND_STATUS, 0));
        if reply.label() != REPLY_MESSAGES_SEND_STATUS || reply.count() != 2 {
            return Err("Messages send status is unavailable");
        }
        Ok((get_mr(0), get_mr(1)))
    }

    fn receive_next(&mut self) -> Result<(), &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_RECEIVE, 0));
        if reply.label() == REPLY_MESSAGES_RECEIVE && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service rejected the receive request")
        }
    }

    fn receive_status(&self) -> Result<(u64, String, usize), &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_RECEIVE_STATUS, 0));
        if reply.label() != REPLY_MESSAGES_RECEIVE_STATUS || reply.count() != 3 {
            return Err("Messages receive status is unavailable");
        }
        let status = get_mr(0);
        let length = get_mr(1) as usize;
        let chat_index = get_mr(2) as usize;
        if length > 2048 {
            return Err("Messages returned an oversized incoming text");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("Messages returned an invalid receive transfer")?;
        if payload.len() != length {
            return Err("Messages receive transfer length did not match");
        }
        Ok((
            status,
            String::from_utf8(payload.into()).map_err(|_| "Incoming message is not UTF-8")?,
            chat_index,
        ))
    }

    fn acknowledge_received(&mut self) -> Result<(), &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_ACK, 0));
        if reply.label() == REPLY_MESSAGES_ACK && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service rejected the acknowledge request")
        }
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn acknowledge_worker_received(&mut self, index: usize) -> Result<(), &'static str> {
        let owner = self.transcript_owners.borrow().get(index).copied().filter(|&owner| owner != 0)
            .ok_or("Unknown incoming chat")?;
        set_mr(0, owner);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_ACK, 1));
        if reply.label() == REPLY_MESSAGES_ACK && reply.count() == 0 { Ok(()) }
        else { Err("Messages service rejected the acknowledge request") }
    }

    fn acknowledge_status(&self) -> Result<u64, &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_ACK_STATUS, 0));
        if reply.label() != REPLY_MESSAGES_ACK_STATUS || reply.count() != 1 {
            return Err("Messages acknowledge status is unavailable");
        }
        Ok(get_mr(0))
    }

    fn select_chat(&self, index: u64) -> Result<(), &'static str> {
        set_mr(0, index);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_SELECT_CHAT, 1));
        if reply.label() == REPLY_MESSAGES_SELECT_CHAT && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service rejected the chat selection")
        }
    }

    #[cfg(not(feature = "simplex-agent-ui"))]
    fn transcript(&self, index: u64) -> Result<String, &'static str> {
        set_mr(0, index);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_TRANSCRIPT, 1));
        if reply.label() != REPLY_MESSAGES_TRANSCRIPT || reply.count() != 1 {
            return Err("Messages service transcript read failed");
        }
        let length = get_mr(0) as usize;
        if length > 700 {
            return Err("Messages service returned an invalid transcript length");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Messages transcript transfer length")?;
        if payload.len() != length {
            return Err("invalid Messages transcript payload");
        }
        String::from_utf8(payload.into()).map_err(|_| "Messages transcript is not UTF-8")
    }

    #[cfg(all(feature = "simplex-agent-ui", not(feature = "utm-transcript-worker")))]
    fn transcript(&self, index: u64) -> Result<String, &'static str> {
        let index = usize::try_from(index).ok().filter(|&index| index < CHAT_SLOTS)
            .ok_or("Unknown Messages chat")?;
        set_mr(0, index as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_HISTORY_INFO, 1));
        if reply.label() != REPLY_MESSAGES_HISTORY_INFO || reply.count() != 2 {
            return Err("Messages history is unavailable");
        }
        let owner = get_mr(0);
        let revision = get_mr(1);
        let mut cache = self.history_cache.borrow_mut();
        if let Some((cached_owner, cached_revision, text)) = &cache[index] {
            if *cached_owner == owner && *cached_revision == revision {
                return Ok(text.clone());
            }
        }
        let mut text = String::new();
        let mut cursor = 0;
        loop {
            set_mr(0, owner);
            set_mr(1, revision);
            set_mr(2, cursor);
            let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_HISTORY_PAGE, 3));
            if reply.label() != REPLY_MESSAGES_HISTORY_PAGE || reply.count() != 2 {
                return Err("Messages history changed during read");
            }
            let length = get_mr(0) as usize;
            let next = get_mr(1);
            let payload = (unsafe { self.transfer.as_ref() }).payload()
                .filter(|bytes| bytes.len() == length)
                .ok_or("Invalid Messages history payload")?;
            text.push_str(core::str::from_utf8(payload).map_err(|_| "Invalid Messages history text")?);
            if next == 0 {
                break;
            }
            if next <= cursor {
                return Err("Invalid Messages history cursor");
            }
            cursor = next;
        }
        debug_println!("SLINT SIMPLEX HISTORY_LOADED {} {}", index, text.len());
        cache[index] = Some((owner, revision, text.clone()));
        Ok(text)
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn transcript(&self, index: u64) -> Result<String, &'static str> {
        let index = usize::try_from(index).ok().filter(|&i| i < CHAT_SLOTS).ok_or("Unknown Messages chat")?;
        self.transcript_queue.borrow_mut().request(index);
        self.start_transcript();
        self.history_cache.borrow()[index].as_ref().map(|(_, _, text)| text.clone())
            .ok_or("Messages history is loading")
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn history_changed(&self, index: usize) {
        let changed = self.transcript_queue.borrow_mut().changed(index);
        if changed {
            self.history_cache.borrow_mut()[index] = None;
            self.start_transcript();
        }
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn start_transcript(&self) {
        if self.transcript_ticket.get().is_some() || !self.transcript_mailbox.available() { return; }
        let Some(ticket) = self.transcript_queue.borrow_mut().start() else { return; };
        let request = personal_os_slint_notes_spike::transcript_ipc::request(ticket.index()).unwrap();
        if self.transcript_mailbox.submit(|out| { out[..request.len()].copy_from_slice(&request); Some(request.len()) }) {
            self.transcript_ticket.set(Some(ticket));
            TRANSCRIPT_WORKER.notify();
        } else {
            self.transcript_queue.borrow_mut().finish(ticket);
            self.transcript_queue.borrow_mut().request(ticket.index());
        }
    }

    #[cfg(feature = "utm-transcript-worker")]
    fn finish_transcript(&self) -> Option<TranscriptCompletion> {
        use personal_os_slint_notes_spike::photo_mailbox::Completion;
        let ticket = self.transcript_ticket.get()?;
        let result = self.transcript_mailbox.take(|bytes| {
            personal_os_slint_notes_spike::transcript_ipc::parse_response(bytes)
                .map(|(owner, revision, text)| (owner, revision, text.to_string()))
        });
        if matches!(result, Completion::Waiting) { return None; }
        self.transcript_ticket.set(None);
        let current = self.transcript_queue.borrow_mut().finish(ticket);
        let result = match result {
            Completion::Ready(Some((owner, revision, text))) if current && self.transcript_owners.borrow()[ticket.index()] == owner => {
                #[cfg(feature = "utm-perf-trace")]
                debug_println!("PERF|TRANSCRIPT_RESULT|{}|{}|{}", ticket.index(), text.len(), perf_time_us());
                self.history_cache.borrow_mut()[ticket.index()] = Some((owner, revision, text.clone()));
                Some(TranscriptCompletion::Loaded(ticket.index(), text))
            }
            Completion::Failed | Completion::Ready(None) if current => {
                Some(TranscriptCompletion::Failed(ticket.index()))
            }
            Completion::Ready(Some(_)) if current => {
                Some(TranscriptCompletion::Failed(ticket.index()))
            }
            _ => {
                #[cfg(feature = "utm-perf-trace")]
                debug_println!("PERF|TRANSCRIPT_REJECT|{}|{}", ticket.index(), perf_time_us());
                None
            },
        };
        self.start_transcript();
        result
    }

    fn image_meta(&self) -> Result<(u64, usize), &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_IMAGE, 0));
        if reply.label() != REPLY_MESSAGES_IMAGE || reply.count() != 2 {
            return Err("Messages image is unavailable");
        }
        Ok((get_mr(0), get_mr(1) as usize))
    }

    fn image_chunk(&self, offset: usize) -> Result<Vec<u8>, &'static str> {
        set_mr(0, offset as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_IMAGE_CHUNK, 1));
        if reply.label() != REPLY_MESSAGES_IMAGE_CHUNK || reply.count() != 1 {
            return Err("Messages image chunk failed");
        }
        let length = get_mr(0) as usize;
        if length > 3000 {
            return Err("Messages image chunk is too large");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Messages image transfer")?;
        if payload.len() != length {
            return Err("Messages image chunk length did not match");
        }
        Ok(payload.to_vec())
    }

    fn read_attachment_part(&self, chat: u64, id: u64, part: usize) -> Option<usize> {
        set_mr(0, chat);
        set_mr(1, id);
        set_mr(2, part as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_IMAGE_CHUNK, 3));
        if reply.label() != REPLY_MESSAGES_IMAGE_CHUNK || reply.count() != 1 {
            return None;
        }
        Some(get_mr(0) as usize)
    }

    fn message_attachment(&self, chat: u64, id: u64) -> Option<Vec<u8>> {
        const CHUNK: usize = 3072;
        const MAX_SIZE: usize = 1024 * 1024;
        const MAX_PARTS: usize = MAX_SIZE.div_ceil(CHUNK);
        if id == 0 { return None; }
        let mut bytes = Vec::new();
        for part in 0..MAX_PARTS {
            let total = self.read_attachment_part(chat, id, part)?;
            let payload = unsafe { self.transfer.as_ref() }.payload()?;
            if !(1..=MAX_SIZE).contains(&total) || bytes.len() >= total {
                return None;
            }
            if payload.len() != CHUNK.min(total - bytes.len()) {
                return None;
            }
            bytes.extend_from_slice(payload);
            if bytes.len() == total {
                return Some(bytes);
            }
        }
        None
    }

    fn group_invite(&self) -> Result<(u64, String), &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_GROUP, 0));
        if reply.label() != REPLY_MESSAGES_GROUP || reply.count() != 2 {
            return Err("Messages group invitation is unavailable");
        }
        let length = get_mr(1) as usize;
        if length == 0 {
            return Ok((get_mr(0), String::new()));
        }
        if length > 2048 {
            return Err("Messages group invitation is too large");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Messages group invitation transfer")?;
        if payload.len() != length {
            return Err("Messages group invitation length did not match");
        }
        let uri = String::from_utf8(payload.to_vec())
            .map_err(|_| "Messages group invitation is not UTF-8")?;
        Ok((get_mr(0), uri))
    }

    fn delete_chat(&mut self, chat: u64) -> Result<(), &'static str> {
        set_mr(0, chat);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_DELETE, 1));
        if reply.label() == REPLY_MESSAGES_DELETE && reply.count() == 0 {
            Ok(())
        } else {
            Err("Messages service rejected the delete request")
        }
    }

    fn archive_chat(&mut self, chat: u64, archived: bool) -> Result<u16, &'static str> {
        set_mr(0, chat);
        set_mr(1, u64::from(archived));
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_ARCHIVE, 2));
        if reply.label() != REPLY_MESSAGES_ARCHIVE || reply.count() != 1 {
            return Err("Messages service rejected the archive request");
        }
        Ok(get_mr(0) as u16)
    }

    fn archived_mask(&self) -> Result<u16, &'static str> {
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_ARCHIVED_MASK, 0));
        if reply.label() != REPLY_MESSAGES_ARCHIVED_MASK || reply.count() != 1 {
            return Err("Messages archive mask is unavailable");
        }
        Ok(get_mr(0) as u16)
    }

    fn peer_photo_meta(&self, index: u64) -> Result<(u64, usize), &'static str> {
        set_mr(0, index);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_PEER_PHOTO, 1));
        if reply.label() != REPLY_MESSAGES_PEER_PHOTO || reply.count() != 2 {
            return Err("Messages peer photo is unavailable");
        }
        Ok((get_mr(0), get_mr(1) as usize))
    }

    fn peer_photo_chunk(&self, index: u64, offset: usize) -> Result<Vec<u8>, &'static str> {
        set_mr(0, index);
        set_mr(1, offset as u64);
        let reply = MESSAGES_SERVICE.pp_call(MessageInfo::new(REQUEST_MESSAGES_PEER_PHOTO_CHUNK, 2));
        if reply.label() != REPLY_MESSAGES_PEER_PHOTO_CHUNK || reply.count() != 1 {
            return Err("Messages peer photo chunk failed");
        }
        let length = get_mr(0) as usize;
        if length > 3000 {
            return Err("Messages peer photo chunk is too large");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Messages peer photo transfer")?;
        if payload.len() != length {
            return Err("Messages peer photo chunk length did not match");
        }
        Ok(payload.to_vec())
    }
}

fn present_message_image(ui: &NotesWindow, service: &MessagesServiceClient) {
    if !ui.get_message_view() {
        ui.set_message_image_visible(false);
        ui.set_message_incoming_image_visible(false);
        return;
    }
    // The + button always sends this one JPEG, so paint it on every outgoing
    // Photo line even after a reboot clears the messages image buffer.
    if transcript_has_outgoing_photo(ui.get_message_transcript().as_str()) {
        if let Some(jpeg) = bundled_photo_jpeg() {
            if let Some(image) = decode_jpeg_preview(&jpeg) {
                ui.set_message_image(image);
                ui.set_message_image_visible(true);
                debug_println!("SLINT SIMPLEX PHOTO {}", jpeg.len());
            }
        }
    } else {
        ui.set_message_image_visible(false);
    }
    // Clear the previous chat's preview if this one cannot be loaded.
    ui.set_message_incoming_image_visible(false);
    match service.image_meta() {
        Ok((chat, length))
            if ui.get_message_chat_index() == chat as i32 && (4..12289).contains(&length) =>
        {
            let mut bytes = Vec::with_capacity(length);
            let mut offset = 0;
            let mut failed = false;
            while offset < length {
                match service.image_chunk(offset) {
                    Ok(chunk) if !chunk.is_empty() => {
                        offset += chunk.len();
                        bytes.extend_from_slice(&chunk);
                    }
                    _ => {
                        failed = true;
                        break;
                    }
                }
            }
            if !failed && bytes.len() == length {
                if let Some(image) = decode_jpeg_preview(&bytes) {
                    ui.set_message_incoming_image(image);
                    ui.set_message_incoming_image_visible(true);
                    debug_println!("SLINT SIMPLEX INCOMING PHOTO {}", bytes.len());
                } else {
                    debug_println!("SLINT SIMPLEX INCOMING PHOTO DECODE FAIL");
                }
            } else {
                debug_println!("SLINT SIMPLEX INCOMING PHOTO READ FAIL {}", bytes.len());
            }
        }
        Ok(_) => {},
        Err(error) => debug_println!("SLINT SIMPLEX INCOMING PHOTO META {}", error),
    }
}

fn attachment_display(bytes: Vec<u8>) -> String {
    use core::fmt::Write;

    match String::from_utf8(bytes) {
        Ok(text) if text.chars().all(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t')) => text,
        result => {
            let bytes = match result {
                Ok(text) => text.into_bytes(),
                Err(error) => error.into_bytes(),
            };
            let mut display = String::new();
            for chunk in bytes.chunks(16) {
                for byte in chunk {
                    let _ = write!(display, "{byte:02x} ");
                }
                display.push('\n');
            }
            display
        }
    }
}

fn file_bubble(text: &str) -> Option<(&str, &str)> {
    let (name, body) = text.split_once(": ")?;
    if name.is_empty()
        || name.len() > 40
        || name.contains(' ')
        || !name.contains('.')
        || name.ends_with('.')
        || body.is_empty()
    {
        return None;
    }
    Some((name, body))
}

fn transcript_has_outgoing_photo(transcript: &str) -> bool {
    transcript.lines().any(|line| {
        if !line.starts_with("NexatrodeOS: ") {
            return false;
        }
        let body = line.split_once(": ").map(|(_, body)| body).unwrap_or(line);
        let body = split_message_time(body).1.trim_end_matches(" \u{2713}");
        let body = strip_shared_msg_id(body).1;
        let (body, _) = strip_reaction_suffix(body);
        body == "Photo"
    })
}

fn bundled_photo_jpeg() -> Option<Vec<u8>> {
    const URL: &str = include_str!("../assets/simplex-attach.url");
    let encoded = URL.split_once(',')?.1.trim();
    let mut out = Vec::with_capacity(encoded.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &byte in encoded.as_bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1u32 << bits) - 1;
        }
    }
    if out.first().copied() != Some(0xff) || out.get(1).copied() != Some(0xd8) {
        return None;
    }
    Some(out)
}

fn present_group_invite(ui: &NotesWindow, service: &MessagesServiceClient) {
    let Ok((chat, uri)) = service.group_invite() else {
        ui.set_message_group_invite(false);
        return;
    };
    if uri.is_empty() || !ui.get_message_view() || ui.get_message_chat_index() != chat as i32 {
        ui.set_message_group_invite(false);
        return;
    }
    ui.set_message_group_uri(uri.into());
    ui.set_message_group_invite(true);
}

fn decode_jpeg_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let options = zune_jpeg::zune_core::options::DecoderOptions::default()
        .jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::RGBA)
        .set_max_width(2560)
        .set_max_height(2560);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(
        zune_jpeg::zune_core::bytestream::ZCursor::new(bytes),
        options,
    );
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    let width = usize::from(info.width);
    let height = usize::from(info.height);
    if width == 0 || height == 0 || width.checked_mul(height)? > 2_500_000 {
        return None;
    }
    let pixels = decoder.decode().ok()?;
    if pixels.len() != width * height * 4 { return None; }
    let longest = width.max(height);
    if longest <= 480 { return Some((width, height, pixels)); }
    let out_width = (width * 480 / longest).max(1);
    let out_height = (height * 480 / longest).max(1);
    let mut scaled = vec![0u8; out_width * out_height * 4];
    for y in 0..out_height {
        for x in 0..out_width {
            let from = ((y * height / out_height) * width + x * width / out_width) * 4;
            let to = (y * out_width + x) * 4;
            scaled[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
        }
    }
    Some((out_width, out_height, scaled))
}

fn decode_jpeg_preview(bytes: &[u8]) -> Option<slint::Image> {
    let (width, height, pixels) = decode_jpeg_rgba(bytes)?;
    let mut buffer =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width as u32, height as u32);
    buffer.make_mut_bytes().copy_from_slice(&pixels);
    Some(slint::Image::from_rgba8(buffer))
}

fn decode_jpeg_thumbnail(bytes: &[u8]) -> Option<slint::Image> {
    let (width, height, pixels) = decode_jpeg_rgba(bytes)?;
    let scale = width.max(height).div_ceil(64).max(1);
    let w = width.div_ceil(scale);
    let h = height.div_ceil(scale);
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w as u32, h as u32);
    let output = buffer.make_mut_bytes();
    for y in 0..h {
        for x in 0..w {
            let source = ((y * scale).min(height - 1) * width + (x * scale).min(width - 1)) * 4;
            let dest = (y * w + x) * 4;
            output[dest..dest + 4].copy_from_slice(&pixels[source..source + 4]);
        }
    }
    Some(slint::Image::from_rgba8(buffer))
}

fn invitation_qr_image(uri: &str) -> Option<slint::Image> {
    personal_os_slint_notes_spike::qr::qr_image(uri)
}

fn present_invitation_qr(ui: &NotesWindow, uri: &str) {
    if let Some(image) = invitation_qr_image(uri) {
        ui.set_message_invitation_qr(image);
        ui.set_message_invitation_qr_visible(true);
        debug_println!("SLINT SIMPLEX INVITATION QR READY");
    } else {
        ui.set_message_invitation_qr_visible(false);
        debug_println!("SLINT SIMPLEX INVITATION QR FAIL");
    }
}

/// Receive tab: QR of the primary address. Payment URI stays unwired.
fn present_wallet_address_qr(ui: &NotesWindow, addr: &str) {
    if addr.len() < 95 {
        return;
    }
    if let Some(image) = invitation_qr_image(addr) {
        ui.set_wallet_address_qr(image);
        ui.set_wallet_address_qr_visible(true);
        debug_println!("SLINT WALLET QR READY");
    } else {
        ui.set_wallet_address_qr_visible(false);
        debug_println!("SLINT WALLET QR FAIL");
    }
}

/// Wire framing so the agent can attach a SimpleX `quote` field.
/// Layout: `\x1eQ\x1f{0|1}\x1f{quote}\x1f{text}` where 1 means quoting own message.
const QUOTE_MARK: &str = "\u{001e}Q\u{001f}";
const QUOTE_SEP: &str = "\u{001f}";

fn encode_quoted_send(quote_mine: bool, quote: &str, text: &str) -> String {
    // Prefer the leaf body when the selected bubble was itself a quote.
    let quote = split_quote_display(quote).1;
    let mut clean_quote = quote.replace(QUOTE_SEP, " ").replace('\n', " ");
    if clean_quote.len() > 120 {
        clean_quote.truncate(120);
        while !clean_quote.is_char_boundary(clean_quote.len()) {
            clean_quote.pop();
        }
    }
    let mut out =
        String::with_capacity(QUOTE_MARK.len() + clean_quote.len() + text.len() + 4);
    out.push_str(QUOTE_MARK);
    out.push(if quote_mine { '1' } else { '0' });
    out.push_str(QUOTE_SEP);
    out.push_str(&clean_quote);
    out.push_str(QUOTE_SEP);
    out.push_str(text);
    out
}

impl CalendarServiceClient {
    fn load(&self) -> Result<String, &'static str> {
        let reply = CALENDAR_SERVICE.pp_call(MessageInfo::new(REQUEST_CALENDAR_LOAD, 0));
        if reply.label() != REPLY_CALENDAR_LOAD || reply.count() != 1 {
            return Err("Calendar service load failed");
        }
        let length = get_mr(0) as usize;
        if length > 21 {
            return Err("Calendar service returned an invalid title length");
        }
        let payload = unsafe { self.transfer.as_ref() }
            .payload()
            .ok_or("invalid Calendar transfer length")?;
        if payload.len() != length {
            return Err("invalid Calendar service payload");
        }
        String::from_utf8(payload.into()).map_err(|_| "Reminder title is not UTF-8")
    }

    fn save_and_verify(&mut self, title: &str) -> Result<(), &'static str> {
        if title.is_empty() || title.len() > 21 {
            return Err("Reminder title exceeds its limit");
        }
        if !unsafe { self.transfer.as_mut() }.set_payload(title.as_bytes()) {
            return Err("Reminder title exceeds the transfer page");
        }
        set_mr(0, title.len() as u64);
        let reply = CALENDAR_SERVICE.pp_call(MessageInfo::new(REQUEST_CALENDAR_SAVE, 1));
        if reply.label() != REPLY_CALENDAR_SAVE || reply.count() != 0 {
            return Err("Calendar service save failed");
        }
        if self.load()?.as_bytes() != title.as_bytes() {
            return Err("Reminder readback did not match");
        }
        Ok(())
    }
}

enum TextField {
    Message,
    Join,
}

fn active_text_field(ui: &NotesWindow) -> Option<TextField> {
    if ui.get_page() != 6 {
        return None;
    }
    if ui.get_message_join_view() {
        return Some(TextField::Join);
    }
    if ui.get_message_view() {
        return Some(TextField::Message);
    }
    None
}

fn wallet_send_payload(ui: &NotesWindow) -> Result<String, &'static str> {
    let dest = ui.get_wallet_send_dest().to_string();
    if dest.is_empty() {
        return Err("Enter an address");
    }
    if ui.get_wallet_send_max() {
        return Ok(alloc::format!("DEST {dest} MAX"));
    }
    match parse_xmr_amount_to_atomic(&ui.get_wallet_send_amount()).filter(|v| *v > 0) {
        Some(atomic) => Ok(alloc::format!("DEST {dest} AMOUNT {atomic}")),
        None => Err("Enter an amount"),
    }
}

fn parse_prepare_fee(raw: &str) -> Option<u64> {
    let rest = raw.split("FEE ").nth(1)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn wallet_preview_text(raw: &str, fee: u64, send_max: bool, ready: bool) -> String {
    let action = if ready { "tap Send" } else { "sending unavailable" };
    let amount = raw.split(" AMOUNT ").nth(1).and_then(|rest| {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u64>().ok()
    });
    if let (true, Some(amount)) = (send_max, amount) {
        alloc::format!("Send {} XMR · fee {} XMR · {action}", format_xmr_atomic(amount), format_xmr_atomic(fee))
    } else {
        alloc::format!("Fee {} XMR · {action}", format_xmr_atomic(fee))
    }
}

fn preview_wallet_fee(ui: &NotesWindow, wallet: &mut WalletServiceClient) {
    ui.set_edit_target(0);
    ui.set_wallet_test_preview(false);
    let payload = match wallet_send_payload(ui) {
        Ok(payload) => payload,
        Err(msg) => {
            ui.set_wallet_fee_ready(false);
            ui.set_wallet_send_status(msg.into());
            debug_println!("SLINT WALLET PREVIEW {msg}");
            return;
        }
    };
    let cur = ui.get_wallet_status();
    let mid_flight = cur.contains("RECV...")
        || cur.contains("CONNECTING")
        || cur.contains("WAIT NET")
        || cur.contains("QUEUED")
        || (cur.contains("TIP ") && cur.contains("SCAN"))
        || cur.contains("FETCH");
    if mid_flight {
        ui.set_wallet_fee_ready(false);
        ui.set_wallet_send_status("Still syncing…".into());
        debug_println!("SLINT WALLET PREVIEW Still syncing");
    } else {
        let _ = unsafe { wallet.transfer.as_mut() }.set_payload(payload.as_bytes());
        let prepared = wallet.prepare_send();
        if prepared.contains("PREPARING DECOYS") {
            ui.set_wallet_status(prepared.clone().into());
            WALLET_SERVICE.notify();
        }
        if let Some(fee) = parse_prepare_fee(&prepared) {
            let ready = !prepared.contains("SEND NOT READY");
            ui.set_wallet_test_preview(prepared.contains("TEST SEND READY"));
            ui.set_wallet_fee_ready(ready);
            ui.set_wallet_send_status(wallet_preview_text(&prepared, fee, ui.get_wallet_send_max(), ready).into());
            debug_println!("SLINT WALLET PREVIEW FEE {}", fee);
        } else {
            ui.set_wallet_fee_ready(false);
            ui.set_wallet_send_status(humanize_wallet_status(&prepared).into());
            debug_println!("SLINT WALLET PREVIEW {}", prepared);
        }
        apply_wallet_face(ui, &prepared);
    }
}

/// Hardware and on-screen keys for the Send address (11) and amount (12).
fn apply_wallet_key(ui: &NotesWindow, key: &str) -> bool {
    if ui.get_page() != 16 { return false; }
    let target = ui.get_edit_target();
    if target == 16 && ui.get_wallet_tab() == 3 && ui.get_wallet_setup() == 0 {
        if key == "ENTER" || key == "\n" { ui.set_edit_target(0); return true; }
        let mut value = ui.get_wallet_node().to_string();
        match key {
            "DELETE" | "\u{8}" => { value.pop(); },
            "CLEAR" => value.clear(),
            k if k.len() == 1 && value.len() < 127
                && (k.as_bytes()[0].is_ascii_alphanumeric() || matches!(k, "." | "-" | ":" | "/")) => value.push_str(k),
            _ => (),
        }
        ui.set_wallet_node(value.into());
        return true;
    }
    if target == 14 || target == 15 {
        if key == "ENTER" || key == "\n" { ui.set_edit_target(0); return true; }
        let mut value = if target == 14 { ui.get_wallet_words().to_string() } else { ui.get_wallet_height().to_string() };
        match key {
            "DELETE" | "\u{8}" => { value.pop(); },
            "CLEAR" => value.clear(),
            "SPACE" | " " if target == 14 && value.len() < 1024 => value.push(' '),
            k if target == 14 && k.len() == 1 && k.as_bytes()[0].is_ascii_alphabetic() && value.len() < 1024 => value.push_str(&k.to_ascii_lowercase()),
            k if target == 15 && k.len() == 1 && k.as_bytes()[0].is_ascii_digit() && value.len() < 20 => value.push_str(k),
            _ => return true,
        }
        if target == 14 { ui.set_wallet_words(value.into()); } else { ui.set_wallet_height(value.into()); }
        return true;
    }
    if ui.get_wallet_tab() != 2 || ui.get_wallet_setup() != 0 { return false; }
    if target != 11 && target != 12 {
        return false;
    }
    if key == "ENTER" || key == "\n" {
        ui.set_edit_target(0);
        return true;
    }
    if key == "SPACE" || key == "SHIFT" || key == "?123" || key == "ABC" {
        return true;
    }
    let mut value = if target == 11 {
        ui.get_wallet_send_dest().to_string()
    } else {
        let cur = ui.get_wallet_send_amount().to_string();
        if cur == "0.000000000001" {
            String::new()
        } else {
            cur
        }
    };
    match key {
        "DELETE" | "\u{8}" => {
            value.pop();
        }
        "CLEAR" => value.clear(),
        "." if target == 12 => {
            if !value.contains('.') && value.len() < 20 {
                value.push('.');
            }
        }
        k if target == 12
            && k.len() == 1
            && k.as_bytes()[0].is_ascii_digit()
            && value.len() < 20 =>
        {
            value.push_str(k);
        }
        k if target == 11
            && k.len() == 1
            && k.as_bytes()[0].is_ascii_alphanumeric()
            && value.len() < 106 =>
        {
            value.push_str(k);
        }
        _ => return false,
    }
    ui.set_wallet_fee_ready(false);
    ui.set_wallet_test_preview(false);
    ui.set_wallet_send_status("".into());
    if target == 11 {
        ui.set_wallet_send_dest(value.into());
    } else {
        ui.set_wallet_send_max(false);
        ui.set_wallet_send_amount(value.into());
    }
    true
}

fn apply_hardware_key(ui: &NotesWindow, code: u64, shift: bool) -> bool {
    let Some(text) = hardware_character(code, shift) else {
        return false;
    };
    #[cfg(feature = "utm-wrapped-vault-experiment")]
    if ui.get_page() == 1 && ui.get_note_editing() && ui.get_edit_target() == 1 {
        ui.invoke_key_tapped(text.into());
        return true;
    }
    if ui.get_page() == 6 && ui.get_edit_target() == 17 {
        ui.invoke_key_tapped(text.into());
        return true;
    }
    if apply_wallet_key(ui, text) {
        return true;
    }
    match active_text_field(ui) {
        Some(TextField::Message) => insert_message(ui, text),
        Some(TextField::Join) => insert_join(ui, text),
        None => false,
    }
}

fn logical_screen() -> (f32, f32) {
    (WIDTH as f32 / SCALE, HEIGHT as f32 / SCALE)
}

/// One-level back — same policy as the chrome HOME/LIST/CHATS/BACK button.
fn shell_navigate_back(ui: &NotesWindow, drafts: Option<&RefCell<[String; CHAT_SLOTS]>>) -> bool {
    if ui.get_device_lock_screen() { return false; }
    let page = ui.get_page();
    if page == 0 || page == 14 {
        return false;
    }
    if page == 1 && ui.get_note_editing() {
        ui.set_note_editing(false);
        ui.set_edit_target(0);
        return true;
    }
    if page == 2 && ui.get_contact_detail() {
        ui.set_contact_detail(false);
        ui.set_edit_target(0);
        return true;
    }
    if page == 6 && ui.get_message_view() {
        if let Some(drafts) = drafts {
            let index = ui.get_message_chat_index().max(0) as usize;
            drafts.borrow_mut()[index.min(CHAT_SLOTS - 1)] = ui.get_message_body().to_string();
        }
        ui.set_message_reply_to("".into());
        ui.set_message_view(false);
        ui.set_edit_target(0);
        return true;
    }
    if page == 6 && ui.get_message_join_view() {
        ui.set_message_join_view(false);
        ui.set_message_join_uri("".into());
        ui.set_edit_target(0);
        return true;
    }
    if page == 6 && ui.get_message_invitation_view() {
        ui.set_message_invitation_view(false);
        ui.set_message_invitation_qr_visible(false);
        ui.set_edit_target(0);
        return true;
    }
    if page == 15 {
        ui.set_page(8);
        ui.set_edit_target(0);
        ui.set_settings_selected(-1);
        return true;
    }
    if page == 6 {
        ui.set_message_search("".into());
    }
    ui.set_page(0);
    ui.set_edit_target(0);
    true
}

fn shell_navigate_home(ui: &NotesWindow, drafts: Option<&RefCell<[String; CHAT_SLOTS]>>) -> bool {
    if ui.get_device_lock_screen() { return false; }
    if ui.get_page() == 0 {
        return false;
    }
    if ui.get_page() == 6 && ui.get_message_view() {
        if let Some(drafts) = drafts {
            let index = ui.get_message_chat_index().max(0) as usize;
            drafts.borrow_mut()[index.min(CHAT_SLOTS - 1)] = ui.get_message_body().to_string();
        }
        ui.set_message_reply_to("".into());
        ui.set_message_view(false);
    }
    ui.set_message_join_view(false);
    ui.set_message_invitation_view(false);
    ui.set_message_invitation_qr_visible(false);
    ui.set_message_search("".into());
    ui.set_note_editing(false);
    ui.set_contact_detail(false);
    ui.set_page(0);
    ui.set_edit_target(0);
    true
}

/// Soft switcher: Tools hub (notes/clock/files/…) until a real multi-PD switcher exists.
fn shell_navigate_recents(ui: &NotesWindow) -> bool {
    if ui.get_device_lock_screen() { return false; }
    if ui.get_page() == 10 {
        return false;
    }
    ui.set_message_view(false);
    ui.set_message_join_view(false);
    ui.set_message_invitation_view(false);
    ui.set_note_editing(false);
    ui.set_contact_detail(false);
    ui.set_edit_target(0);
    ui.set_page(10);
    true
}

fn remember_app_page(page: i32, last: &mut i32) {
    if page != 0 && page != 14 {
        *last = page;
    }
}

fn insert_message(ui: &NotesWindow, text: &str) -> bool {
    let mut body = ui.get_message_body().to_string();
    if text == "\u{8}" {
        body.pop();
    } else if text == "\n" {
        if body.len() >= 128 {
            return false;
        }
        body.push('\n');
    } else if body.len() + text.len() > 128
        || !text.bytes().all(|byte| byte == b' ' || byte.is_ascii_graphic())
    {
        return false;
    } else {
        body.push_str(text);
    }
    ui.set_message_body(body.into());
    true
}

fn insert_join(ui: &NotesWindow, text: &str) -> bool {
    if text == "\n" || text == " " {
        return false;
    }
    let mut uri = ui.get_message_join_uri().to_string();
    if text == "\u{8}" {
        uri.pop();
    } else if uri.len() + text.len() > 2048 || !text.bytes().all(|byte| byte.is_ascii_graphic()) {
        return false;
    } else {
        uri.push_str(text);
    }
    ui.set_message_join_uri(uri.into());
    true
}

fn copy_active_field(ui: &NotesWindow) {
    let text = if ui.get_page() == 6 && ui.get_message_invitation_view() && !ui.get_message_view() {
        ui.get_message_invitation_uri().to_string()
    } else {
        match active_text_field(ui) {
            Some(TextField::Message) => {
                let draft = ui.get_message_body().to_string();
                if draft.is_empty() {
                    ui.get_message_transcript().to_string()
                } else {
                    draft
                }
            }
            Some(TextField::Join) => ui.get_message_join_uri().to_string(),
            None => return,
        }
    };
    if text.is_empty() {
        return;
    }
    let escaped = escape_copy(&text);
    debug_println!("NEXATRODE_COPY|BEGIN|{}", escaped.len());
    let mut start = 0;
    while start < escaped.len() {
        let mut end = (start + 80).min(escaped.len());
        while end > start && !escaped.is_char_boundary(end) {
            end -= 1;
        }
        debug_println!("NEXATRODE_COPY|DATA|{}", &escaped[start..end]);
        start = end;
    }
    debug_println!("NEXATRODE_COPY|END");
}

fn log_simplex_link(uri: &str) {
    if uri.is_empty() {
        return;
    }
    let https = if let Some(path) = uri.strip_prefix("simplex:/") {
        alloc::format!("https://simplex.chat/{path}")
    } else {
        uri.to_string()
    };
    debug_println!("SIMPLEX_LINK BEGIN {}", https.len());
    let mut index = 0;
    let mut part = 0;
    while index < https.len() {
        let mut end = (index + 64).min(https.len());
        while end > index && !https.is_char_boundary(end) {
            end -= 1;
        }
        debug_println!("SIMPLEX_LINK {} {}", part, &https[index..end]);
        index = end;
        part += 1;
    }
    debug_println!("SIMPLEX_LINK END");
}

fn escape_copy(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => {}
            other if other.is_ascii() => escaped.push(other),
            _ => {}
        }
    }
    escaped
}

/// Map wallet PD transfer strings into standard (non-techno) NexaWal face fields.
/// Does not overwrite `wallet-status` (tip/sync state machine keeps the raw string).
fn apply_wallet_face(ui: &NotesWindow, raw: &str) {
    let human = humanize_wallet_status(raw);
    let waiting = human.contains("Waiting for deposit");
    // A tap sets "Address copied", then this same frame republishes the
    // scan line. Leave the confirmation up unless a send or a node error
    // has something newer to say.
    let keep_copied = ui.get_wallet_sync().as_str() == "Address copied"
        && matches!(
            human.as_str(),
            "Wallet synced"
                | "Syncing…"
                | "Connecting…"
                | "Opening wallet…"
                | "Still syncing…"
                | "Waiting for deposit…"
        );
    if !keep_copied && ui.get_wallet_sync().as_str() != human {
        debug_println!("SLINT FACE SYNC {}", human);
        ui.set_wallet_sync(human.into());
    }
    if waiting {
        let hist = ui.get_wallet_history().to_string();
        if hist.starts_with("No transactions") || hist.is_empty() {
            ui.set_wallet_history("Waiting for a deposit…".into());
            apply_wallet_tx_rows(ui, "");
        }
    } else if ui.get_wallet_history().as_str() == "Waiting for a deposit…" {
        ui.set_wallet_history("Transaction history unavailable until synced".into());
        apply_wallet_tx_rows(ui, "");
    }

    if let Some(bal) = parse_wallet_balance_atomic(raw) {
        // BAL 0 / spent-out clear → em dash (not "0.0000") for standard face.
        let pretty = if bal == 0 {
            "—".into()
        } else {
            format_xmr_atomic(bal)
        };
        debug_println!("SLINT FACE BAL {}", pretty);
        if ui.get_wallet_balance().as_str() != pretty {
            ui.set_wallet_balance(pretty.clone().into());
        }
        // Full wallet status carries the independently calculated mature balance.
        let unlocked = raw.split("UNLOCKED ").nth(1).and_then(|rest| {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().ok()
        }).unwrap_or(bal);
        let unlocked = if unlocked == 0 { "—".into() } else { format_xmr_atomic(unlocked) };
        if ui.get_wallet_unlocked().as_str() != unlocked {
            ui.set_wallet_unlocked(unlocked.into());
        }
    } else if raw.contains("NEED DEPOSIT") {
        debug_println!("SLINT FACE BAL —");
        if ui.get_wallet_balance().as_str() != "—" {
            ui.set_wallet_balance("—".into());
        }
        if ui.get_wallet_unlocked().as_str() != "—" {
            ui.set_wallet_unlocked("—".into());
        }
        // Do not present an unscanned wallet as waiting for a deposit.
        let hist = ui.get_wallet_history().to_string();
        if waiting && (hist.starts_with("No transactions") || hist.is_empty()) {
            ui.set_wallet_history("Waiting for a deposit…".into());
            apply_wallet_tx_rows(ui, "");
        }
    } else if raw.contains("BAL ") {
        debug_println!("SLINT FACE BAL PARSE_FAIL");
    }
    // Detail follows the same spent-gate sticky as the sync title.
    let detail = if ui.get_wallet_sync().as_str().contains("Waiting for deposit") {
        Some("Use Receive to copy your address".into())
    } else {
        wallet_sync_detail(raw)
    };
    if let Some(detail) = detail {
        if ui.get_wallet_sync_detail().as_str() != detail {
            ui.set_wallet_sync_detail(detail.into());
        }
    }
    if let Some(addr) = parse_wallet_address(raw) {
        if ui.get_wallet_address().as_str() != addr {
            ui.set_wallet_address(addr.clone().into());
            let short = if addr.len() > 16 {
                alloc::format!("{}…{}", &addr[..8], &addr[addr.len() - 6..])
            } else {
                addr.clone()
            };
            ui.set_wallet_address_short(short.into());
            present_wallet_address_qr(ui, &addr);
        }
    }
    if let Some(hist) = parse_wallet_history(raw) {
        if ui.get_wallet_history().as_str() != hist {
            debug_println!("SLINT FACE HIST {}", hist);
            apply_wallet_tx_rows(ui, &hist);
            ui.set_wallet_history(hist.into());
        }
    }
    if raw.contains("SEND OK") {
        ui.set_wallet_send_status("Relayed · awaiting confirmation".into());
    } else if raw.contains("SEND UNCONFIRMED") {
        ui.set_wallet_send_status("Relay outcome unknown · check transaction".into());
    }
}

fn humanize_wallet_status(raw: &str) -> String {
    let r = raw.trim();
    if r.contains("RATE WAIT") { return "Node busy · waiting to retry…".into(); }
    if r.contains("DNS RETRY WAIT") { return "Node lookup failed · retrying…".into(); }
    if r.contains("NETWORK WAIT") || r.contains("NETWORK RETRY WAIT") { return "Connection lost · retrying…".into(); }
    if r.contains("LEGACY OUTPUTS UNSUPPORTED") {
        return "Older wallet outputs are not supported yet".into();
    }
    if r.contains("NODE NETWORK MISMATCH") {
        return "Node network does not match wallet".into();
    }
    if r.contains("PAYMENT HISTORY FULL") {
        return "Payment history storage is full".into();
    }
    if r == "Address copied" || r.eq_ignore_ascii_case("address copied") {
        return "Address copied".into();
    }
    if r.contains("Address not ready") {
        return "Address not ready yet".into();
    }
    if r.contains("SEND OK") {
        return "Relayed · awaiting confirmation".into();
    }
    if r.contains("SEND UNCONFIRMED") {
        return "Relay outcome unknown · check transaction".into();
    }
    if r.contains("VERIFY NODE FIRST") { return "Verify the node before retrying".into(); }
    if r.contains("SEND NOT SAVED") || r.contains("SEND NEED PREPARE") { return "No saved transaction to retry".into(); }
    if r.contains("SEND INVALID PENDING") { return "Saved transaction is invalid".into(); }
    if r.contains("SEND BUSY") { return "Wallet is busy · try again".into(); }
    if r.contains("PENDING SEND") { return "A transaction is pending · check or retry it".into(); }
    if r.contains("SEND REJECTED") || r.contains("SEND FAIL") {
        return "Send failed".into();
    }
    if r.contains("SEND QUEUED") {
        return "Sending…".into();
    }
    if r.contains("BAD ADDR") {
        return "Address not valid".into();
    }
    if r.contains("PREPARE OK") {
        return "Ready to send".into();
    }
    if r.contains("INSUFFICIENT") {
        return "Not enough balance".into();
    }
    if r.contains("TLS TIMEOUT") || r.contains("TLS FAILED") || r.contains("TLS INIT FAILED")
        || r.contains("TLS STATE LOST") || r.contains("SYNC FAIL")
    {
        return "Connection failed · retry sync".into();
    }
    if r.contains("CONNECT FAIL") || r.contains("RECV FAIL") || r.contains("SYNC TIMEOUT")
        || r.contains("BAD HTTP")
    {
        return "Can’t reach node — retrying…".into();
    }
    if r.contains("NEED DEPOSIT") {
        return if waiting_for_deposit(r) { "Waiting for deposit…" } else { "Syncing…" }.into();
    }
    if r.contains("NEED SCAN") || r.contains("NEED DECOYS") {
        return "Still syncing…".into();
    }
    if r.contains("REORG RECOVERY REQUIRED") {
        return "Payment needs recovery".into();
    }
    if r.contains("JOURNAL SAVE FAILED") {
        return "Couldn’t save payment history".into();
    }
    if r.contains("HISTORY UNVERIFIED") || r.contains("HISTORY INVALID") {
        return "Couldn’t verify wallet history".into();
    }
    if r.contains("PAYMENT REORGED") {
        return "Payment needs checking".into();
    }
    if r.contains("SCAN OK") && r.contains("SEND NOT READY") {
        return "Synced — sending unavailable".into();
    }
    if scan_complete(r) {
        return "Wallet synced".into();
    }
    if r.contains("TIP HUNT") || r.contains("TIP ") || r.contains(" RX") {
        return "Syncing…".into();
    }
    // "NEED DEPOSIT" already handled above — bare DEPOSIT/RETRY stays Syncing.
    if r.contains("RECV") || r.contains("TIP ") || r.contains(" RX")
        || (r.contains("DEPOSIT") && !r.contains("NEED DEPOSIT"))
    {
        return "Syncing…".into();
    }
    if r.contains("CONNECTING") || r.contains("WAIT NET") || r.contains("QUEUED") {
        return "Connecting…".into();
    }
    if r.contains("OPENING") || r.contains("ARMED") || r.contains("BOOT PHASE") {
        return "Opening wallet…".into();
    }
    if r.contains("OPEN FAIL") || r.contains("IPC FAIL") {
        return "Couldn’t open wallet".into();
    }
    if r.contains("OPEN ") && r.contains("BAL ") {
        return "Waiting to sync".into();
    }
    if r.contains("CLOSED") {
        return "Closed".into();
    }
    // Fall back: strip techno prefixes.
    r.trim_start_matches("NEXAWAL | ")
        .trim_start_matches("NEXAWAL · ")
        .to_string()
}

fn parse_wallet_balance_atomic(raw: &str) -> Option<u64> {
    let rest = raw.split("BAL ").nth(1)?;
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn parse_wallet_address(raw: &str) -> Option<String> {
    if let Some(rest) = raw.split("ADDR ").nth(1) {
        let addr: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        // Standard Monero primary addresses are ~95 chars; require full length
        // so a 12-char tip prefix never populates Receive as the "full" address.
        if addr.len() >= 95 {
            return Some(addr);
        }
    }
    None
}

fn apply_wallet_tx_rows(ui: &NotesWindow, hist: &str) {
    let mut rows: [Option<(bool, alloc::string::String)>; 5] = [None, None, None, None, None];
    let mut count = 0usize;
    for line in hist.lines() {
        if count == rows.len() {
            break;
        }
        let line = line.trim();
        let incoming = line.starts_with("Received");
        let outgoing = line.starts_with("Sent");
        if !incoming && !outgoing {
            continue;
        }
        let Some(amount) = line.split('·').nth(1).map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let signed = if incoming {
            alloc::format!("+ {amount}")
        } else {
            alloc::format!("− {amount}")
        };
        rows[count] = Some((incoming, signed));
        count += 1;
    }
    let slot = |index: usize| -> (bool, alloc::string::String) {
        rows[index]
            .clone()
            .unwrap_or((false, alloc::string::String::new()))
    };
    let (i0, a0) = slot(0);
    let (i1, a1) = slot(1);
    let (i2, a2) = slot(2);
    let (i3, a3) = slot(3);
    let (i4, a4) = slot(4);
    ui.set_wallet_tx_in_0(i0);
    ui.set_wallet_tx_amt_0(a0.into());
    ui.set_wallet_tx_in_1(i1);
    ui.set_wallet_tx_amt_1(a1.into());
    ui.set_wallet_tx_in_2(i2);
    ui.set_wallet_tx_amt_2(a2.into());
    ui.set_wallet_tx_in_3(i3);
    ui.set_wallet_tx_amt_3(a3.into());
    ui.set_wallet_tx_in_4(i4);
    ui.set_wallet_tx_amt_4(a4.into());
}

fn parse_wallet_history(raw: &str) -> Option<String> {
    let rest = raw.split("HIST ").nth(1)?;
    let body = rest.split('|').next()?.trim();
    if body.is_empty() {
        return None;
    }
    let mut lines = String::new();
    for (i, piece) in body.split(';').enumerate() {
        let p = piece.trim();
        if p.is_empty() {
            continue;
        }
        let (sign, digits) = if let Some(d) = p.strip_prefix('+') {
            ("Received", d)
        } else if let Some(d) = p.strip_prefix('-') {
            ("Sent", d)
        } else {
            ("Received", p)
        };
        let Ok(atomic) = digits
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
        else {
            continue;
        };
        if i > 0 {
            lines.push('\n');
        }
        lines.push_str(&alloc::format!("{sign} · {} XMR", format_xmr_atomic(atomic)));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

fn parse_xmr_amount_to_atomic(text: &str) -> Option<u64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let mut parts = t.split('.');
    let whole = parts.next()?;
    let frac = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return None;
    }
    let whole_n: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut frac_digits = [b'0'; 12];
    for (i, c) in frac.chars().take(12).enumerate() {
        if !c.is_ascii_digit() {
            return None;
        }
        frac_digits[i] = c as u8;
    }
    let frac_n: u64 = core::str::from_utf8(&frac_digits).ok()?.parse().ok()?;
    whole_n
        .checked_mul(1_000_000_000_000)?
        .checked_add(frac_n)
}

fn format_xmr_atomic(atomic: u64) -> String {
    let whole = atomic / 1_000_000_000_000;
    let frac = atomic % 1_000_000_000_000;
    // Trim trailing zeros for a consumer-facing balance (keep ≥4 frac digits).
    let mut s = alloc::format!("{whole}.{frac:012}");
    while s.ends_with('0') && s.len() > whole.to_string().len() + 1 + 4 {
        s.pop();
    }
    s
}

fn wallet_sync_detail(raw: &str) -> Option<String> {
    if raw.contains("LEGACY OUTPUTS UNSUPPORTED") {
        return Some("Sync stopped; the balance and history may be incomplete".into());
    }
    if raw.contains("NODE NETWORK MISMATCH") {
        return Some("Choose a node for this network in Settings".into());
    }
    if let Some((current, tip)) = scan_progress(raw) {
        return Some(alloc::format!("Scanning block {current}\nNode height {tip}"));
    }
    if scan_complete(raw) {
        return Some("Scan complete".into());
    }
    // Spent-out / empty wallet: don't leave "Opening…" + node paste under a clear face.
    if waiting_for_deposit(raw) {
        return Some("Use Receive to copy your address".into());
    }
    let mut height = None;
    if let Some(rest) = raw.split("| H").nth(1).or_else(|| raw.split(" H").nth(1)) {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            height = digits.parse::<u64>().ok();
        }
    }
    if let Some(h) = height {
        // Only claim 100% when the wallet reports a finished scan — restore
        // height H3519450 must not look "fully synced" while still Connecting.
        let done = scan_complete(raw);
        let progress = if done { "100.0%" } else { "…" };
        Some(alloc::format!(
            "Scanned to block {h}\nProgress  {progress}"
        ))
    } else if raw.contains("OPEN ") || raw.contains("SCAN") || raw.contains("TIP ") {
        Some("Waiting for scan progress".into())
    } else {
        None
    }
}

fn emit_nexatrode_copy(text: &str) {
    if text.is_empty() {
        return;
    }
    let escaped = escape_copy(text);
    debug_println!("NEXATRODE_COPY|BEGIN|{}", escaped.len());
    let mut start = 0;
    while start < escaped.len() {
        let mut end = (start + 80).min(escaped.len());
        while end > start && !escaped.is_char_boundary(end) {
            end -= 1;
        }
        debug_println!("NEXATRODE_COPY|DATA|{}", &escaped[start..end]);
        start = end;
    }
    debug_println!("NEXATRODE_COPY|END");
}

fn hardware_character(code: u64, shift: bool) -> Option<&'static str> {
    if code == 14 {
        return Some("\u{8}");
    }
    if code == 28 {
        return Some("\n");
    }
    if code == 57 {
        return Some(" ");
    }
    let (plain, shifted) = match code {
        2 => ("1", "!"),
        3 => ("2", "@"),
        4 => ("3", "#"),
        5 => ("4", "$"),
        6 => ("5", "%"),
        7 => ("6", "^"),
        8 => ("7", "&"),
        9 => ("8", "*"),
        10 => ("9", "("),
        11 => ("0", ")"),
        12 => ("-", "_"),
        13 => ("=", "+"),
        16 => ("q", "Q"),
        17 => ("w", "W"),
        18 => ("e", "E"),
        19 => ("r", "R"),
        20 => ("t", "T"),
        21 => ("y", "Y"),
        22 => ("u", "U"),
        23 => ("i", "I"),
        24 => ("o", "O"),
        25 => ("p", "P"),
        26 => ("[", "{"),
        27 => ("]", "}"),
        30 => ("a", "A"),
        31 => ("s", "S"),
        32 => ("d", "D"),
        33 => ("f", "F"),
        34 => ("g", "G"),
        35 => ("h", "H"),
        36 => ("j", "J"),
        37 => ("k", "K"),
        38 => ("l", "L"),
        39 => (";", ":"),
        40 => ("'", "\""),
        41 => ("`", "~"),
        43 => ("\\", "|"),
        44 => ("z", "Z"),
        45 => ("x", "X"),
        46 => ("c", "C"),
        47 => ("v", "V"),
        48 => ("b", "B"),
        49 => ("n", "N"),
        50 => ("m", "M"),
        51 => (",", "<"),
        52 => (".", ">"),
        53 => ("/", "?"),
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

fn has_simplex_connection(snapshot: &str) -> bool {
    snapshot
        .lines()
        .any(|line| line.starts_with("CONNECTION ID "))
}

fn profile_name_from_snapshot(snapshot: &str) -> &str {
    snapshot
        .lines()
        .find_map(|line| line.strip_prefix("PROFILE "))
        .unwrap_or("")
}

fn contact_name_from_snapshot(snapshot: &str) -> &str {
    snapshot
        .lines()
        .find_map(|line| line.strip_prefix("CONTACT "))
        .unwrap_or("")
}

fn append_into(transcript: &mut String, speaker: &str, body: &str) {
    let line_start = transcript.len();
    if !transcript.is_empty() && !transcript.ends_with('\n') {
        transcript.push('\n');
    }
    transcript.push_str(speaker);
    transcript.push_str(": ");
    transcript.push_str(body);
    transcript.push('\n');
    const LIMIT: usize = 700;
    if transcript.len() > LIMIT {
        let tail = transcript.len() - LIMIT;
        let cut = transcript[tail..]
            .find('\n')
            .map(|index| tail + index + 1)
            .unwrap_or(line_start);
        let kept = transcript[cut..].to_string();
        transcript.clear();
        transcript.push_str(&kept);
    }
}

fn chat_name_at(ui: &NotesWindow, index: usize) -> alloc::string::String {
    let name = ui
        .get_message_chats()
        .row_data(index)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| ui.get_message_selected());
    if name.is_empty() {
        String::from("Contact")
    } else {
        name.to_string()
    }
}

fn remember_chat_line(
    ui: &NotesWindow,
    transcripts: &RefCell<[String; CHAT_SLOTS]>,
    chat_index: usize,
    speaker: &str,
    body: &str,
) {
    let index = chat_index.min(CHAT_SLOTS - 1);
    let saved = {
        let mut slots = transcripts.borrow_mut();
        append_into(&mut slots[index], speaker, body);
        slots[index].clone()
    };
    if ui.get_message_chat_index() == index as i32 {
        ui.set_message_transcript(saved.into());
    }
}

fn photo_mask_from_snapshot(snapshot: &str) -> u16 {
    for line in snapshot.lines() {
        if let Some(value) = line.strip_prefix("PHOTO_MASK ") {
            if let Ok(mask) = value.trim().parse::<u16>() {
                return mask;
            }
        }
    }
    0
}

fn present_peer_photos(service: &MessagesServiceClient, photo_mask: u16) {
    for index in 0..CHAT_SLOTS {
        if photo_mask & (1u16 << index) == 0 {
            continue;
        }
        let Ok((_, length)) = service.peer_photo_meta(index as u64) else {
            continue;
        };
        if length == 0 || length > 12288 {
            continue;
        }
        let mut bytes = Vec::with_capacity(length);
        let mut offset = 0;
        while offset < length {
            let Ok(chunk) = service.peer_photo_chunk(index as u64, offset) else {
                bytes.clear();
                break;
            };
            if chunk.is_empty() {
                break;
            }
            offset += chunk.len();
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != length {
            continue;
        }
        if let Some((width, height, pixels)) = decode_jpeg_rgba(&bytes) {
            skia_guest::note_chat_photo(index, &pixels, width, height);
            debug_println!("SLINT SIMPLEX PEER PHOTO {} {}x{}", index, width, height);
        }
    }
}

fn apply_message_chats(
    ui: &NotesWindow,
    snapshot: &str,
    archived_mask: u16,
    transcripts: Option<&[String; CHAT_SLOTS]>,
) {
    let mut slots: Vec<(bool, slint::SharedString)> = Vec::new();
    for line in snapshot.lines() {
        if let Some(name) = line.strip_prefix("CHAT ") {
            if slots.len() < CHAT_SLOTS && !name.is_empty() {
                slots.push((false, slint::SharedString::from(name)));
            }
        } else if let Some(name) = line.strip_prefix("GROUP ") {
            if slots.len() < CHAT_SLOTS && !name.is_empty() {
                slots.push((true, slint::SharedString::from(name)));
            }
        }
    }
    if slots.is_empty() {
        let contact = contact_name_from_snapshot(snapshot);
        if !contact.is_empty() {
            slots.push((false, slint::SharedString::from(contact)));
        }
    }
    let query = ui.get_message_search().to_string();
    let query_upper = query.to_ascii_uppercase();
    let matches_query = |index: usize, name: &str| -> bool {
        if query.is_empty() {
            return true;
        }
        if name.to_ascii_uppercase().contains(&query_upper) {
            return true;
        }
        transcripts
            .and_then(|slots| slots.get(index))
            .is_some_and(|text| text.to_ascii_uppercase().contains(&query_upper))
    };
    let mut active = Vec::new();
    let mut active_map = Vec::new();
    let mut groups = Vec::new();
    let mut group_map = Vec::new();
    let mut archived = Vec::new();
    let mut archived_map = Vec::new();
    let mut listed = [""; CHAT_SLOTS];
    for (index, (is_group, name)) in slots.iter().enumerate().take(CHAT_SLOTS) {
        listed[index] = name.as_str();
        if !matches_query(index, name.as_str()) {
            continue;
        }
        if archived_mask & (1u16 << index) != 0 {
            archived.push(name.clone());
            archived_map.push(index as i32);
        } else if *is_group {
            groups.push(name.clone());
            group_map.push(index as i32);
        } else {
            active.push(name.clone());
            active_map.push(index as i32);
        }
    }
    let count = slots.len();
    skia_guest::set_chat_names(&listed[..count.min(CHAT_SLOTS)]);
    ui.set_message_chats(ModelRc::new(VecModel::from(active)));
    ui.set_message_chat_index_map(ModelRc::new(VecModel::from(active_map)));
    ui.set_message_group_chats(ModelRc::new(VecModel::from(groups)));
    ui.set_message_group_index_map(ModelRc::new(VecModel::from(group_map)));
    ui.set_message_archived_chats(ModelRc::new(VecModel::from(archived)));
    ui.set_message_archived_index_map(ModelRc::new(VecModel::from(archived_map)));
    debug_println!("SLINT SIMPLEX CHATS {}", count);
}

fn transcript_stamp(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ (text.len() as u64) ^ 1
}

fn strip_shared_msg_id(body: &str) -> (String, &str) {
    const MARK: &str = "\u{001e}#\u{001f}";
    if let Some(rest) = body.strip_prefix(MARK) {
        if let Some((id, text)) = rest.split_once('\u{001f}') {
            return (id.to_string(), text);
        }
    }
    (String::new(), body)
}

fn strip_reaction_suffix(body: &str) -> (&str, String) {
    const MARK: &str = "\u{001e}!\u{001f}";
    match body.split_once(MARK) {
        Some((text, reacts)) => (text, reacts.to_string()),
        None => (body, String::new()),
    }
}

/// The software font has no emoji, so a stored reaction is shown as letters.
fn visible_react(react: &str) -> String {
    if react.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for mark in react.chars() {
        let label = match mark {
            '\u{1f44d}' => "+1 ",
            '\u{2764}' => "<3 ",
            '\u{1f602}' => "ha ",
            '\u{2705}' => "ok ",
            _ => "",
        };
        if label.is_empty() {
            out.push(mark);
        } else {
            out.push_str(label);
        }
    }
    out
}

/// Transcript display for quoted sends: `↩ {quote} · {text}`.
fn split_quote_display(body: &str) -> (String, &str) {
    const PREFIX: &str = "↩ ";
    const SEP: &str = " · ";
    if let Some(rest) = body.strip_prefix(PREFIX) {
        if let Some((quote, text)) = rest.split_once(SEP) {
            if !quote.is_empty() && !text.is_empty() {
                return (quote.to_string(), text);
            }
        }
    }
    (String::new(), body)
}

#[cfg(feature = "utm-photo-worker")]
struct CachedPhotoModel {
    rows: personal_os_slint_notes_spike::photo_rows::PhotoRows<slint::Image>,
    notify: slint::ModelNotify,
}

#[cfg(feature = "utm-photo-worker")]
impl slint::Model for CachedPhotoModel {
    type Data = slint::Image;
    fn row_count(&self) -> usize { self.rows.len() }
    fn row_data(&self, row: usize) -> Option<Self::Data> {
        (row < self.rows.len()).then(|| self.rows.get(row).unwrap_or_default())
    }
    fn set_row_data(&self, row: usize, _image: Self::Data) {
        if row < self.rows.len() { self.notify.row_changed(row); }
    }
    fn model_tracker(&self) -> &dyn slint::ModelTracker { &self.notify }
}

fn sync_message_bubbles(
    ui: &NotesWindow,
    loader: &RefCell<personal_os_slint_notes_spike::chat_photo_loader::PhotoLoader>,
    #[cfg(feature = "utm-photo-worker")] cache: &Rc<RefCell<personal_os_slint_notes_spike::photo_cache::PhotoCache<slint::Image>>>,
    #[cfg(feature = "utm-photo-worker")] owner: Option<u64>,
) {
    static STAMP: AtomicU64 = AtomicU64::new(0);
    let transcript = ui.get_message_transcript();
    let chat = ui.get_message_chat_index().max(0) as u64;
    let stamp = transcript_stamp(transcript.as_str()).rotate_left(8) ^ chat;
    if STAMP.swap(stamp, Ordering::Relaxed) == stamp {
        return;
    }
    #[cfg(feature = "utm-perf-trace")]
    let trace_bubbles_start = perf_time_us();
    let mut bodies = Vec::new();
    let mut quotes = Vec::new();
    let mut times = Vec::new();
    let mut sides = Vec::new();
    let mut ids = Vec::new();
    let mut reacts = Vec::new();
    let mut photos = Vec::new();
    #[cfg(feature = "utm-photo-worker")]
    let mut display_ids = Vec::new();
    let mut photo_ready = Vec::new();
    let mut photo_pending = Vec::new();
    let mut photo_jobs = Vec::new();
    let mut photo_ids = Vec::new();
    let mut files = Vec::new();
    let mut failed_uploads = Vec::new();
    let mut saw_outgoing_photo = false;
    let mut incoming_photo: i32 = -1;
    for line in transcript.lines() {
        if line.is_empty() {
            continue;
        }
        let mine = line.starts_with("NexatrodeOS: ");
        let body = line.split_once(": ").map(|(_, body)| body).unwrap_or(line);
        let body = if body.is_empty() { line } else { body };
        let (clock, body) = split_message_time(body);
        let direct = split_direct_text(body);
        let mine = mine && direct.is_none();
        let delivered = direct.is_none() && body.ends_with(" \u{2713}");
        let body = if delivered {
            body.trim_end_matches(" \u{2713}")
        } else {
            body
        };
        let body = if body.is_empty() { line } else { body };
        let failed = mine && body.ends_with(personal_os_rust_server::messages::UPLOAD_FAILED_SUFFIX);
        let body = if failed { body.trim_end_matches(personal_os_rust_server::messages::UPLOAD_FAILED_SUFFIX) } else { body };
        failed_uploads.push(failed);
        let indexed_photo = photo_reference(body);
        #[cfg(feature = "utm-photo-worker")]
        display_ids.push(indexed_photo.map(|photo| photo.display_id).unwrap_or(0));
        photo_ids.push(slint::SharedString::from(indexed_photo.map(|r| r.save_id.to_string()).unwrap_or_default()));
        let (image, body) = match indexed_photo {
            Some(photo) => {
                let id = photo.display_id;
                #[cfg(feature = "utm-photo-worker")]
                let image = owner.and_then(|owner| cache.borrow_mut().get(owner,id));
                #[cfg(not(feature = "utm-photo-worker"))]
                let image: Option<slint::Image> = None;
                if id != 0 { photo_jobs.push((bodies.len(), id)); }
                photo_pending.push(id != 0 && image.is_none());
                (image, photo.caption)
            }
            None => { photo_pending.push(false); (None, body) },
        };
        let (file, body) = match split_file_reference(body) {
            Some((_, id, name)) => (slint::SharedString::from(id.to_string()), name),
            None => (slint::SharedString::default(), body),
        };
        files.push(file);
        photo_ready.push(image.is_some());
        photos.push(image.unwrap_or_default());
        let (msg_id, body) = direct.map(|(id, body)| (id.to_string(), body))
            .unwrap_or_else(|| strip_shared_msg_id(body));
        let (body, react) = strip_reaction_suffix(body);
        let mut decoded = String::new();
        let body = if direct.is_some() && decode_direct_text(body, &mut decoded).is_ok() {
            decoded.as_str()
        } else { body };
        let (quote, body) = split_quote_display(body);
        if mine && body == "Photo" && indexed_photo.is_none() {
            saw_outgoing_photo = true;
        }
        if direct.is_none() && !mine && indexed_photo.is_none() && body == "Photo" {
            incoming_photo = bodies.len() as i32;
        }
        let mut when = String::new();
        if let Some(clock) = clock {
            when.push_str(clock);
        }
        if failed {
            when.push_str("  Upload failed");
        } else if delivered {
            if !when.is_empty() {
                when.push_str("  ");
            }
            when.push('\u{2713}');
        }
        bodies.push(slint::SharedString::from(body));
        quotes.push(slint::SharedString::from(quote));
        times.push(slint::SharedString::from(when));
        sides.push(if mine { 1 } else { 0 });
        ids.push(slint::SharedString::from(msg_id));
        reacts.push(slint::SharedString::from(visible_react(&react)));
    }
    let count = bodies.len();
    // Conversations open at the newest messages; prefer those visible previews.
    photo_jobs.reverse();
    loader.borrow_mut().reset(stamp, chat, &photo_jobs);
    #[cfg(feature = "utm-perf-trace")]
    debug_println!("PERF|BUBBLES|{}|{}|{}", trace_bubbles_start, perf_time_us(), count);
    ui.set_message_bubbles(ModelRc::new(VecModel::from(bodies)));
    ui.set_message_bubble_quotes(ModelRc::new(VecModel::from(quotes)));
    ui.set_message_bubble_time(ModelRc::new(VecModel::from(times)));
    ui.set_message_bubble_side(ModelRc::new(VecModel::from(sides)));
    ui.set_message_bubble_ids(ModelRc::new(VecModel::from(ids)));
    ui.set_message_bubble_reacts(ModelRc::new(VecModel::from(reacts)));
    #[cfg(feature = "utm-photo-worker")]
    ui.set_message_bubble_photos(ModelRc::new(CachedPhotoModel {
        rows: personal_os_slint_notes_spike::photo_rows::PhotoRows::new(cache.clone(), owner, display_ids),
        notify: slint::ModelNotify::default(),
    }));
    #[cfg(not(feature = "utm-photo-worker"))]
    ui.set_message_bubble_photos(ModelRc::new(VecModel::from(photos)));
    ui.set_message_bubble_photo_ready(ModelRc::new(VecModel::from(photo_ready)));
    ui.set_message_bubble_photo_pending(ModelRc::new(VecModel::from(photo_pending)));
    ui.set_message_bubble_photo_ids(ModelRc::new(VecModel::from(photo_ids)));
    ui.set_message_bubble_files(ModelRc::new(VecModel::from(files)));
    ui.set_message_bubble_upload_failed(ModelRc::new(VecModel::from(failed_uploads)));
    ui.set_message_incoming_photo_index(incoming_photo);
    if saw_outgoing_photo && !ui.get_message_image_visible() {
        if let Some(jpeg) = bundled_photo_jpeg() {
            if let Some(image) = decode_jpeg_preview(&jpeg) {
                ui.set_message_image(image);
                ui.set_message_image_visible(true);
                debug_println!("SLINT SIMPLEX PHOTO {}", jpeg.len());
            }
        }
    }
    debug_println!("SLINT BUBBLES {}", count);
}

fn chat_preview(transcript: &str) -> alloc::borrow::Cow<'_, str> {
    let Some(line) = transcript.lines().rev().find(|line| !line.is_empty()) else {
        return "Encrypted".into();
    };
    let body = match line.split_once(": ") {
        Some((_, body)) if !body.is_empty() => body,
        _ => line,
    };
    let body = split_message_time(body).1;
    if let Some((_, encoded)) = split_direct_text(body) {
        let mut preview = String::new();
        if decode_direct_text(strip_reaction_suffix(encoded).0, &mut preview).is_ok() {
            return preview.into();
        }
    }
    let body = body.trim_end_matches(" \u{2713}");
    let body = body.strip_suffix(personal_os_rust_server::messages::UPLOAD_FAILED_SUFFIX).unwrap_or(body);
    let body = split_photo_reference(body)
        .or_else(|| split_file_reference(body))
        .map_or(body, |(_, _, caption)| caption);
    strip_reaction_suffix(strip_shared_msg_id(body).1).0.into()
}

use personal_os_slint_notes_spike::chat_unread::unread_incoming;

fn chat_clock(transcript: &str) -> &str {
    let Some(line) = transcript.lines().rev().find(|line| !line.is_empty()) else {
        return "";
    };
    let body = match line.split_once(": ") {
        Some((_, body)) if !body.is_empty() => body,
        _ => return "",
    };
    split_message_time(body).0.unwrap_or("")
}

fn keep_received_message(
    ui: &NotesWindow,
    text: &str,
    chat_index: usize,
    transcripts: &Rc<RefCell<[String; CHAT_SLOTS]>>,
    service: &Rc<RefCell<MessagesServiceClient>>,
) {
    #[cfg(feature = "simplex-cli-qemu-test")]
    if text == "REPLY FROM SIMPLEX CLI" {
        debug_println!("SLINT SIMPLEX EXTERNAL REPLY VERIFIED");
    }
    let index = if chat_index < CHAT_SLOTS {
        chat_index
    } else {
        ui.get_message_chat_index().max(0) as usize
    };
    let speaker = chat_name_at(ui, index);
    #[cfg(feature = "utm-transcript-worker")]
    service.borrow().history_changed(index);
    match service.borrow().transcript(index as u64) {
        Ok(saved) => {
            transcripts.borrow_mut()[index.min(CHAT_SLOTS - 1)] = saved.clone();
            if ui.get_message_chat_index() == index.min(CHAT_SLOTS - 1) as i32 {
                ui.set_message_transcript(saved.into());
            }
        }
        Err(_) if !text.is_empty() => remember_chat_line(ui, transcripts, index, speaker.as_str(), text),
        Err(_) => {}
    }
    ui.set_message_incoming_body("".into());
    ui.set_message_receive_status(alloc::format!("From {speaker}").into());
    debug_println!("SLINT SIMPLEX RECEIVE PASS LEN {} CHAT {}", text.len(), index);
    #[cfg(feature = "utm-transcript-worker")]
    let acknowledged = service.borrow_mut().acknowledge_worker_received(index);
    #[cfg(not(feature = "utm-transcript-worker"))]
    let acknowledged = service.borrow_mut().acknowledge_received();
    match acknowledged {
        Ok(()) => match service.borrow().acknowledge_status() {
            Ok(2) => ui.set_message_ack_status("ACKNOWLEDGED".into()),
            Ok(1) => ui.set_message_ack_status("ACKNOWLEDGING".into()),
            Ok(_) => ui.set_message_ack_status("ACKNOWLEDGE FAILED".into()),
            Err(_) => ui.set_message_ack_status("ACK STATUS UNAVAILABLE".into()),
        },
        Err(_) => ui.set_message_ack_status("ACKNOWLEDGE REQUEST FAILED".into()),
    }
}

impl SlintService {
    fn sync_chat_rows(&mut self) -> usize {
        let count = skia_guest::chat_count();
        let reading = self._ui.get_message_view()
            && !self._ui.get_message_join_view()
            && !self._ui.get_message_invitation_view();
        let open_index = self
            ._ui
            .get_message_chat_index()
            .clamp(0, CHAT_SLOTS as i32 - 1) as usize;
        if count == 0 && self._ui.get_message_has_connections() {
            skia_guest::note_chat_name(0, b"Open chat");
            skia_guest::note_chat_preview(0, b"A contact is connected");
            skia_guest::note_chat_time(0, b"");
            skia_guest::note_chat_unread(0, 0);
            return 1;
        }
        #[cfg(not(feature = "simplex-agent-ui"))]
        {
            let _ = (reading, open_index);
            return count;
        }
        #[cfg(feature = "simplex-agent-ui")]
        {
            let mut next_read = self.chat_read_len;
            {
                let slots = self.chat_transcripts.borrow();
                for index in 0..count {
                    let slot = &slots[index.min(CHAT_SLOTS - 1)];
                    if reading && index == open_index {
                        next_read[index] = slot.len();
                        #[cfg(feature = "utm-transcript-worker")]
                        { self.initial_unread[index] = 0; }
                    }
                    let slot_i = index.min(CHAT_SLOTS - 1);
                    let unread = unread_incoming(slot, next_read[slot_i]);
                    #[cfg(feature = "utm-transcript-worker")]
                    let unread = unread.saturating_add(self.initial_unread[slot_i]);
                    let prev = self.last_unread[slot_i];
                    if unread != prev {
                        if unread > 0 {
                            debug_println!("SLINT SIMPLEX UNREAD|{}|{}", index, unread);
                        } else if prev > 0 {
                            debug_println!("SLINT SIMPLEX UNREAD_CLEAR|{}", index);
                        }
                        self.last_unread[slot_i] = unread;
                    }
                    skia_guest::note_chat_preview(index, chat_preview(slot).as_bytes());
                    skia_guest::note_chat_time(index, chat_clock(slot).as_bytes());
                    skia_guest::note_chat_unread(index, unread);
                }
            }
            self.chat_read_len = next_read;
            count
        }
    }

    fn current_view(&self) -> i32 {
        if self._ui.get_device_lock_screen() { return 1400; }
        let page = self._ui.get_page();
        if page == 1 && self._ui.get_note_editing() {
            return 101;
        }
        if page == 2 && self._ui.get_contact_detail() {
            return 102;
        }
        if page != 6 {
            return page;
        }
        if self._ui.get_message_view() {
            600 + self
                ._ui
                .get_message_chat_index()
                .clamp(0, CHAT_SLOTS as i32 - 1)
        } else if self._ui.get_message_join_view() {
            61
        } else if self._ui.get_message_invitation_view() {
            62
        } else {
            match self._ui.get_message_list_tab() {
                1 => 63,
                2 => 64,
                _ => 6,
            }
        }
    }

    fn picture_slot(view: i32) -> Option<usize> {
        match view {
            0 => Some(0),
            6 => Some(1),
            8 => Some(2),
            2 => Some(3),
            9 => Some(4),
            10 => Some(5),
            1 => Some(6),
            600 => Some(7),
            601 => Some(22),
            602 => Some(23),
            603 => Some(24),
            604 => Some(25),
            605 => Some(26),
            606 => Some(27),
            607 => Some(28),
            3 => Some(8),
            5 => Some(9),
            7 => Some(10),
            12 => Some(11),
            11 => Some(12),
            13 => Some(13),
            101 => Some(14),
            102 => Some(15),
            4 => Some(16),
            14 => Some(17),
            61 => Some(18),
            62 => Some(19),
            63 => Some(20),
            64 => Some(21),
            _ => None,
        }
    }

    fn view_stamp(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        let mix = |hash: &mut u64, bytes: &[u8]| {
            for byte in bytes {
                *hash ^= u64::from(*byte);
                *hash = hash.wrapping_mul(0x100000001b3);
            }
            *hash ^= bytes.len() as u64;
        };
        let page = self._ui.get_page();
        mix(&mut hash, &page.to_le_bytes());
        mix(&mut hash, self._ui.get_system_clock().as_str().as_bytes());
        mix(&mut hash, &[u8::from(self._ui.get_aurora_accent())]);
        if page == 1 {
            mix(&mut hash, &[u8::from(self._ui.get_note_editing())]);
            mix(&mut hash, self._ui.get_note().as_str().as_bytes());
        }
        if page == 2 {
            mix(&mut hash, &[u8::from(self._ui.get_contact_detail())]);
            mix(&mut hash, self._ui.get_contact_name().as_str().as_bytes());
            mix(&mut hash, self._ui.get_contact_phone().as_str().as_bytes());
            mix(&mut hash, self._ui.get_contact_status().as_str().as_bytes());
        }
        if page == 8 {
            mix(&mut hash, &self._ui.get_settings_selected().to_le_bytes());
            mix(&mut hash, self._ui.get_settings_status().as_str().as_bytes());
        }
        if page == 3 {
            mix(&mut hash, self._ui.get_calc_display().as_str().as_bytes());
        }
        if page == 5 {
            mix(&mut hash, self._ui.get_calendar_item().as_str().as_bytes());
            mix(&mut hash, self._ui.get_calendar_status().as_str().as_bytes());
        }
        if page == 7 {
            mix(&mut hash, self._ui.get_phone_number().as_str().as_bytes());
            mix(&mut hash, self._ui.get_phone_status().as_str().as_bytes());
        }
        if page == 9 {
            mix(&mut hash, self._ui.get_files_status().as_str().as_bytes());
        }
        if page == 16 {
            mix(&mut hash, self._ui.get_wallet_status().as_str().as_bytes());
            // Balance/unlocked must bust picture-cache — otherwise a cached
            // "Unlocked —" frame wins after EARLY_FUNDED sets the properties.
            mix(&mut hash, self._ui.get_wallet_balance().as_str().as_bytes());
            mix(&mut hash, self._ui.get_wallet_unlocked().as_str().as_bytes());
            // Sync title/detail too — spent-gate "Waiting for deposit…" must
            // not stay stuck on a cached "Connecting…" frame.
            mix(&mut hash, self._ui.get_wallet_sync().as_str().as_bytes());
            mix(
                &mut hash,
                self._ui.get_wallet_sync_detail().as_str().as_bytes(),
            );
            // History too — a cached "Waiting for a deposit…" card stays up
            // after HIST arrives if the stamp still matches the old face.
            mix(&mut hash, self._ui.get_wallet_history().as_str().as_bytes());
            mix(&mut hash, &self._ui.get_wallet_tab().to_le_bytes());
        }
        if page == 12 {
            mix(&mut hash, self._ui.get_theme_choice().as_str().as_bytes());
        }
        if page == 11 {
            mix(&mut hash, self._ui.get_quick_selected().as_str().as_bytes());
        }
        if page == 13 {
            mix(&mut hash, &[u8::from(self._ui.get_lockdown_preview())]);
            mix(&mut hash, &[u8::from(self._ui.get_message_receipts())]);
            mix(
                &mut hash,
                self._ui.get_message_servers().as_str().as_bytes(),
            );
        }
        if page == 15 {
            mix(&mut hash, &self._ui.get_edit_target().to_le_bytes());
            mix(
                &mut hash,
                self._ui.get_db_passphrase_current().as_str().as_bytes(),
            );
            mix(
                &mut hash,
                self._ui.get_db_passphrase_new().as_str().as_bytes(),
            );
            mix(&mut hash, self._ui.get_db_unlock_status().as_str().as_bytes());
            mix(&mut hash, self._ui.get_db_export_status().as_str().as_bytes());
            mix(&mut hash, self._ui.get_db_import_status().as_str().as_bytes());
            mix(&mut hash, &[u8::from(self._ui.get_db_unlocked())]);
        }
        if page == 6 {
            mix(&mut hash, &self._ui.get_edit_target().to_le_bytes());
            mix(&mut hash, &self._ui.get_message_list_scroll().to_bits().to_le_bytes());
            mix(&mut hash, self._ui.get_message_group_name().as_str().as_bytes());
            mix(&mut hash, self._ui.get_message_group_status().as_str().as_bytes());
            mix(&mut hash, &self._ui.get_message_group_recipient().to_le_bytes());
            mix(&mut hash, self._ui.get_message_group_recipient_name().as_str().as_bytes());
            mix(
                &mut hash,
                self._ui.get_message_search().as_str().as_bytes(),
            );
            mix(&mut hash, &self._ui.get_message_list_tab().to_le_bytes());
            mix(&mut hash, &[u8::from(self._ui.get_message_service_ready())]);
            mix(&mut hash, &[u8::from(self._ui.get_message_has_connections())]);
            let chats = self._ui.get_message_chats();
            mix(&mut hash, &(chats.row_count() as u64).to_le_bytes());
            for index in 0..chats.row_count() {
                if let Some(name) = chats.row_data(index) {
                    mix(&mut hash, name.as_str().as_bytes());
                }
            }
            #[cfg(feature = "simplex-agent-ui")]
            {
                let slots = self.chat_transcripts.borrow();
                for slot in slots.iter() {
                    mix(&mut hash, slot.as_bytes());
                }
            }
            if self._ui.get_message_view() {
                mix(&mut hash, &self._ui.get_message_chat_index().to_le_bytes());
                mix(&mut hash, self._ui.get_message_body().as_str().as_bytes());
                mix(&mut hash, self._ui.get_message_send_status().as_str().as_bytes());
                mix(&mut hash, self._ui.get_message_receive_status().as_str().as_bytes());
                mix(&mut hash, self._ui.get_message_transcript().as_str().as_bytes());
                // A decoded/released photo changes pixels without changing the
                // transcript. Do not restore an older Loading-photo picture.
                let ready=self._ui.get_message_bubble_photo_ready();
                let pending=self._ui.get_message_bubble_photo_pending();
                for row in 0..ready.row_count() {
                    mix(&mut hash, &[u8::from(ready.row_data(row).unwrap_or(false)),
                        u8::from(pending.row_data(row).unwrap_or(false))]);
                }
                mix(&mut hash, self._ui.get_message_selected().as_str().as_bytes());
                mix(&mut hash, &[u8::from(self._ui.get_message_image_visible())]);
                mix(
                    &mut hash,
                    &[u8::from(self._ui.get_message_incoming_image_visible())],
                );
                mix(&mut hash, &[u8::from(self._ui.get_message_file_open())]);
                mix(&mut hash, &[u8::from(self._ui.get_message_attach_menu())]);
                mix(
                    &mut hash,
                    self._ui.get_message_file_body().as_str().as_bytes(),
                );
                mix(&mut hash, &[u8::from(self._ui.get_message_group_invite())]);
            } else if self._ui.get_message_join_view() {
                mix(&mut hash, &self._ui.get_edit_target().to_le_bytes());
                mix(&mut hash, self._ui.get_message_join_uri().as_str().as_bytes());
                mix(&mut hash, self._ui.get_message_join_status().as_str().as_bytes());
                mix(&mut hash, &[u8::from(self._ui.get_message_join_requested())]);
            } else if self._ui.get_message_invitation_view() {
                mix(
                    &mut hash,
                    self._ui.get_message_invitation_uri().as_str().as_bytes(),
                );
                mix(
                    &mut hash,
                    self._ui.get_message_invitation_status().as_str().as_bytes(),
                );
            } else {
                mix(
                    &mut hash,
                    self._ui.get_message_receive_status().as_str().as_bytes(),
                );
                mix(
                    &mut hash,
                    self._ui.get_message_incoming_body().as_str().as_bytes(),
                );
                mix(&mut hash, self._ui.get_message_ack_status().as_str().as_bytes());
            }
        }
        hash
    }

    fn archive_shown_picture(&mut self) {
        if self.feedback_frame_dirty {
            // A pressed card or row is transient. Keep the last clean cached
            // picture rather than archiving that visual state as the page.
            self.feedback_frame_dirty = false;
            return;
        }
        // Join always opens on the empty form, so keep that prepared picture.
        if self.shown_view == 61 {
            return;
        }
        let Some(slot) = Self::picture_slot(self.shown_view) else {
            return;
        };
        #[cfg(feature = "utm-perf-trace")]
        let trace_start = perf_time_us();
        unsafe {
            personal_os_slint_notes_spike::frame_copy::copy_nonoverlapping_ram(
                picture_bytes(slot),
                self.rendered.as_ptr(),
                FRAME_PIXEL_BYTES,
            );
            let meta = &mut *core::ptr::addr_of_mut!(PAGE_PICTURE_META)
                .cast::<PagePictureMeta>()
                .add(slot);
            meta.ready = true;
            meta.stamp = self.shown_stamp;
        }
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|CACHE_STORE|{}|{}|{}", self.shown_view, trace_start, perf_time_us());
        debug_println!("SLINT PAGE STORE {}", self.shown_view);
    }

    fn restore_picture(&mut self, view: i32) -> bool {
        let Some(slot) = Self::picture_slot(view) else {
            return false;
        };
        let stamp = self.view_stamp();
        unsafe {
            let meta = &*core::ptr::addr_of!(PAGE_PICTURE_META)
                .cast::<PagePictureMeta>()
                .add(slot);
            if !meta.ready || meta.stamp != stamp {
                return false;
            }
            #[cfg(feature = "utm-perf-trace")]
            let trace_start = perf_time_us();
            personal_os_slint_notes_spike::frame_copy::copy_nonoverlapping_ram(
                self.rendered.as_mut_ptr(),
                picture_bytes(slot),
                FRAME_PIXEL_BYTES,
            );
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|CACHE_RESTORE|{}|{}|{}", view, trace_start, perf_time_us());
        }
        self.shown_stamp = stamp;
        skia_guest::invalidate_chrome();
        debug_println!("SLINT PAGE RESTORE {}", view);
        true
    }

    fn arm_page_sync(&mut self) {
        self.needs_catch_up = true;
        let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
        if !flag.is_null() {
            unsafe { flag.write_volatile(1) };
        }
    }

    fn load_vault_apps(&mut self) {
        #[cfg(not(any(feature = "desktop-simplex-frontend", feature = "desktop-real-app")))]
        {
        match self.notes_service.borrow().load() {
            Ok(note) => {
                self._ui.set_note(note.into());
                self._ui.set_status("LOADED FROM VAULT".into());
            }
            Err(error) => {
                debug_println!("SLINT NOTES LOAD FAIL {}", error);
                self._ui.set_status("VAULT UNAVAILABLE".into());
            }
        }
        match self.contacts_service.borrow().load() {
            Ok((name, phone)) => {
                self._ui.set_contact_name(name.into());
                self._ui.set_contact_phone(phone.into());
                self._ui.set_contact_status("LOADED FROM VAULT".into());
            }
            Err(error) => {
                debug_println!("SLINT CONTACTS LOAD FAIL {}", error);
                self._ui.set_contact_status("VAULT UNAVAILABLE".into());
            }
        }
        match self.calendar_service.borrow().load() {
            Ok(title) => {
                self._ui.set_calendar_item(title.into());
                self._ui.set_calendar_status("LOADED FROM VAULT".into());
            }
            Err(error) => {
                debug_println!("SLINT CALENDAR LOAD FAIL {}", error);
                self._ui.set_calendar_status("VAULT UNAVAILABLE".into());
            }
        }
        }
        #[cfg(feature = "simplex-agent-ui")]
        match self.messages_service.borrow().load() {
            Ok(snapshot) => {
                self._ui.set_message_service_ready(true);
                self._ui
                    .set_message_has_connections(has_simplex_connection(&snapshot));
                self._ui
                    .set_message_contact_name(contact_name_from_snapshot(&snapshot).into());
                let profile = profile_name_from_snapshot(&snapshot);
                if !profile.is_empty() {
                    self._ui.set_message_profile_name(profile.into());
                }
                let mask = self.messages_service.borrow().archived_mask().unwrap_or(0);
                apply_message_chats(
                    &self._ui,
                    &snapshot,
                    mask,
                    Some(&self.chat_transcripts.borrow()),
                );
                present_peer_photos(
                    &self.messages_service.borrow(),
                    photo_mask_from_snapshot(&snapshot),
                );
                self._ui.set_message_service_state(snapshot.into());
                debug_println!("SLINT MESSAGES LOAD PASS");
            }
            Err(error) => {
                self._ui.set_message_service_ready(false);
                debug_println!("SLINT MESSAGES LOAD FAIL {}", error);
            }
        }
    }

    fn overlay_chat_names(&mut self) {
        if cfg!(feature = "desktop-real-app") { return; }
        if self._ui.get_page() != 6
            || !self._ui.get_message_service_ready()
            || self._ui.get_message_view()
            || self._ui.get_message_join_view()
            || self._ui.get_message_invitation_view()
            || self._ui.get_message_list_tab() != 0
        {
            return;
        }
        let chat_rows = self.sync_chat_rows();
        if chat_rows == 0 {
            return;
        }
        skia_guest::flush_chat_sprites();
        let pressed_row = self.pressed_chat_row(chat_rows);
        skia_guest::blit_chat_rows(
            &mut self.rendered,
            self._ui.get_message_list_scroll(),
            self._ui.get_chat_list_rows_height(),
            chat_rows,
            0xffff,
            0,
            FRAME_HEIGHT as i32,
            pressed_row,
        );
    }

    fn pressed_chat_row(&self, count: usize) -> Option<usize> {
        if !self.nav_gesture.active || !self.nav_gesture.feedback_press { return None; }
        let (width, _) = logical_screen();
        personal_os_slint_notes_spike::chat_drag::pressed_chat_row(
            self.nav_gesture.start_x, self.nav_gesture.start_y, width,
            self._ui.get_top_inset(), self._ui.get_chat_list_rows_height(),
            self._ui.get_message_list_scroll(), count,
        )
    }

    fn publish_copied_frame(&mut self) {
        #[cfg(feature = "utm-perf-trace")]
        let trace_start = perf_time_us();
        let shared_pixels =
            unsafe { core::slice::from_raw_parts_mut(self.pixels.as_ptr(), FRAME_PIXEL_BYTES) };
        assert!(
            unsafe { self.header.as_ref() }.publish_rows(
                shared_pixels,
                &self.rendered,
                0,
                FRAME_HEIGHT
            ),
            "publish stored page"
        );
        core::sync::atomic::fence(Ordering::SeqCst);
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|CACHE_PUBLISH|{}|{}", trace_start, perf_time_us());
    }

    fn store_rendered_picture(&mut self, view: i32, stamp: u64) {
        let Some(slot) = Self::picture_slot(view) else {
            return;
        };
        unsafe {
            personal_os_slint_notes_spike::frame_copy::copy_nonoverlapping_ram(
                picture_bytes(slot),
                self.rendered.as_ptr(),
                FRAME_PIXEL_BYTES,
            );
            let meta = &mut *core::ptr::addr_of_mut!(PAGE_PICTURE_META)
                .cast::<PagePictureMeta>()
                .add(slot);
            meta.ready = true;
            meta.stamp = stamp;
        }
    }

    #[cfg(feature = "simplex-agent-ui")]
    fn ensure_chat_transcripts(&mut self) {
        if self.transcripts_loaded {
            return;
        }
        self.transcripts_loaded = true;
        let count = self._ui.get_message_chats().row_count();
        for index in 0..count.min(CHAT_SLOTS) {
            #[cfg(feature = "utm-perf-trace")]
            let started = perf_time_us();
            let transcript = self.messages_service.borrow().transcript(index as u64);
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|TRANSCRIPT_LOAD|{}|{}|{}|{}", index, started, perf_time_us(), u8::from(transcript.is_ok()));
            if let Ok(saved) = transcript {
                self.chat_transcripts.borrow_mut()[index] = saved;
            }
        }
        let mut read_len = [0usize; CHAT_SLOTS];
        {
            let slots = self.chat_transcripts.borrow();
            for index in 0..CHAT_SLOTS {
                read_len[index] = slots[index].len();
            }
        }
        self.chat_read_len = read_len;
    }

    fn paint_rendered(&mut self, full: bool) -> DamageBand {
        #[cfg(feature = "utm-perf-trace")]
        let trace_paint_start = perf_time_us();
        if full {
            self.window.request_redraw();
            // NewBuffer does not guarantee every line is rewritten; leftover Skia
            // titles (e.g. PRIVATE NOTES) would otherwise flash through on page 16.
            self.rendered.fill(0x05);
        }
        #[cfg(all(feature = "simplex-agent-ui", feature = "utm-photo-worker"))]
        {
            let chat = self._ui.get_message_chat_index().max(0) as usize;
            let owner = {
                let service = self.messages_service.borrow();
                let history = service.history_cache.borrow();
                history.get(chat).and_then(|entry| entry.as_ref().map(|entry| entry.0))
            };
            sync_message_bubbles(&self._ui, &self.chat_photo_loader, &self.photo_cache, owner);
        }
        #[cfg(all(feature = "simplex-agent-ui", not(feature = "utm-photo-worker")))]
        sync_message_bubbles(&self._ui, &self.chat_photo_loader);
        #[cfg(feature = "utm-perf-trace")]
        let trace_before_draw = perf_time_us();
        let mut band = DamageBand::new();
        let mut slint_y = 0i32;
        let mut slint_bottom = 0i32;
        self.window.draw_if_needed(|renderer| {
            if full {
                renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
            }
            let region = renderer.render_by_line(RgbLines {
                output: NonNull::new(self.rendered.as_mut_ptr()).expect("render buffer"),
            });
            if full {
                renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
            }
            let origin_y = region.bounding_box_origin().y;
            let height = region.bounding_box_size().height;
            if origin_y >= 0 && height > 0 {
                slint_y = origin_y;
                slint_bottom = origin_y.saturating_add(height as i32);
                band.cover(origin_y as u32, height);
            }
        });
        #[cfg(feature = "utm-perf-trace")]
        let trace_after_draw = perf_time_us();
        if cfg!(feature = "desktop-real-app") {
            if full { band.cover(0, FRAME_HEIGHT); }
            return band;
        }
        let chat_rows = self.sync_chat_rows();
        let dirty_chats = skia_guest::chat_dirty_mask();
        skia_guest::flush_chat_sprites();
        let page = self._ui.get_page();
        // The passphrase surface must remain the topmost compositor layer,
        // even if a background page changes while the device is locked.
        let locked = self._ui.get_device_lock_screen();
        let (chrome_y, chrome_height) = skia_guest::page_chrome_band(page);
        if !locked && (skia_guest::take_chrome_page(page)
            || full
            || personal_os_slint_notes_spike::frame::bands_intersect(slint_y, slint_bottom, chrome_y, chrome_height))
        {
            if skia_guest::blit_skia_chrome(&mut self.rendered, page) {
                let (y, height) = skia_guest::page_chrome_band(page);
                band.cover(y, height);
            }
        }
        if !locked && chat_rows > 0
            && page == 6
            && self._ui.get_message_service_ready()
            && !self._ui.get_message_view()
            && !self._ui.get_message_join_view()
            && !self._ui.get_message_invitation_view()
            && self._ui.get_message_list_tab() == 0
        {
            let pressed_row = self.pressed_chat_row(chat_rows);
            let (top, bottom) = skia_guest::blit_chat_rows(
                &mut self.rendered,
                self._ui.get_message_list_scroll(),
                self._ui.get_chat_list_rows_height(),
                chat_rows,
                dirty_chats,
                slint_y,
                slint_bottom,
                pressed_row,
            );
            if bottom > top {
                band.cover(top as u32, (bottom - top) as u32);
            }
        }
        let open_chat = if !locked && page == 6 && self._ui.get_message_view() {
            self._ui.get_message_chat_index().max(0)
        } else {
            -1
        };
        let (badge_y, badge_height) = skia_guest::open_chat_badge_band();
        let open_chat_changed = skia_guest::take_open_chat(open_chat);
        if open_chat >= 0 && (open_chat_changed
            || full
            || personal_os_slint_notes_spike::frame::bands_intersect(slint_y, slint_bottom, badge_y, badge_height))
        {
            skia_guest::blit_open_chat_initial(&mut self.rendered, open_chat as usize);
            let (y, height) = skia_guest::open_chat_badge_band();
            band.cover(y, height);
        }
        if full {
            band.cover(0, FRAME_HEIGHT);
        }
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|PAINT_STAGES|{}|{}|{}|{}|{}", self._ui.get_page(), trace_paint_start, trace_before_draw, trace_after_draw, perf_time_us());
        band
    }

    fn prepare_hidden_pages(&mut self) {
        if self.pictures_prepared || self.shown_view != 0 {
            return;
        }
        self.pictures_prepared = true;
        self.archive_shown_picture();
        #[cfg(feature = "simplex-agent-ui")]
        self.ensure_chat_transcripts();
        for page in [8, 6] {
            self._ui.set_page(page);
            self._ui.set_edit_target(0);
            if page == 6 {
                self._ui.set_message_view(false);
                self._ui.set_message_join_view(false);
                self._ui.set_message_invitation_view(false);
                self._ui.set_message_list_tab(0);
            }
            if page == 8 {
                self._ui.set_settings_selected(-1);
            }
            skia_guest::invalidate_chrome();
            self.paint_rendered(true);
            let view = self.current_view();
            self.store_rendered_picture(view, self.view_stamp());
            debug_println!("SLINT PAGE PREPARE {}", view);
        }
        self._ui.set_page(0);
        self._ui.set_edit_target(0);
        if !self.restore_picture(0) {
            debug_println!("SLINT PAGE PREPARE HOME MISSING");
        }
    }

    fn next_background_view(&self) -> Option<i32> {
        const VIEWS: [i32; 18] = [
            2, 9, 10, 1, 3, 5, 7, 12, 11, 13, 101, 102, 4, 14, 63, 64, 61, 62,
        ];
        VIEWS.into_iter().find(|view| !picture_ready(*view))
    }

    fn prepare_next_home_card(&mut self) -> bool {
        if Self::picture_slot(self.shown_view).is_none() {
            return false;
        }
        let next = self.next_background_view();
        let Some(page) = next else {
            return false;
        };
        let saved_page = self._ui.get_page();
        let saved_edit = self._ui.get_edit_target();
        let saved_note_editing = self._ui.get_note_editing();
        let saved_contact_detail = self._ui.get_contact_detail();
        let saved_message_view = self._ui.get_message_view();
        let saved_join_view = self._ui.get_message_join_view();
        let saved_invitation_view = self._ui.get_message_invitation_view();
        let saved_list_tab = self._ui.get_message_list_tab();
        let saved_join_uri = self._ui.get_message_join_uri().to_string();
        let saved_join_status = self._ui.get_message_join_status().to_string();
        let saved_join_requested = self._ui.get_message_join_requested();
        let saved_invitation_uri = self._ui.get_message_invitation_uri().to_string();
        let saved_invitation_status = self._ui.get_message_invitation_status().to_string();
        self.archive_shown_picture();
        self._ui.set_page(match page {
            101 => 1,
            102 => 2,
            61 | 62 | 63 | 64 | 600..=607 => 6,
            other => other,
        });
        self._ui.set_edit_target(if page == 61 { 7 } else { 0 });
        self._ui.set_note_editing(page == 101);
        self._ui.set_contact_detail(page == 102);
        self._ui.set_message_view(false);
        self._ui.set_message_join_view(page == 61);
        self._ui.set_message_invitation_view(page == 62);
        self._ui.set_message_list_tab(match page {
            63 => 1,
            64 => 2,
            _ => 0,
        });
        if page == 61 {
            self._ui.set_message_join_requested(false);
            self._ui.set_message_join_uri("".into());
            self._ui
                .set_message_join_status("PASTE OR TYPE AN INVITATION".into());
        }
        if (600..600 + CHAT_SLOTS as i32).contains(&page) {
            let index = page - 600;
            self._ui.set_message_view(true);
            self._ui.set_edit_target(6);
            self._ui.set_message_chat_index(index);
            self._ui.set_message_body("".into());
            self._ui
                .set_message_receive_status("Up to date".into());
            let name = self
                ._ui
                .get_message_chats()
                .row_data(index as usize)
                .map(|name| name.to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Contact".to_string());
            self._ui.set_message_selected(name.into());
            #[cfg(feature = "simplex-agent-ui")]
            {
                let saved = self.chat_transcripts.borrow()
                    [(index as usize).min(CHAT_SLOTS - 1)]
                .clone();
                self._ui.set_message_transcript(saved.into());
            }
        }
        #[cfg(feature = "simplex-agent-ui")]
        if page == 62 {
            match self.messages_service.borrow().invitation() {
                Ok(uri) if !uri.is_empty() => {
                    present_invitation_qr(&self._ui, uri.as_str());
                    self._ui.set_message_invitation_uri(uri.into());
                    self._ui.set_message_invitation_status(
                        "INVITATION READY · SCAN QR OR COPY LINK".into(),
                    );
                }
                Ok(_) => {
                    self._ui.set_message_invitation_uri("".into());
                    self._ui.set_message_invitation_qr_visible(false);
                    self._ui.set_message_invitation_status(
                        "REQUEST SENT · TAP CHECK TO REFRESH".into(),
                    );
                }
                Err(_) => {
                    self._ui.set_message_invitation_uri("".into());
                    self._ui.set_message_invitation_qr_visible(false);
                    self._ui
                        .set_message_invitation_status("INVITATION SERVICE UNAVAILABLE".into());
                }
            }
        }
        skia_guest::invalidate_chrome();
        self.paint_rendered(true);
        let view = self.current_view();
        self.store_rendered_picture(view, self.view_stamp());
        debug_println!("SLINT PAGE PREPARE {}", view);
        self._ui.set_page(saved_page);
        self._ui.set_edit_target(saved_edit);
        self._ui.set_note_editing(saved_note_editing);
        self._ui.set_contact_detail(saved_contact_detail);
        self._ui.set_message_view(saved_message_view);
        self._ui.set_message_join_view(saved_join_view);
        self._ui.set_message_invitation_view(saved_invitation_view);
        self._ui.set_message_list_tab(saved_list_tab);
        self._ui.set_message_join_uri(saved_join_uri.into());
        self._ui.set_message_join_status(saved_join_status.into());
        self._ui.set_message_join_requested(saved_join_requested);
        self._ui
            .set_message_invitation_uri(saved_invitation_uri.into());
        self._ui
            .set_message_invitation_status(saved_invitation_status.into());
        if !self.restore_picture(self.shown_view) {
            debug_println!("SLINT PAGE PREPARE RESTORE MISS {}", self.shown_view);
        }
        true
    }

    fn publish_frame(&mut self) {
        #[cfg(feature = "desktop-apps-observe")]
        desktop_observation::publish(&self._ui);
        if self.defer_frame {
            self.frame_dirty = true;
            return;
        }
        if self.scanout_hold > 0 {
            self.scanout_hold -= 1;
            return;
        }
        // Paint the initial frame promptly, then initialize text sprites on
        // the next frame. Wallet synchronization may run for hours and must
        // not be a prerequisite for rendering other apps' labels.
        static FIRST_FRAME_PUBLISHED: AtomicBool = AtomicBool::new(false);
        let font_prepared = FIRST_FRAME_PUBLISHED.swap(true, Ordering::AcqRel)
            && skia_guest::prepare_skia_label();
        if font_prepared {
            skia_guest::invalidate_chrome();
        }
        #[cfg(feature = "utm-perf-trace")]
        let perf_start = perf_time_us();
        let view = self.current_view();
        // Keep Home/Receive/Send face in standard language while tip state machine
        // still uses raw wallet-status strings.
        if view == 16 {
            let raw = self._ui.get_wallet_status().to_string();
            let hist_before = self._ui.get_wallet_history().to_string();
            apply_wallet_face(&self._ui, &raw);
            // History can change without the balance or sync title. A partial
            // band then leaves "Waiting for a deposit…" in the list card.
            if self._ui.get_wallet_history().as_str() != hist_before {
                self.wallet_face_tab_dirty = true;
            }
        }
        #[cfg(feature = "simplex-agent-ui")]
        if view == 6 || view == 63 || view == 64 || (600..600 + CHAT_SLOTS as i32).contains(&view) {
            self.ensure_chat_transcripts();
        }
        #[cfg(feature = "utm-perf-trace")]
        let perf_after_transcripts = perf_time_us();
        let mut view_changed = false;
        if view != self.shown_view {
            view_changed = true;
            self.archive_shown_picture();
            if self.restore_picture(view) {
                self.shown_view = view;
                if view == 6 {
                    self.overlay_chat_names();
                }
                self.publish_copied_frame();
                self.arm_page_sync();
                #[cfg(feature = "utm-perf-trace")]
                debug_println!("PERF|CACHED_FRAME|{}|{}|{}|{}", view, perf_start, perf_after_transcripts, perf_time_us());
                return;
            }
            // Uncached views (e.g. NexaWal): partial dirty bands would paint onto the prior page.
            skia_guest::invalidate_chrome();
        }
        // Full redraw through boot phases 1–3 (before sync poll). Partial
        // damage onto an early-cleared FB otherwise leaves a black screen with
        // only the latest strip. Sync polls stay partial so TCG can breathe.
        let face_tab_dirty = self.wallet_face_tab_dirty;
        if face_tab_dirty {
            self.wallet_face_tab_dirty = false;
        }
        let force_full = view_changed
            || matches!(self.wallet_boot_phase, 1 | 2 | 3)
            || face_tab_dirty
            || font_prepared;
        #[cfg(feature = "utm-perf-trace")]
        let perf_before_paint = perf_time_us();
        let mut band = self.paint_rendered(force_full);
        if !self.nav_gesture.active { self.feedback_frame_dirty = false; }
        #[cfg(feature = "utm-perf-trace")]
        let perf_painted = perf_time_us();
        static FIRST_FRAME: AtomicBool = AtomicBool::new(true);
        if FIRST_FRAME.swap(false, Ordering::Relaxed) || force_full {
            band.cover(0, FRAME_HEIGHT);
        }
        let height = band.height();
        if height == 0 {
            self.shown_view = view;
            self.shown_stamp = self.view_stamp();
            return;
        }
        let shared_pixels =
            unsafe { core::slice::from_raw_parts_mut(self.pixels.as_ptr(), FRAME_PIXEL_BYTES) };
        assert!(
            unsafe { self.header.as_ref() }.publish_rows(shared_pixels, &self.rendered, band.y, height),
            "publish Slint frame"
        );
        core::sync::atomic::fence(Ordering::SeqCst);
        self.shown_view = view;
        self.shown_stamp = self.view_stamp();
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|FRAME|{}|{}|{}|{}|{}", perf_start, perf_painted, perf_time_us(), height, u8::from(force_full));
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|FRAME_PREP|{}|{}|{}|{}", view, perf_start, perf_after_transcripts, perf_before_paint);
    }
}

#[derive(Clone, Copy)]
struct PagePictureMeta {
    ready: bool,
    stamp: u64,
}

const PAGE_PICTURE_COUNT: usize = 29;

fn picture_ready(view: i32) -> bool {
    SlintService::picture_slot(view).is_some_and(|slot| unsafe {
        (*core::ptr::addr_of!(PAGE_PICTURE_META)
            .cast::<PagePictureMeta>()
            .add(slot))
        .ready
    })
}

static mut PAGE_PICTURE_META: [PagePictureMeta; PAGE_PICTURE_COUNT] =
    [PagePictureMeta { ready: false, stamp: 0 }; PAGE_PICTURE_COUNT];
static PAGE_PICTURES: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static CATCH_UP_FLAG: AtomicPtr<u32> = AtomicPtr::new(core::ptr::null_mut());
/// Message registers belong to the incoming IPC. A tap drained from the
/// sync pump is not an IPC, so the event is passed through these words.
static POSTED_DEVICE: AtomicU64 = AtomicU64::new(0);
static POSTED_KIND: AtomicU64 = AtomicU64::new(0);
static POSTED_CODE: AtomicU64 = AtomicU64::new(0);
static POSTED_VALUE: AtomicU64 = AtomicU64::new(0);
static POSTED_POSITION: AtomicU64 = AtomicU64::new(0);
static POSTED_ACTIVE: AtomicBool = AtomicBool::new(false);
static WALLET_SETUP_ACTIONS: AtomicU64 = AtomicU64::new(0);
static WALLET_NODE_ACTIONS: AtomicU64 = AtomicU64::new(0);

fn picture_bytes(slot: usize) -> *mut u8 {
    let base = PAGE_PICTURES.load(Ordering::Acquire);
    assert!(!base.is_null() && slot < PAGE_PICTURE_COUNT, "page pictures");
    unsafe { base.add(slot * FRAME_PIXEL_BYTES) }
}

struct DamageBand {
    y: u32,
    bottom: u32,
}

impl DamageBand {
    fn new() -> Self {
        Self { y: 0, bottom: 0 }
    }

    fn cover(&mut self, y: u32, height: u32) {
        if height == 0 {
            return;
        }
        let bottom = y.saturating_add(height).min(FRAME_HEIGHT);
        let y = y.min(bottom);
        if self.bottom <= self.y {
            self.y = y;
            self.bottom = bottom;
        } else {
            self.y = self.y.min(y);
            self.bottom = self.bottom.max(bottom);
        }
    }

    fn height(&self) -> u32 {
        self.bottom.saturating_sub(self.y)
    }
}

#[protection_domain]
fn init() -> SlintService {
    debug_println!("SLINT|FRAME_COPY_RAM_V1");
    let heap = memory_region_symbol!(slint_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(heap.as_ptr(), HEAP_BYTES))
        .expect("slint heap initialized once");
    let pictures = memory_region_symbol!(page_pictures: *mut u8);
    PAGE_PICTURES.store(pictures.as_ptr(), Ordering::Release);
    let surface = memory_region_symbol!(slint_frame: *mut u8);
    let focus = memory_region_symbol!(ui_focus: *mut u32);
    let pointer = memory_region_symbol!(ui_pointer: *mut UiPointerState);
    let transfer = memory_region_symbol!(notes_transfer: *mut BlobTransferPage);
    let contacts_transfer = memory_region_symbol!(contacts_transfer: *mut BlobTransferPage);
    let calendar_transfer = memory_region_symbol!(calendar_transfer: *mut BlobTransferPage);
    let messages_transfer = memory_region_symbol!(messages_transfer: *mut BlobTransferPage);
    let wallet_transfer = memory_region_symbol!(wallet_transfer: *mut BlobTransferPage);
    let surface_ptr = surface.as_ptr();
    unsafe {
        core::ptr::write(
            surface_ptr.cast::<FrameSurfaceHeader>(),
            FrameSurfaceHeader::new(),
        );
    }
    let pixels = unsafe { surface_ptr.add(core::mem::size_of::<FrameSurfaceHeader>()) };

    // Clear the shared FB up front. An orange "beacon" band used to prove the
    // loader owned the surface, but when boot notify was delayed it stayed on
    // screen as a solid strip above NexaWal.
    {
        let pix = unsafe { core::slice::from_raw_parts_mut(pixels, FRAME_PIXEL_BYTES) };
        pix.fill(0x05);
        let header = unsafe { &*surface_ptr.cast::<FrameSurfaceHeader>() };
        let odd = header.revision.load(Ordering::Relaxed).wrapping_add(1) | 1;
        header.revision.store(odd, Ordering::Release);
        header.damage_y.store(0, Ordering::Relaxed);
        header.damage_height.store(FRAME_HEIGHT, Ordering::Relaxed);
        header.revision.store(odd.wrapping_add(1), Ordering::Release);
        debug_println!("SLINT EARLY FB CLEAR");
    }

    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    personal_os_slint_notes_spike::guest_platform::install(&window);
    window
        .window()
        .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
            scale_factor: SCALE,
        });
    window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    let ui = NotesWindow::new().expect("create Slint workspace suite");
    ui.set_desktop_mode(cfg!(feature = "desktop-real-app"));
    #[cfg(feature = "utm-device-lock")]
    let device_lock = device_lock_utm::initialize(&ui, memory_region_symbol!(device_lock_transfer: *mut BlobTransferPage));
    ui.set_wallet_node("".into());
    ui.set_wallet_node_status("Loading saved node…".into());
    personal_os_slint_notes_spike::suite::wire(&ui);
    #[cfg(feature = "simplex-agent-ui")]
    ui.set_show_preview_privacy_controls(false);
    let notes_service = NotesServiceClient {
        transfer: NonNull::new(transfer.as_ptr()).expect("Notes transfer page"),
    };
    let notes_service = Rc::new(RefCell::new(notes_service));
    // Defer vault loads until after the first GPU frame. A hung notes/messages
    // PPC previously left virtio-gpu at the 640x480 "Display output is not
    // active" placeholder for the entire boot — blocking wallet evidence.
    ui.set_status("LOADING…".into());

    let contacts_service = ContactsServiceClient {
        transfer: NonNull::new(contacts_transfer.as_ptr()).expect("Contacts transfer page"),
    };
    let contacts_service = Rc::new(RefCell::new(contacts_service));
    ui.set_contact_status("LOADING…".into());
    let weak_ui = ui.as_weak();
    let save_contacts = contacts_service.clone();
    ui.on_save_contact(move |name, phone| {
        if let Some(ui) = weak_ui.upgrade() {
            let status = match save_contacts
                .borrow_mut()
                .save_and_verify(name.as_str(), phone.as_str())
            {
                Ok(()) => "SAVED THROUGH VAULT",
                Err(error) => {
                    debug_println!("SLINT CONTACTS SAVE FAIL {}", error);
                    "VAULT SAVE FAILED"
                }
            };
            ui.set_contact_status(status.into());
        }
    });
    let calendar_service = CalendarServiceClient {
        transfer: NonNull::new(calendar_transfer.as_ptr()).expect("Calendar transfer page"),
    };
    let calendar_service = Rc::new(RefCell::new(calendar_service));
    ui.set_calendar_status("LOADING…".into());

    let messages_service = Rc::new(RefCell::new(MessagesServiceClient {
        transfer: NonNull::new(messages_transfer.as_ptr()).expect("Messages transfer page"),
        showing_address: Cell::new(false),
        #[cfg(feature = "simplex-agent-ui")]
        history_cache: RefCell::new(core::array::from_fn(|_| None)),
        #[cfg(feature = "utm-transcript-worker")]
        transcript_mailbox: unsafe { memory_region_symbol!(transcript_mailbox: *mut personal_os_slint_notes_spike::transcript_ipc::TranscriptMailbox).as_ref() },
        #[cfg(feature = "utm-transcript-worker")]
        transcript_queue: RefCell::new(personal_os_slint_notes_spike::transcript_queue::Queue::new()),
        #[cfg(feature = "utm-transcript-worker")]
        transcript_ticket: Cell::new(None),
        #[cfg(feature = "utm-transcript-worker")]
        transcript_owners: RefCell::new([0; CHAT_SLOTS]),
        #[cfg(feature = "utm-transcript-worker")]
        force_chat_refresh: Cell::new(false),
        #[cfg(feature = "utm-transcript-worker")]
        live_receive_mailbox: unsafe { memory_region_symbol!(live_receive_mailbox: *mut personal_os_slint_notes_spike::photo_mailbox::Mailbox<1, 4096>).as_ref() },
        #[cfg(feature = "utm-transcript-worker")]
        pending_live_receive: RefCell::new(None),
        #[cfg(feature = "utm-transcript-worker")]
        live_receive_requested: Cell::new(false),
        #[cfg(feature = "utm-transcript-worker")]
        live_receive_start_requested: Cell::new(false),
        #[cfg(feature = "utm-transcript-worker")]
        live_receive_inflight_start: Cell::new(false),
        #[cfg(feature = "utm-transcript-worker")]
        navigation_mailbox: unsafe { memory_region_symbol!(navigation_mailbox: *mut personal_os_slint_notes_spike::photo_mailbox::Mailbox<8, 1>).as_ref() },
        #[cfg(feature = "utm-transcript-worker")]
        navigation_queue: RefCell::new(personal_os_slint_notes_spike::navigation_selection::Selection::new()),
        #[cfg(feature = "utm-transcript-worker")]
        navigation_ticket: Cell::new(None),
        #[cfg(feature = "utm-transcript-worker")]
        navigation_legacy_photo_pending: Cell::new(false),
        #[cfg(feature = "utm-transcript-worker")]
        snapshot_mailbox: unsafe { memory_region_symbol!(snapshot_mailbox: *mut personal_os_slint_notes_spike::photo_mailbox::Mailbox<1, 4096>).as_ref() },
        #[cfg(feature = "utm-transcript-worker")]
        snapshot_refresh: RefCell::new(personal_os_slint_notes_spike::snapshot_refresh::Refresh::new()),
        #[cfg(feature = "utm-transcript-worker")]
        snapshot_ticket: Cell::new(None),
    }));
    let chat_transcripts: Rc<RefCell<[String; CHAT_SLOTS]>> =
        Rc::new(RefCell::new(core::array::from_fn(|_| String::new())));
    ui.set_message_service_ready(false);
    let chat_drafts: Rc<RefCell<[String; CHAT_SLOTS]>> =
        Rc::new(RefCell::new(core::array::from_fn(|_| String::new())));
    #[cfg(feature = "simplex-agent-ui")]
    let chat_photo_loader = Rc::new(RefCell::new(personal_os_slint_notes_spike::chat_photo_loader::PhotoLoader::default()));
    #[cfg(feature = "utm-photo-worker")]
    let photo_cache = Rc::new(RefCell::new(personal_os_slint_notes_spike::photo_cache::PhotoCache::<slint::Image>::new(4 * 1024 * 1024)));
    #[cfg(feature = "simplex-agent-ui")]
    {
        let loader = chat_photo_loader.clone();
        ui.on_set_chat_photo_visibility(move |row, visible| {
            if row >= 0 {
                loader.borrow_mut().set_visible(row as usize, visible);
                #[cfg(feature = "utm-perf-trace")]
                debug_println!("PERF|PHOTO_VIS|{}|{}|{}", perf_time_us(), row, u8::from(visible));
            }
        });
    }
    let weak_ui = ui.as_weak();
    let close_drafts = chat_drafts.clone();
    ui.on_close_chat(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let index = ui.get_message_chat_index().max(0) as usize;
            close_drafts.borrow_mut()[index.min(CHAT_SLOTS - 1)] = ui.get_message_body().to_string();
            ui.set_message_reply_to("".into());
            debug_println!("SLINT SIMPLEX DRAFT SAVE {}", index.min(CHAT_SLOTS - 1));
        }
    });
    let weak_ui = ui.as_weak();
    let filter_transcripts = chat_transcripts.clone();
    let filter_service = messages_service.clone();
    ui.on_filter_message_chats(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let snapshot = ui.get_message_service_state().to_string();
            if snapshot.is_empty() {
                return;
            }
            let mask = filter_service.borrow().archived_mask().unwrap_or(0);
            apply_message_chats(&ui, &snapshot, mask, Some(&filter_transcripts.borrow()));
            debug_println!("SLINT SIMPLEX SEARCH {}", ui.get_message_search().as_str());
        }
    });
    let weak_ui = ui.as_weak();
    ui.on_copy_invitation(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let uri = ui.get_message_invitation_uri().to_string();
            if uri.is_empty() {
                ui.set_message_invitation_status("NOTHING TO COPY · TAP CHECK".into());
                return;
            }
            let escaped = escape_copy(&uri);
            debug_println!("NEXATRODE_COPY|BEGIN|{}", escaped.len());
            let mut start = 0;
            while start < escaped.len() {
                let mut end = (start + 80).min(escaped.len());
                while end > start && !escaped.is_char_boundary(end) {
                    end -= 1;
                }
                debug_println!("NEXATRODE_COPY|DATA|{}", &escaped[start..end]);
                start = end;
            }
            debug_println!("NEXATRODE_COPY|END");
            ui.set_message_invitation_status("COPIED · PASTE ON THE OTHER DEVICE".into());
            debug_println!("SLINT SIMPLEX INVITATION COPY LEN {}", uri.len());
        }
    });
    let weak_ui = ui.as_weak();
    let group_service = messages_service.clone();
    ui.on_create_group(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let name = ui.get_message_group_name().to_string();
            let recipient = ui.get_message_group_recipient();
            let current_name = usize::try_from(recipient).ok()
                .and_then(|index| ui.get_message_chats().row_data(index));
            if current_name.as_ref().map(|name| name.as_str()) != Some(ui.get_message_group_recipient_name().as_str()) {
                ui.set_message_group_recipient(-1);
                ui.set_message_group_status("Contact list changed; choose a contact again".into());
                return;
            }
            let contact = usize::try_from(recipient).ok()
                .and_then(|index| ui.get_message_chat_index_map().row_data(index))
                .and_then(|index| usize::try_from(index).ok());
            let Some(contact) = contact else {
                ui.set_message_group_status("Choose a contact to invite".into());
                return;
            };
            match group_service.borrow_mut().create_group(name.trim(), contact) {
                Ok(()) => { ui.set_message_group_status("Group creation started".into()); ui.set_edit_target(0); },
                Err(error) => { ui.set_message_group_status(error.into()); debug_println!("SLINT SIMPLEX CREATE GROUP FAIL {}", error); },
            }
        }
    });
    let weak_ui = ui.as_weak();
    let profile_service = messages_service.clone();
    ui.on_save_profile(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let name = ui.get_message_profile_name().to_string();
            match profile_service.borrow_mut().set_profile(name.trim()) {
                Ok(()) => {
                    ui.set_message_profile_name(name.trim().into());
                    debug_println!("SLINT SIMPLEX PROFILE {}", name.trim().len());
                }
                Err(error) => debug_println!("SLINT SIMPLEX PROFILE FAIL {}", error),
            }
        }
    });
    let weak_ui = ui.as_weak();
    let invitation_service = messages_service.clone();
    ui.on_create_invitation(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let result = {
                let service = invitation_service.borrow();
                service
                    .create_invitation()
                    .and_then(|()| service.invitation())
            };
            match result {
                Ok(uri) if !uri.is_empty() => {
                    log_simplex_link(uri.as_str());
                    debug_println!("SLINT SIMPLEX INVITATION READY LEN {}", uri.len());
                    present_invitation_qr(&ui, uri.as_str());
                    ui.set_message_invitation_uri(uri.into());
                    ui.set_message_invitation_status(
                        "INVITATION READY · SCAN QR OR COPY LINK".into(),
                    );
                }
                Ok(_) => {
                    ui.set_message_invitation_uri("".into());
                    ui.set_message_invitation_qr_visible(false);
                    ui.set_message_invitation_status("REQUEST SENT · TAP CHECK TO REFRESH".into());
                    debug_println!("SLINT SIMPLEX INVITATION PENDING");
                }
                Err(error) => {
                    ui.set_message_invitation_uri("".into());
                    ui.set_message_invitation_qr_visible(false);
                    ui.set_message_invitation_status("INVITATION SERVICE UNAVAILABLE".into());
                    debug_println!("SLINT SIMPLEX INVITATION FAIL {}", error);
                }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let invitation_service = messages_service.clone();
    ui.on_create_address(move || {
        if let Some(ui) = weak_ui.upgrade() {
            ui.set_message_address_view(true);
            ui.set_message_invitation_view(true);
            let result = {
                let service = invitation_service.borrow();
                service.create_address().and_then(|()| service.address())
            };
            match result {
                Ok(uri) if !uri.is_empty() => {
                    log_simplex_link(uri.as_str());
                    debug_println!("SLINT SIMPLEX ADDRESS READY LEN {}", uri.len());
                    present_invitation_qr(&ui, uri.as_str());
                    ui.set_message_invitation_uri(uri.into());
                    ui.set_message_invitation_status(
                        "ADDRESS READY · REUSABLE LINK".into(),
                    );
                }
                Ok(_) => {
                    ui.set_message_invitation_uri("".into());
                    ui.set_message_invitation_qr_visible(false);
                    ui.set_message_invitation_status(
                        "CREATING ADDRESS · TAP CHECK".into(),
                    );
                    debug_println!("SLINT SIMPLEX ADDRESS PENDING");
                }
                Err(error) => {
                    ui.set_message_invitation_qr_visible(false);
                    ui.set_message_invitation_status("ADDRESS SERVICE UNAVAILABLE".into());
                    debug_println!("SLINT SIMPLEX ADDRESS FAIL {}", error);
                }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let invitation_service = messages_service.clone();
    ui.on_refresh_invitation(move || {
        if let Some(ui) = weak_ui.upgrade() {
            if invitation_service.borrow().showing_address.get() {
                match invitation_service.borrow().address() {
                    Ok(uri) if !uri.is_empty() => {
                        log_simplex_link(uri.as_str());
                        debug_println!("SLINT SIMPLEX ADDRESS READY LEN {}", uri.len());
                        present_invitation_qr(&ui, uri.as_str());
                        ui.set_message_invitation_uri(uri.into());
                        ui.set_message_invitation_status(
                            "ADDRESS READY · REUSABLE LINK".into(),
                        );
                    }
                    Ok(_) => {
                        ui.set_message_invitation_uri("".into());
                        ui.set_message_invitation_qr_visible(false);
                        ui.set_message_invitation_status(
                            "STILL CREATING ADDRESS · TAP CHECK AGAIN".into(),
                        );
                    }
                    Err(error) => {
                        ui.set_message_invitation_qr_visible(false);
                        ui.set_message_invitation_status("ADDRESS SERVICE UNAVAILABLE".into());
                        debug_println!("SLINT SIMPLEX ADDRESS REFRESH FAIL {}", error);
                    }
                }
                return;
            }
            match invitation_service.borrow().invitation() {
                Ok(uri) if !uri.is_empty() => {
                    log_simplex_link(uri.as_str());
                    debug_println!("SLINT SIMPLEX INVITATION READY LEN {}", uri.len());
                    present_invitation_qr(&ui, uri.as_str());
                    ui.set_message_invitation_uri(uri.into());
                    ui.set_message_invitation_status(
                        "INVITATION READY · SCAN QR OR COPY LINK".into(),
                    );
                }
                Ok(_) => {
                    ui.set_message_invitation_uri("".into());
                    ui.set_message_invitation_qr_visible(false);
                    ui.set_message_invitation_status("STILL CREATING · TAP CHECK AGAIN".into());
                }
                Err(error) => {
                    ui.set_message_invitation_qr_visible(false);
                    ui.set_message_invitation_status("INVITATION SERVICE UNAVAILABLE".into());
                    debug_println!("SLINT SIMPLEX INVITATION REFRESH FAIL {}", error);
                }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let join_service = messages_service.clone();
    ui.on_submit_join_invitation(move |uri| {
        if let Some(ui) = weak_ui.upgrade() {
            let uri = uri.as_str();
            ui.set_message_join_status("Joining…".into());
            match join_service.borrow_mut().join_invitation(uri) {
                Ok(()) => {
                    ui.set_message_join_requested(true);
                    ui.set_message_join_status("REQUEST ACCEPTED · CHECKING AGENT".into());
                }
                Err(_) => {
                    ui.set_message_join_requested(false);
                    ui.set_message_join_status("INVALID LINK OR SERVICE UNAVAILABLE".into());
                }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let join_service = messages_service.clone();
    ui.on_check_join_status(move || {
        if let Some(ui) = weak_ui.upgrade() {
            match join_service.borrow().join_status() {
                Ok((0, _)) => ui.set_message_join_status("PASTE OR TYPE AN INVITATION".into()),
                Ok((1, _)) => ui.set_message_join_status("JOIN REQUEST IN PROGRESS".into()),
                Ok((2, _)) => ui.set_message_join_status("JOIN ACCEPTED · WAITING FOR PEER".into()),
                Ok((3, _)) => ui.set_message_join_status("LINK REJECTED · CHECK INVITATION".into()),
                Ok((_, _)) => ui.set_message_join_status("AGENT COULD NOT JOIN LINK".into()),
                Err(_) => ui.set_message_join_status("AGENT STATUS UNAVAILABLE".into()),
            }
        }
    });
    let weak_ui = ui.as_weak();
    let select_service = messages_service.clone();
    let select_transcripts = chat_transcripts.clone();
    let select_drafts = chat_drafts.clone();
    ui.on_open_chat(move |index| {
        #[cfg(feature = "utm-transcript-worker")]
        {
            if index < 0 || !select_service.borrow().request_navigation(index as usize) {
                if let Some(ui) = weak_ui.upgrade() { ui.set_message_receive_status("Couldn’t open conversation. Try again.".into()); }
                return false;
            }
            if let Some(ui) = weak_ui.upgrade() {
                let index = index as usize;
                select_service.borrow().transcript_queue.borrow_mut().prioritize(index);
                let saved = select_service.borrow().transcript(index as u64)
                    .unwrap_or_else(|_| select_transcripts.borrow()[index].clone());
                select_transcripts.borrow_mut()[index] = saved.clone();
                ui.set_message_chat_index(index as i32);
                ui.set_message_transcript(saved.into());
                ui.set_message_body(select_drafts.borrow()[index].clone().into());
                ui.set_message_image_visible(false);
                ui.set_message_incoming_image_visible(false);
                ui.set_message_group_invite(false);
                ui.set_message_target_ready(false);
                ui.set_message_receive_status("Opening conversation…".into());
                #[cfg(feature = "utm-perf-trace")]
                debug_println!("PERF|NAV_VIEW_REQUEST|{}|{}", index, perf_time_us());
            }
            return true;
        }
        #[cfg(not(feature = "utm-transcript-worker"))]
        {
        #[cfg(feature = "utm-perf-trace")]
        let trace_open_start = perf_time_us();
        let index = index as u64;
        if let Err(error) = select_service.borrow().select_chat(index) {
            debug_println!("SLINT SIMPLEX SELECT CHAT FAIL {} {}", index, error);
            return false;
        }
        debug_println!("SLINT SIMPLEX SELECT CHAT {}", index);
        #[cfg(feature = "utm-perf-trace")]
        let trace_selected = perf_time_us();
        if let Some(ui) = weak_ui.upgrade() {
            if ui.get_message_view() {
                let previous = ui.get_message_chat_index().max(0) as usize;
                select_drafts.borrow_mut()[previous.min(CHAT_SLOTS - 1)] =
                    ui.get_message_body().to_string();
            }
            #[cfg(feature = "utm-transcript-worker")]
            select_service.borrow().transcript_queue.borrow_mut().prioritize(index as usize);
            let result = select_service.borrow().transcript(index);
            #[cfg(feature = "utm-transcript-worker")]
            let loading = result.is_err();
            let saved = match result {
                Ok(saved) => saved,
                Err(_) => select_transcripts.borrow()[(index as usize).min(CHAT_SLOTS - 1)].clone(),
            };
            #[cfg(feature = "utm-perf-trace")]
            let trace_transcript = perf_time_us();
            select_transcripts.borrow_mut()[(index as usize).min(CHAT_SLOTS - 1)] = saved.clone();
            let draft = select_drafts.borrow()[(index as usize).min(CHAT_SLOTS - 1)].clone();
            ui.set_message_chat_index(index as i32);
            ui.set_message_view(true);
            ui.set_message_transcript(saved.into());
            #[cfg(feature = "utm-transcript-worker")]
            if loading { ui.set_message_receive_status("Loading conversation…".into()); }
            ui.set_message_body(draft.into());
            present_message_image(&ui, &select_service.borrow());
            present_group_invite(&ui, &select_service.borrow());
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|CHAT_OPEN|{}|{}|{}|{}|{}", index, trace_open_start, trace_selected, trace_transcript, perf_time_us());
        }
        true
        }
    });
    let weak_ui = ui.as_weak();
    let delete_service = messages_service.clone();
    let delete_transcripts = chat_transcripts.clone();
    #[cfg(feature = "utm-photo-worker")]
    let deleted_photo_cache = photo_cache.clone();
    ui.on_delete_chat(move |index| {
        let index = if index < 0 { 0 } else { index as u64 };
        debug_println!("SLINT SIMPLEX DELETE TRY {}", index);
        let deleted = delete_service.borrow_mut().delete_chat(index);
        if let Some(ui) = weak_ui.upgrade() {
            match deleted {
                Ok(()) => {
                    #[cfg(feature = "utm-photo-worker")]
                    deleted_photo_cache.borrow_mut().clear();
                    if ui.get_message_chat_index() == index as i32 {
                        ui.set_message_transcript("".into());
                    }
                    delete_transcripts.borrow_mut()[(index as usize).min(CHAT_SLOTS - 1)].clear();
                    if let Ok(snapshot) = delete_service.borrow().load() {
                        #[cfg(not(feature = "utm-transcript-worker"))]
                        {
                        ui.set_message_has_connections(has_simplex_connection(&snapshot));
                        let mask = delete_service.borrow().archived_mask().unwrap_or(0);
                        apply_message_chats(&ui, &snapshot, mask, Some(&delete_transcripts.borrow()));
                        ui.set_message_service_state(snapshot.into());
                        }
                        #[cfg(feature = "utm-transcript-worker")]
                        { let _ = snapshot; delete_service.borrow().force_chat_refresh.set(true); }
                    }
                    ui.set_message_send_status("Contact deleted".into());
                    debug_println!("SLINT SIMPLEX DELETE {}", index);
                }
                Err(error) => {
                    ui.set_message_send_status("Delete failed".into());
                    debug_println!("SLINT SIMPLEX DELETE FAIL {}", error);
                }
            }
        } else {
            debug_println!("SLINT SIMPLEX DELETE NO UI {}", index);
        }
    });
    let weak_ui = ui.as_weak();
    let archive_service = messages_service.clone();
    let archive_transcripts = chat_transcripts.clone();
    ui.on_archive_chat(move |index, archived| {
        let index = if index < 0 { 0 } else { index as u64 };
        debug_println!(
            "SLINT SIMPLEX ARCHIVE TRY {} {}",
            index,
            u8::from(archived)
        );
        let archived_result = archive_service.borrow_mut().archive_chat(index, archived);
        if let Some(ui) = weak_ui.upgrade() {
            match archived_result {
                Ok(mask) => {
                    if let Ok(snapshot) = archive_service.borrow().load() {
                        #[cfg(not(feature = "utm-transcript-worker"))]
                        {
                        apply_message_chats(
                            &ui,
                            &snapshot,
                            mask,
                            Some(&archive_transcripts.borrow()),
                        );
                        ui.set_message_service_state(snapshot.into());
                        }
                        #[cfg(feature = "utm-transcript-worker")]
                        { let _ = snapshot; archive_service.borrow().force_chat_refresh.set(true); }
                    } else {
                        apply_message_chats(
                            &ui,
                            ui.get_message_service_state().as_str(),
                            mask,
                            Some(&archive_transcripts.borrow()),
                        );
                    }
                    debug_println!("SLINT SIMPLEX ARCHIVE {} {}", index, u8::from(archived));
                }
                Err(error) => debug_println!("SLINT SIMPLEX ARCHIVE FAIL {}", error),
            }
        } else {
            debug_println!("SLINT SIMPLEX ARCHIVE NO UI {}", index);
        }
    });
    let weak_ui = ui.as_weak();
    let file_service = messages_service.clone();
    ui.on_inspect_bubble(move |bubble, file| {
        if let Some(ui) = weak_ui.upgrade() {
            if let Ok(id) = file.parse::<u64>() {
                let chat = ui.get_message_chat_index().max(0) as u64;
                let bytes = file_service.borrow().message_attachment(chat, id);
                let contents = match bytes {
                    Some(bytes) => {
                        debug_println!("SLINT SIMPLEX FILE OPEN {} {}", id, bytes.len());
                        attachment_display(bytes)
                    }
                    None => "File unavailable".into(),
                };
                ui.set_message_file_name(bubble);
                ui.set_message_file_body(contents.into());
                ui.set_message_file_open(true);
            } else if let Some((name, body)) = file_bubble(bubble.as_str()) {
                ui.set_message_file_name(name.into());
                ui.set_message_file_body(body.into());
                ui.set_message_file_open(true);
                debug_println!("SLINT SIMPLEX FILE OPEN {}", name);
            } else {
                ui.set_message_file_open(false);
            }
        }
    });
    let weak_ui = ui.as_weak();
    let library_save_service = messages_service.clone();
    ui.on_save_chat_photo(move |value| {
        if let Some(ui) = weak_ui.upgrade() {
            let Some(id) = value.parse::<u64>().ok().filter(|id| *id != 0) else {
                ui.set_message_send_status("Photo unavailable".into()); return;
            };
            let chat = ui.get_message_chat_index().max(0) as usize;
            let mut service = library_save_service.borrow_mut();
            let id = service.transcript(chat as u64).ok().and_then(|transcript|
                personal_os_rust_server::messages::current_photo_save_id(&transcript, id)
            );
            let Some(id) = id else {
                ui.set_message_send_status("Photo unavailable".into()); return;
            };
            let first = service.read_attachment_part(chat as u64, id, 0)
                .and_then(|_| unsafe { service.transfer.as_ref() }.payload());
            let suffix = match first {
                Some(bytes) if bytes.starts_with(&[0xff, 0xd8]) => ".jpg",
                Some(bytes) if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => ".png",
                Some(bytes) if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => ".gif",
                Some(bytes) if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => ".webp",
                _ => "",
            };
            let name = alloc::format!("Photo-{id}{suffix}");
            match service.save_to_library(chat, id, &name) {
                Ok(()) => ui.set_message_send_status("Saved to Files".into()),
                Err(message) => ui.set_message_send_status(message.into()),
            }
        }
    });
    let weak_ui = ui.as_weak();
    let library_save_file_service = messages_service.clone();
    ui.on_save_chat_file(move |value, name| {
        if let Some(ui) = weak_ui.upgrade() {
            let Some(id) = value.parse::<u64>().ok().filter(|id| *id != 0) else {
                ui.set_message_send_status("File unavailable".into()); return;
            };
            let chat = ui.get_message_chat_index().max(0) as usize;
            match library_save_file_service.borrow_mut().save_to_library(chat, id, name.as_str()) {
                Ok(()) => ui.set_message_send_status("Saved to Files".into()),
                Err(message) => ui.set_message_send_status(message.into()),
            }
        }
    });
    let saved_files = Rc::new(RefCell::new(Vec::<(u64, usize, String)>::new()));
    let weak_ui = ui.as_weak();
    let file_list_service = messages_service.clone();
    let file_list_entries = saved_files.clone();
    ui.on_refresh_files(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let service = file_list_service.borrow();
            let mut files = file_list_entries.borrow_mut();
            files.clear();
            let mut cursor = 0;
            let mut truncated = false;
            loop {
                match service.next_library_file(cursor) {
                    Ok(Some((next, id, len, name))) => {
                        cursor = next;
                        if files.len() == 64 { files.remove(0); truncated = true; }
                        files.push((id, len, name));
                    }
                    Ok(None) => break,
                    Err(_) => {
                        files.clear();
                        ui.set_files_names(ModelRc::new(VecModel::from(Vec::<slint::SharedString>::new())));
                        ui.set_files_preview_open(false);
                        ui.set_files_status("Could not list Files".into());
                        return;
                    }
                }
            }
            files.reverse();
            let names: Vec<slint::SharedString> = files.iter().map(|(_, len, name)| alloc::format!("{}  ·  {} B", name, len).into()).collect();
            ui.set_files_names(ModelRc::new(VecModel::from(names)));
            ui.set_files_preview_open(false);
            ui.set_files_status(if files.is_empty() { "Save a photo from a chat" } else if truncated { "Showing latest 64 files" } else { "Saved on device" }.into());
        }
    });
    let weak_ui = ui.as_weak();
    let file_open_service = messages_service.clone();
    let file_open_entries = saved_files.clone();
    let file_selected = Rc::new(RefCell::new(None::<u64>));
    let file_open_selected = file_selected.clone();
    ui.on_open_saved_file(move |index| {
        if let Some(ui) = weak_ui.upgrade() {
            let entry = usize::try_from(index).ok().and_then(|i| file_open_entries.borrow().get(i).cloned());
            let Some((id, len, name)) = entry else { ui.set_files_status("File unavailable".into()); return; };
            let Some(bytes) = file_open_service.borrow().read_library_file(id, len) else { ui.set_files_status("Could not read file".into()); return; };
            let preview = decode_jpeg_preview(&bytes);
            ui.set_files_preview_name(name.into());
            ui.set_files_preview_text(if preview.is_some() { "" } else { "Preview unavailable" }.into());
            ui.set_files_preview(preview.unwrap_or_default());
            ui.set_files_preview_open(true);
            ui.set_files_delete_confirm(false);
            *file_open_selected.borrow_mut() = Some(id);
        }
    });
    let weak_ui = ui.as_weak();
    let file_delete_service = messages_service.clone();
    ui.on_delete_saved_file(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let Some(id) = *file_selected.borrow() else { return; };
            let result = file_delete_service.borrow().delete_library_file(id);
            match result {
                Ok(()) => { ui.invoke_refresh_files(); ui.set_files_status("File deleted".into()); }
                Err(_) => { ui.set_files_delete_confirm(false); ui.set_files_preview_text("Could not delete file".into()); }
            }
        }
    });
    #[cfg(feature = "utm-file-import-test")]
    {
        ui.set_files_test_import_enabled(true);
        let weak_ui = ui.as_weak();
        let import_service = messages_service.clone();
        ui.on_import_test_file(move || {
            if let Some(ui) = weak_ui.upgrade() {
                let result = import_service.borrow_mut().import_test_file(
                    option_env!("NEXATRODE_TEST_IMPORT_NAME").unwrap_or("UTM-import.bin"),
                    include_bytes!(env!("NEXATRODE_TEST_IMPORT")));
                match result {
                    Ok(()) => { ui.invoke_refresh_files(); ui.set_files_status("Test file imported".into()); }
                    Err(message) => ui.set_files_status(message.into()),
                }
            }
        });
    }
    let saved_photo_choices = Rc::new(RefCell::new(Vec::<(usize, u64, String)>::new()));
    let weak_ui = ui.as_weak();
    let picker_choices = saved_photo_choices.clone();
    let picker_service = messages_service.clone();
    ui.on_attach_picture(move |photos_only| {
        if let Some(ui) = weak_ui.upgrade() {
            if !ui.get_message_target_ready() { return; }
            let mut choices = picker_choices.borrow_mut();
            choices.clear();
            let mut labels = Vec::new();
            let mut images = Vec::new();
            let service = picker_service.borrow();
            let mut entries = Vec::new();
            let mut cursor = 0;
            loop {
                match service.next_library_file(cursor) {
                    Ok(Some((next, id, len, name))) => {
                        cursor = next;
                        if entries.len() == 64 { entries.remove(0); }
                        entries.push((id, len, name));
                    }
                    Ok(None) => break,
                    Err(_) => { ui.set_message_send_status("Could not list Files".into()); break; }
                }
            }
            for (id, len, name) in entries.into_iter().rev() {
                let Some(bytes) = service.read_library_file(id, len) else { continue; };
                let is_photo = bytes.starts_with(&[0xff, 0xd8]);
                if photos_only && !is_photo { continue; }
                choices.push((usize::MAX, id, name.clone()));
                images.push(if is_photo { decode_jpeg_thumbnail(&bytes).unwrap_or_default() } else { Default::default() });
                labels.push(slint::SharedString::from(name));
            }
            ui.set_message_photo_picker_labels(ModelRc::new(VecModel::from(labels)));
            ui.set_message_photo_picker_images(ModelRc::new(VecModel::from(images)));
            ui.set_message_photo_picker_open(true);
        }
    });
    let weak_ui = ui.as_weak();
    let attach_service = messages_service.clone();
    let retry_service = messages_service.clone();
    let retry_ui = ui.as_weak();
    ui.on_retry_attachment(move |value, name| {
        let Some(ui) = retry_ui.upgrade() else { return; };
        if !ui.get_message_target_ready() { return; }
        let chat = ui.get_message_chat_index().max(0) as usize;
        let sent = value.parse::<u64>().ok().is_some_and(|id|
            retry_service.borrow_mut().send_saved_photo(chat, id, name.as_str()).is_ok());
        ui.set_message_send_status(if sent { "Sending…" } else { "Could not retry attachment" }.into());
    });
    let picker_choices = saved_photo_choices.clone();
    ui.on_pick_saved_photo(move |index| {
        if let Some(ui) = weak_ui.upgrade() {
            if !ui.get_message_target_ready() { return; }
            let choice = usize::try_from(index).ok().and_then(|i| picker_choices.borrow().get(i).cloned());
            let Some((chat, id, name)) = choice else { ui.set_message_send_status("File unavailable".into()); return; };
            if chat != usize::MAX && ui.get_message_chat_index().max(0) as usize != chat {
                ui.set_message_send_status("Select a photo again".into());
                return;
            }
            match attach_service.borrow_mut().send_saved_photo(chat, id, &name) {
                Ok(()) => ui.set_message_send_status("Sending…".into()),
                Err(_) => ui.set_message_send_status("Could not send photo".into()),
            }
        }
    });
    let weak_ui = ui.as_weak();
    let send_service = messages_service.clone();
    let send_transcripts = chat_transcripts.clone();
    let send_drafts = chat_drafts.clone();
    ui.on_send_message(move |text| {
        if let Some(ui) = weak_ui.upgrade() {
            if text.is_empty() || !ui.get_message_view() || !ui.get_message_service_ready() || !ui.get_message_target_ready() {
                return;
            }
            // FWD/DEL chips already pass fully framed bodies.
            let preframed = text.starts_with('\u{001e}');
            let reply = ui.get_message_reply_to().to_string();
            let action_id = ui.get_message_action_id().to_string();
            let wire = if preframed {
                text.to_string()
            } else if ui.get_message_edit_mode() && !action_id.is_empty() {
                alloc::format!("\u{001e}E\u{001f}{action_id}\u{001f}{text}")
            } else if reply.is_empty() {
                text.to_string()
            } else {
                encode_quoted_send(ui.get_message_reply_mine(), reply.as_str(), text.as_str())
            };
            debug_println!(
                "SLINT SIMPLEX SEND REQUEST LEN {} EDIT {} QUOTE {}",
                wire.len(),
                ui.get_message_edit_mode(),
                !reply.is_empty() && !ui.get_message_edit_mode()
            );
            let result = send_service.borrow_mut().send_message(wire.as_str());
            match result {
                Ok(()) => {
                    let index = ui.get_message_chat_index().max(0) as usize;
                    #[cfg(feature = "utm-transcript-worker")]
                    send_service.borrow().history_changed(index.min(CHAT_SLOTS - 1));
                    match send_service.borrow().transcript(index as u64) {
                        Ok(saved) => {
                            send_transcripts.borrow_mut()[index.min(CHAT_SLOTS - 1)] = saved.clone();
                            ui.set_message_transcript(saved.into());
                        }
                        Err(_) => {
                            let display = if reply.is_empty() || ui.get_message_edit_mode() {
                                text.to_string()
                            } else {
                                alloc::format!("↩ {} · {}", reply.replace('\n', " "), text.as_str())
                            };
                            remember_chat_line(
                                &ui,
                                &send_transcripts,
                                index,
                                "NexatrodeOS",
                                display.as_str(),
                            );
                        }
                    }
                    send_drafts.borrow_mut()[index.min(CHAT_SLOTS - 1)].clear();
                    ui.set_message_body("".into());
                    ui.set_message_reply_to("".into());
                    ui.set_message_action_id("".into());
                    ui.set_message_edit_mode(false);
                    match send_service.borrow().send_status() {
                        Ok((2, _)) => ui.set_message_send_status("Sent".into()),
                        Ok((4, _)) => ui.set_message_send_status("Send failed".into()),
                        Ok(_) => ui.set_message_send_status("Sending…".into()),
                        Err(_) => ui.set_message_send_status("Sending…".into()),
                    }
                }
                Err(error) => {
                    debug_println!("SLINT SIMPLEX SEND FAIL {}", error);
                    ui.set_message_send_status("Not sent".into());
                }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let send_service = messages_service.clone();
    let send_status_transcripts = chat_transcripts.clone();
    ui.on_check_message_send_status(move || {
        if let Some(ui) = weak_ui.upgrade() {
            match send_service.borrow().send_status() {
                Ok((0, _)) => ui.set_message_send_status("Ready".into()),
                Ok((1, _)) => ui.set_message_send_status("Sending…".into()),
                Ok((7, _)) => ui.set_message_send_status("Upload failed · tap Retry".into()),
                Ok((5, _)) => {
                    let index = ui.get_message_chat_index().max(0) as usize;
                    if let Ok(saved) = send_service.borrow().transcript(index as u64) {
                        send_status_transcripts.borrow_mut()[index.min(CHAT_SLOTS - 1)] = saved.clone();
                        ui.set_message_transcript(saved.into());
                    }
                    ui.set_message_send_status("Photo transfer unverified".into());
                }
                Ok((2, _)) => {
                    let index = ui.get_message_chat_index().max(0) as usize;
                    if let Ok(saved) = send_service.borrow().transcript(index as u64) {
                        send_status_transcripts.borrow_mut()[index.min(CHAT_SLOTS - 1)] =
                            saved.clone();
                        ui.set_message_transcript(saved.into());
                    }
                    ui.set_message_send_status("Sent".into());
                }
                Ok((_, _)) => ui.set_message_send_status("Send failed".into()),
                Err(_) => ui.set_message_send_status("Send status unknown".into()),
            }
        }
    });
    let weak_ui = ui.as_weak();
    let receive_service = messages_service.clone();
    let receive_transcripts = chat_transcripts.clone();
    ui.on_receive_next_message(move || {
        if let Some(ui) = weak_ui.upgrade() {
            debug_println!("SLINT SIMPLEX INBOX POLL REQUEST");
            ui.set_message_receive_status(if ui.get_message_target_ready() { "Listening" } else { "Opening conversation…" }.into());
            #[cfg(feature = "utm-transcript-worker")]
            receive_service.borrow().request_live_receive(true);
            #[cfg(not(feature = "utm-transcript-worker"))]
            {
            let requested = receive_service.borrow_mut().receive_next();
            match requested {
                Ok(()) => {
                    let status = receive_service.borrow().receive_status();
                    match status {
                    Ok((2, text, chat_index)) => keep_received_message(
                        &ui,
                        &text,
                        chat_index,
                        &receive_transcripts,
                        &receive_service,
                    ),
                    Ok((3, _, _)) => ui.set_message_receive_status("Up to date".into()),
                    Ok((1, _, _)) => ui.set_message_receive_status("Listening".into()),
                    Ok((_, _, _)) => ui.set_message_receive_status("Receive failed".into()),
                    Err(_) => ui.set_message_receive_status("Receive status unknown".into()),
                    }
                }
                Err(error) => {
                    debug_println!("SLINT SIMPLEX RECEIVE FAIL {}", error);
                    ui.set_message_receive_status("Receive request failed".into());
                }
            }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let receive_service = messages_service.clone();
    let receive_status_transcripts = chat_transcripts.clone();
    ui.on_check_message_receive_status(move || {
        if let Some(ui) = weak_ui.upgrade() {
            #[cfg(feature = "utm-transcript-worker")]
            {
                receive_service.borrow().request_live_receive(false);
                ui.set_message_receive_status("Listening".into());
            }
            #[cfg(not(feature = "utm-transcript-worker"))]
            {
            let status = receive_service.borrow().receive_status();
            match status {
                Ok((1, _, _)) => ui.set_message_receive_status("Listening".into()),
                Ok((2, text, chat_index)) => keep_received_message(
                    &ui,
                    &text,
                    chat_index,
                    &receive_status_transcripts,
                    &receive_service,
                ),
                Ok((3, _, _)) => ui.set_message_receive_status("Up to date".into()),
                Ok((_, _, _)) => ui.set_message_receive_status("Receive failed".into()),
                Err(_) => ui.set_message_receive_status("Receive status unknown".into()),
            }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let ack_service = messages_service.clone();
    ui.on_acknowledge_incoming_message(move || {
        if let Some(ui) = weak_ui.upgrade() {
            match ack_service.borrow_mut().acknowledge_received() {
                Ok(()) => ui.set_message_ack_status("ACKNOWLEDGING".into()),
                Err(error) => {
                    debug_println!("SLINT SIMPLEX ACK FAIL {}", error);
                    ui.set_message_ack_status("ACKNOWLEDGE REQUEST FAILED".into());
                }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let ack_service = messages_service.clone();
    ui.on_check_message_ack_status(move || {
        if let Some(ui) = weak_ui.upgrade() {
            match ack_service.borrow().acknowledge_status() {
                Ok(1) => ui.set_message_ack_status("ACKNOWLEDGING".into()),
                Ok(2) => {
                    ui.set_message_ack_status("ACKNOWLEDGED".into());
                    debug_println!("SLINT SIMPLEX ACK PASS");
                }
                Ok(_) => ui.set_message_ack_status("ACKNOWLEDGE FAILED".into()),
                Err(_) => ui.set_message_ack_status("ACK STATUS UNAVAILABLE".into()),
            }
        }
    });
    let weak_ui = ui.as_weak();
    let save_calendar = calendar_service.clone();
    ui.on_save_calendar(move |title| {
        if let Some(ui) = weak_ui.upgrade() {
            let status = match save_calendar.borrow_mut().save_and_verify(title.as_str()) {
                Ok(()) => "SAVED THROUGH VAULT",
                Err(error) => {
                    debug_println!("SLINT CALENDAR SAVE FAIL {}", error);
                    "VAULT SAVE FAILED"
                }
            };
            ui.set_calendar_status(status.into());
        }
    });
    let weak_ui = ui.as_weak();
    let save_vault = notes_service.clone();
    ui.on_save_note(move |note| {
        if let Some(ui) = weak_ui.upgrade() {
            let status = match save_vault
                .borrow_mut()
                .save_and_verify(note.as_str().as_bytes())
            {
                Ok(()) => "SAVED THROUGH VAULT",
                Err(error) => {
                    debug_println!("SLINT NOTES SAVE FAIL {}", error);
                    "VAULT SAVE FAILED"
                }
            };
            ui.set_status(status.into());
        }
    });
    let weak_ui = ui.as_weak();
    ui.on_key_tapped(move |key| {
        if let Some(ui) = weak_ui.upgrade() {
            #[cfg(feature = "utm-wrapped-vault-experiment")]
            if ui.get_page() == 1 && ui.get_note_editing() && ui.get_edit_target() == 1 {
                let mut text = ui.get_note().to_string();
                match key.as_str() {
                    "DELETE" | "\u{8}" => { text.pop(); },
                    "SPACE" | " " if text.len() < 128 => text.push(' '),
                    "ENTER" | "\n" if text.len() < 128 => text.push('\n'),
                    k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() && text.len() < 128 => text.push(k.as_bytes()[0].to_ascii_uppercase() as char),
                    _ => {},
                }
                ui.set_note(text.into());
                SLINT_INPUT.notify();
                return;
            }
            if ui.get_page() == 6 && ui.get_edit_target() == 17 {
                let mut name = ui.get_message_group_name().to_string();
                match key.as_str() {
                    "ENTER" | "\n" => ui.set_edit_target(0),
                    "DELETE" | "\u{8}" => { name.pop(); },
                    "SPACE" | " " if name.len() < 32 => name.push(' '),
                    k if k.len() == 1 && name.len() < 32 && (k.as_bytes()[0].is_ascii_alphanumeric() || matches!(k, "." | "_" | "-")) => name.push_str(k),
                    _ => (),
                }
                ui.set_message_group_name(name.into());
                SLINT_INPUT.notify();
                return;
            }
            if ui.get_edit_target() == 13 {
                let key = key.as_str();
                if key == "ENTER" {
                    ui.set_edit_target(0);
                    ui.invoke_save_profile();
                } else if key == "DELETE" || key == "SPACE" || (key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric()) {
                    let mut next = ui.get_message_profile_name().to_string();
                    if key == "DELETE" {
                        next.pop();
                    } else if next.len() < 48 {
                        next.push_str(key);
                    }
                    ui.set_message_profile_name(next.into());
                }
                let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
                if !flag.is_null() {
                    unsafe { flag.write_volatile(1) };
                }
                SLINT_INPUT.notify();
                return;
            }
            if apply_wallet_key(&ui, key.as_str()) {
                let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
                if !flag.is_null() {
                    unsafe { flag.write_volatile(1) };
                }
                SLINT_INPUT.notify();
                return;
            }
            // The shell replaces the suite key handler. Chat compose is
            // edit-target 6; without this, the on-screen keyboard never
            // changes the message box.
            if ui.get_page() == 6 && ui.get_message_view() && ui.get_edit_target() == 6 {
                let key = key.as_str();
                let mut next = ui.get_message_body().to_string();
                let changed = match key {
                    "DELETE" => {
                        next.pop();
                        true
                    }
                    "SPACE" if next.len() < 128 => {
                        next.push(' ');
                        true
                    }
                    letter
                        if letter.len() == 1
                            && next.len() < 128
                            && letter.as_bytes()[0].is_ascii_alphanumeric() =>
                    {
                        next.push_str(letter);
                        true
                    }
                    _ => false,
                };
                if changed {
                    debug_println!("SLINT UTM MESSAGE BODY {}", next.len());
                    ui.set_message_body(next.into());
                    let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
                    if !flag.is_null() {
                        unsafe { flag.write_volatile(1) };
                    }
                    SLINT_INPUT.notify();
                }
            }
        }
    });
    let weak_ui = ui.as_weak();
    let mut wallet_setup = WalletServiceClient {
        transfer: NonNull::new(wallet_transfer.as_ptr()).expect("Wallet transfer page"),
    };
    ui.on_wallet_setup_action(move |action| {
        WALLET_SETUP_ACTIONS.fetch_add(1, Ordering::Relaxed);
        debug_println!("SLINT WALLET SETUP ACTION {}", action);
        let Some(ui) = weak_ui.upgrade() else { return; };
        ui.set_edit_target(0);
        if action == 6 {
            let _ = wallet_setup.call(0x50b);
            ui.set_wallet_words("".into());
            ui.set_wallet_backup("".into());
            ui.set_wallet_setup_status("".into());
            ui.set_wallet_setup(1);
            return;
        }
        let request = match action { 0 => 0x509, 1 => 0x50a, 3 => 0x50c, 4 => 0x50e, 5 => 0x50d, _ => return };
        if action == 3 || action == 4 {
            let words = ui.get_wallet_words();
            if words.is_empty() || !unsafe { wallet_setup.transfer.as_mut() }.set_payload(words.as_bytes()) {
                ui.set_wallet_setup_status("Enter your seed words".into());
                return;
            }
        }
        let reply = if action == 0 || action == 3 {
            let Ok(height) = ui.get_wallet_height().as_str().parse::<u64>() else {
                unsafe { wallet_setup.transfer.as_mut() }.set_payload(&[]);
                ui.set_wallet_setup_status("Enter a valid restore height".into());
                return;
            };
            set_mr(0, ui.get_wallet_network() as u64);
            set_mr(1, height);
            WALLET_SERVICE.pp_call(MessageInfo::new(request, 2))
        } else { WALLET_SERVICE.pp_call(MessageInfo::new(request, 0)) };
        if reply.label() != REPLY_WALLET_OK || reply.count() != 0 {
            if action == 5 { debug_println!("SLINT WALLET LOCK REJECTED"); }
            unsafe { wallet_setup.transfer.as_mut() }.set_payload(&[]);
            ui.set_wallet_setup_status("Wallet request failed or wallet is busy".into());
            return;
        }
        if action != 3 { ui.set_wallet_words("".into()); }
        ui.set_wallet_setup_status(if action == 0 { "Finding the current network height…" } else { "Working…" }.into());
        match action {
            0 => ui.set_wallet_setup(6),
            1 => ui.set_wallet_setup(7),
            3 => ui.set_wallet_setup(9),
            4 => ui.set_wallet_setup(8),
            5 => {
                ui.set_wallet_address("".into());
                ui.set_wallet_address_short("".into());
                ui.set_wallet_balance("—".into());
                ui.set_wallet_unlocked("—".into());
                ui.set_wallet_address_qr_visible(false);
                ui.set_wallet_history("No transactions yet".into());
                ui.set_wallet_tx_amt_0("".into());
                ui.set_wallet_tx_amt_1("".into());
                ui.set_wallet_tx_amt_2("".into());
                ui.set_wallet_tx_amt_3("".into());
                ui.set_wallet_tx_amt_4("".into());
                ui.set_wallet_send_dest("".into());
                ui.set_wallet_send_amount("".into());
                ui.set_wallet_send_status("".into());
                ui.set_wallet_fee_ready(false);
                ui.set_wallet_test_preview(false);
                ui.set_wallet_status("NEXAWAL | LOCKED".into());
                ui.set_wallet_setup_status("Wallet locked".into());
                ui.set_wallet_setup(5);
                debug_println!("SLINT WALLET LOCKED");
            },
            _ => (),
        }
        if action != 5 { WALLET_SERVICE.notify(); }
        SLINT_INPUT.notify();
    });
    let weak_ui = ui.as_weak();
    let mut wallet_node = WalletServiceClient {
        transfer: NonNull::new(wallet_transfer.as_ptr()).expect("Wallet transfer page"),
    };
    ui.on_set_wallet_node(move || {
        WALLET_NODE_ACTIONS.fetch_add(1, Ordering::Relaxed);
        let Some(ui) = weak_ui.upgrade() else { return; };
        ui.set_edit_target(0);
        let value = ui.get_wallet_node();
        let node_value = wallet_node_authority(value.as_str());
        if !unsafe { wallet_node.transfer.as_mut() }.set_payload(node_value.as_bytes()) {
            ui.set_wallet_node_status("Node address is too long".into());
            return;
        }
        match wallet_node.call(0x510) {
            Ok(_) => {
                ui.set_wallet_node_status("Saving node…".into());
                WALLET_SERVICE.notify();
            }
            Err(_) => ui.set_wallet_node_status("Use host:port; wallet must be idle".into()),
        }
        SLINT_INPUT.notify();
    });
    let weak_ui = ui.as_weak();
    let mut wallet_prepare = WalletServiceClient {
        transfer: NonNull::new(wallet_transfer.as_ptr()).expect("Wallet transfer page"),
    };
    ui.on_prepare_wallet_send(move || {
        if let Some(ui) = weak_ui.upgrade() {
            preview_wallet_fee(&ui, &mut wallet_prepare);
            let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
            if !flag.is_null() {
                unsafe { flag.write_volatile(1) };
            }
            SLINT_INPUT.notify();
        }
    });
    let weak_ui = ui.as_weak();
    let mut wallet_confirm = WalletServiceClient {
        transfer: NonNull::new(wallet_transfer.as_ptr()).expect("Wallet transfer page"),
    };
    ui.on_confirm_wallet_send(move || {
        if let Some(ui) = weak_ui.upgrade() {
            ui.set_edit_target(0);
            if !ui.get_wallet_fee_ready() {
                ui.set_wallet_send_status("Preview the fee first".into());
            } else {
                let sent = if ui.get_wallet_test_preview() {
                    match wallet_send_payload(&ui) {
                        Ok(payload) => wallet_confirm.confirm_test_preview(&payload),
                        Err(_) => "NEXAWAL | PREVIEW CHANGED".into(),
                    }
                } else { wallet_confirm.send_tx() };
                if sent.contains("SEND QUEUED") || sent.contains("VALIDATING PREVIEW") {
                    ui.set_wallet_status(sent.clone().into());
                    WALLET_SERVICE.notify();
                }
                ui.set_wallet_fee_ready(false);
                ui.set_wallet_test_preview(false);
                apply_wallet_face(&ui, &sent);
                ui.set_wallet_send_status(if sent.contains("VALIDATING PREVIEW") { "Checking payment…".into() } else { humanize_wallet_status(&sent).into() });
            }
            let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
            if !flag.is_null() {
                unsafe { flag.write_volatile(1) };
            }
            SLINT_INPUT.notify();
        }
    });
    let weak_ui = ui.as_weak();
    let wallet_reconcile = WalletServiceClient {
        transfer: NonNull::new(wallet_transfer.as_ptr()).expect("Wallet transfer page"),
    };
    ui.on_reconcile_wallet_send(move || {
        if let Some(ui) = weak_ui.upgrade() {
            match wallet_reconcile.call(0x50f) {
                Ok(status) => {
                    if let Some(height) = status.split("TRANSACTION SCANNED BLOCK ").nth(1) {
                        ui.set_wallet_send_status(alloc::format!("Last scan found transaction in block {}", height).into());
                    } else {
                        ui.set_wallet_send_status("Checking transaction…".into());
                        WALLET_SERVICE.notify();
                    }
                }
                Err(_) => ui.set_wallet_send_status("Check unavailable · verify node and wait for sync".into()),
            }
            SLINT_INPUT.notify();
        }
    });
    let weak_ui = ui.as_weak();
    let wallet_retry = WalletServiceClient {
        transfer: NonNull::new(wallet_transfer.as_ptr()).expect("Wallet transfer page"),
    };
    ui.on_retry_wallet_send(move || {
        if let Some(ui) = weak_ui.upgrade() {
            ui.set_edit_target(0);
            let sent = wallet_retry.send_tx();
            ui.set_wallet_fee_ready(false);
            ui.set_wallet_send_status(humanize_wallet_status(&sent).into());
            if sent.contains("SEND QUEUED") {
                ui.set_wallet_status(sent.into());
                WALLET_SERVICE.notify();
            }
            SLINT_INPUT.notify();
        }
    });
    let weak_ui = ui.as_weak();
    ui.on_copy_wallet_address(move || {
        if let Some(ui) = weak_ui.upgrade() {
            let addr = ui.get_wallet_address().to_string();
            if addr.is_empty() {
                apply_wallet_face(&ui, "Address not ready yet");
            } else {
                emit_nexatrode_copy(&addr);
                apply_wallet_face(&ui, "Address copied");
                // Keep address fields; only refresh sync line.
                ui.set_wallet_sync("Address copied".into());
            }
            let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
            if !flag.is_null() {
                unsafe { flag.write_volatile(1) };
            }
            SLINT_INPUT.notify();
        }
    });
    let weak_ui = ui.as_weak();
    #[cfg(feature = "simplex-agent-ui")]
    let back_drafts = chat_drafts.clone();
    ui.on_system_back(move || {
        if let Some(ui) = weak_ui.upgrade() {
            #[cfg(feature = "simplex-agent-ui")]
            let changed = shell_navigate_back(&ui, Some(back_drafts.as_ref()));
            #[cfg(not(feature = "simplex-agent-ui"))]
            let changed = shell_navigate_back(&ui, None);
            if changed {
                debug_println!("SLINT NAV BACK -> {}", ui.get_page());
            }
        }
    });
    let weak_ui = ui.as_weak();
    #[cfg(feature = "simplex-agent-ui")]
    let home_drafts = chat_drafts.clone();
    ui.on_system_home(move || {
        if let Some(ui) = weak_ui.upgrade() {
            #[cfg(feature = "simplex-agent-ui")]
            let changed = shell_navigate_home(&ui, Some(home_drafts.as_ref()));
            #[cfg(not(feature = "simplex-agent-ui"))]
            let changed = shell_navigate_home(&ui, None);
            if changed {
                debug_println!("SLINT NAV HOME");
            }
        }
    });
    let weak_ui = ui.as_weak();
    ui.on_system_recents(move || {
        if let Some(ui) = weak_ui.upgrade() {
            if shell_navigate_recents(&ui) {
                debug_println!("SLINT NAV RECENTS");
            }
        }
    });
    ui.show().expect("show Slint workspace suite");
    // Skip prepare_skia_label before first wallet frame: on TCG it can run so
    // long that the FB stays on a stale Notes band and tip sync never starts.
    // Labels are filled on demand / later catch-up.
    // Defer ui_focus until after the first publish_frame so spice does not show
    // a stale host-side Notes surface while init is still building SlintService.
    CATCH_UP_FLAG.store(unsafe { focus.as_ptr().add(1) }, Ordering::Release);
    let timer_id = *sel4_microkit::var!(slint_timer_channel: usize = usize::MAX);
    let mut service = SlintService {
        #[cfg(feature = "utm-device-lock")]
        device_lock,
        #[cfg(feature = "utm-photo-worker")]
        photo_mailbox: unsafe { memory_region_symbol!(photo_mailbox_region: *mut personal_os_slint_notes_spike::photo_ipc::PhotoMailbox).as_ref() },
        #[cfg(feature = "utm-photo-worker")]
        pending_photo: None,
        #[cfg(feature = "utm-photo-worker")]
        pending_photo_read: None,
        #[cfg(feature = "utm-photo-worker")]
        photo_read_mailbox: unsafe { memory_region_symbol!(photo_read_mailbox: *mut personal_os_slint_notes_spike::photo_reader_ipc::PhotoReadMailbox).as_ref() },
        #[cfg(feature = "utm-photo-worker")]
        photo_cache,
        frame_timer: (timer_id < 64).then(|| Channel::new(timer_id)),
        frame_deadline: None,
        _ui: ui,
        window,
        header: NonNull::new(surface_ptr.cast::<FrameSurfaceHeader>()).expect("frame header"),
        pixels: NonNull::new(pixels).expect("frame pixels"),
        rendered: vec![0u8; FRAME_PIXEL_BYTES],
        pointer,
        shift_down: false,
        meta_down: false,
        ctrl_down: false,
        notes_service,
        contacts_service,
        calendar_service,
        #[cfg(feature = "simplex-agent-ui")]
        messages_service,
        #[cfg(feature = "simplex-agent-ui")]
        chat_transcripts,
        #[cfg(feature = "simplex-agent-ui")]
        chat_drafts,
        #[cfg(feature = "simplex-agent-ui")]
        chat_photo_loader,
        shown_view: -1,
        shown_stamp: 0,
        pictures_prepared: false,
        needs_catch_up: false,
        prepare_one: false,
        #[cfg(feature = "simplex-agent-ui")]
        transcripts_loaded: false,
        #[cfg(feature = "simplex-agent-ui")]
        chat_read_len: [0; CHAT_SLOTS],
        #[cfg(feature = "utm-transcript-worker")]
        history_baselined: [false; CHAT_SLOTS],
        #[cfg(feature = "utm-transcript-worker")]
        initial_unread: [0; CHAT_SLOTS],
        #[cfg(feature = "simplex-agent-ui")]
        last_unread: [0; CHAT_SLOTS],
        wallet_service: WalletServiceClient {
            transfer: NonNull::new(wallet_transfer.as_ptr()).expect("Wallet transfer page"),
        },
        wallet_opened: false,
        wallet_boot_phase: if cfg!(feature="desktop-simplex-frontend") {0} else {1},
        wallet_boot_sync_queued: false,
        wallet_boot_opened: alloc::string::String::new(),
        wallet_sync_polls: 0,
        wallet_tip_rearm_cool: 0,
        wallet_sync_active: false,
        wallet_face_tab_dirty: false,
        scanout_hold: 0,
        defer_frame: false,
        frame_dirty: false,
        feedback_frame_dirty: false,
        #[cfg(feature = "simplex-agent-ui")]
        chat_refresh: personal_os_slint_notes_spike::chat_refresh::ChatRefresh::default(),
        wallet_connect_keep_cycles: 0,
        nav_gesture: NavGesture {
            active: false,
            defer_press: false,
            feedback_press: false,
            start_x: 0.0,
            start_y: 0.0,
            last_x: 0.0,
            last_y: 0.0,
            from_edge: false,
            from_home: false,
        },
        nav_last_app_page: 10,
        #[cfg(feature = "desktop-real-app")]
        desktop_held: core::array::from_fn(|_| None),
        #[cfg(feature = "desktop-real-app")]
        desktop_pointer: None,
    };
    // Paint ARMED only. Open+queue runs on the first GPU→input→SLINT pulse
    // after scanout — init-time OPEN/REFRESH Calls left the shell with no
    // handler bounce, so WaitConnect never got notify pumps.
    if !service._ui.get_device_lock_screen() { service._ui.set_page(if cfg!(feature="desktop-simplex-frontend") {6} else {16}); }
    let page_now = service._ui.get_page();
    #[cfg(not(feature = "utm-wrapped-vault-experiment"))]
    service
        ._ui
        .set_note(alloc::format!("PAGEPROBE AFTER SET16 GET {page_now}").into());
    service._ui.set_wallet_status(
        alloc::format!("NEXAWAL · PAGE {page_now} · BOOT PHASE ARMED").into(),
    );
    skia_guest::invalidate_chrome();
    service.refresh_wall_clock();
    service.window.request_redraw();
    service.publish_frame();
    unsafe {
        focus.as_ptr().write_volatile(1);
        // Catch-up stays set until input flushes the ARMED frame and notifies
        // us — required when gpu timer is slow to pick up the first revision.
        focus.as_ptr().add(1).write_volatile(1);
    }
    #[cfg(not(feature="desktop-simplex-frontend"))]
    { service.wallet_boot_phase = 1; }
    #[cfg(feature="desktop-simplex-frontend")]
    {
        service._ui.set_wallet_available(false);
        service.load_vault_apps();
        service.chat_refresh.request(true);
        service.publish_frame();
        #[cfg(not(feature = "desktop-real-app"))]
        frontend_probe::publish(&service._ui);
        debug_println!("SIMPLEX_FRONTEND|STARTED");
    }
    SLINT_INPUT.notify();
    debug_println!("SLINT FIRST FRAME PAGE {} PHASE ARMED", page_now);
    let revision = unsafe { service.header.as_ref() }
        .revision
        .load(Ordering::Acquire);
    debug_println!(
        "SLINT UTM FRAME READY {}x{} STRIDE {} REV {}",
        WIDTH,
        HEIGHT,
        FRAME_STRIDE_BYTES,
        revision
    );
    #[cfg(feature = "utm-clock-selftest")]
    {
        use personal_os_slint_notes_spike::{frame_clock::elapsed, guest_platform::{counter_frequency, counter_ticks}};
        let start = counter_ticks();
        let delay = core::time::Duration::from_millis(80);
        slint::Timer::single_shot(delay, move || {
            let took = elapsed(counter_ticks().wrapping_sub(start), counter_frequency());
            debug_println!("SLINT CLOCK CALLBACK {} us={}", if took.as_millis() >= 79 { "PASS" } else { "EARLY" }, took.as_micros());
        });
    }
    service.arm_frame_timer();
    service
}

impl SlintService {
    fn refresh_wall_clock(&self) {
        let address = clock_view_address();
        if address == 0 { return; }
        let view = address as *const personal_os_rust_server::app_view::AppViewState;
        let mut text = [0u8; 19];
        // The clock service publishes through the shared view's sequence
        // counter. A bounded retry avoids ever blocking the UI on the writer.
        for _ in 0..8 {
            let revision = unsafe { &*core::ptr::addr_of!((*view).revision) };
            let first = revision.load(Ordering::Acquire);
            if first & 1 != 0 { continue; }
            let size = unsafe { core::ptr::addr_of!((*view).length).read_volatile() };
            if size != text.len() as u32 { return; }
            let source = unsafe { core::ptr::addr_of!((*view).text).cast::<u8>() };
            for (i, byte) in text.iter_mut().enumerate() {
                *byte = unsafe { source.add(i).read_volatile() };
            }
            core::sync::atomic::fence(Ordering::Acquire);
            if first != revision.load(Ordering::Acquire) { continue; }
            let table = include_bytes!(concat!(env!("OUT_DIR"), "/timezone.bin"));
            if let Some(time) = personal_os_slint_notes_spike::wall_clock::display_time(&text, table) {
                let time = core::str::from_utf8(&time).expect("clock digits");
                if self._ui.get_system_clock().as_str() != time {
                    self._ui.set_system_clock(time.into());
                }
            }
            return;
        }
    }

    fn nav_drafts(&self) -> Option<&RefCell<[String; CHAT_SLOTS]>> {
        #[cfg(feature = "simplex-agent-ui")]
        {
            Some(self.chat_drafts.as_ref())
        }
        #[cfg(not(feature = "simplex-agent-ui"))]
        {
            let _ = self;
            None
        }
    }

    fn begin_sync_poll(&mut self) {
        self.wallet_sync_active = true;
        self.wallet_sync_polls = 0;
        WALLET_SERVICE.notify();
    }

    /// One boot phase per notify so gpu_client (prio 1) can scan out between
    /// OPENING → smoke PPC → sync queue on 1-core TCG. No dhcp STATUS PPC.
    fn advance_wallet_boot(&mut self) -> bool {
        if self.wallet_boot_phase == 0 {
            return false;
        }
        match self.wallet_boot_phase {
            1 | 2 => {
                // Sync OPEN Call only (derive in wallet protected). Tip refresh
                // is forced in phase 5 / OPEN-park rearm — do not nest refresh
                // in this same advance (dump223 tipq left ARMED wedged).
                debug_println!("SLINT WALLET BOOT PHASE OPEN+SYNC");
                if !self._ui.get_device_lock_screen() { self._ui.set_page(16); }
                self._ui
                    .set_wallet_status("NEXAWAL · OPENING WALLET PD…".into());
                self.publish_frame();
                self.wallet_opened = true;
                let opened = self.wallet_service.smoke_round_trip();
                self.wallet_boot_opened = opened.clone();
                if opened.contains("SETUP REQUIRED") {
                    self.wallet_boot_phase = 4;
                    self.wallet_sync_active = false;
                    self._ui.set_wallet_status(opened.into());
                    self._ui.set_wallet_setup(1);
                    self._ui.set_wallet_setup_status("Create, restore, or unlock a wallet".into());
                    self.publish_frame();
                    SLINT_INPUT.notify();
                    return true;
                }
                if let Ok(endpoint) = self.wallet_service.call(0x511) {
                    let kind = if endpoint.ends_with(":443") { "Saved HTTPS node · host:port" } else { "Saved HTTP node · host:port" };
                    self._ui.set_wallet_node(endpoint.into());
                    self._ui.set_wallet_node_status(kind.into());
                } else {
                    self._ui.set_wallet_node_status("Couldn’t load saved node".into());
                }
                {
                    #[repr(C)]
                    struct TcpPageProbe {
                        magic: u32,
                        request_len: u32,
                        response_len: u32,
                        reserved: u32,
                    }
                    let page = memory_region_symbol!(haskell_tcp_page: *mut TcpPageProbe);
                    unsafe {
                        core::ptr::write_volatile(
                            page.as_ptr(),
                            TcpPageProbe {
                                magic: 0x3150_4354,
                                request_len: 0,
                                response_len: 0,
                                reserved: 0,
                            },
                        );
                    }
                }
                self.wallet_sync_active = true;
                self.wallet_sync_polls = 0;
                self.wallet_boot_phase = 5;
                // OPEN restores the saved checkpoint but does not start IO.
                // Explicitly queue a refresh; a notify alone leaves the
                // wallet idle on its last saved scan height.
                let queued = self.wallet_service.queue_refresh();
                self.wallet_boot_sync_queued = true;
                self._ui.set_wallet_status(alloc::format!("{opened} · {queued}").into());
                self.publish_frame();
                WALLET_SERVICE.notify();
                let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
                if !flag.is_null() {
                    unsafe { flag.write_volatile(1) };
                }
                SLINT_INPUT.notify();
                true
            }
            3 => {
                // Legacy phase — treat as sync poll (open+queue now happens in 2).
                self.wallet_boot_phase = 5;
                true
            }
            5 => {
                // After async OPEN: wait for derive, then queue tip refresh once.
                if let Some(peeked) = self.wallet_service.peek_transfer() {
                    if peeked.contains("OPENING") {
                        self._ui.set_wallet_status(peeked.into());
                        self.publish_frame();
                        WALLET_SERVICE.notify();
                        return true;
                    }
                    let opened = peeked.contains("OPEN mainnet")
                        || peeked.contains("H3519450")
                        || peeked.contains("OPEN FAIL");
                    // A restored checkpoint appears in OPEN status as SCAN.
                    // That describes saved progress, not an active refresh.
                    if opened && !self.wallet_boot_sync_queued {
                        let queued = self.wallet_service.queue_refresh();
                        self.wallet_boot_sync_queued = true;
                        self._ui.set_wallet_status(
                            alloc::format!("{peeked} · {queued}").into(),
                        );
                        self.publish_frame();
                        self.begin_sync_poll();
                        return true;
                    }
                }
                if self.poll_wallet_sync_once() {
                    self.wallet_boot_phase = 4;
                    true
                } else {
                    true
                }
            }
            4 => {
                debug_println!("SLINT WALLET BOOT PHASE VAULT");
                self.wallet_boot_phase = 0;
                skia_guest::prepare_skia_label();
                self.load_vault_apps();
                if !self._ui.get_device_lock_screen() { self.prepare_hidden_pages(); }
                self.arm_page_sync();
                false
            }
            _ => {
                self.wallet_boot_phase = 0;
                false
            }
        }
    }

    /// Advance wallet TCP via notify + paint live transfer status. Returns true when done.
    fn poll_wallet_sync_once(&mut self) -> bool {
        self.wallet_sync_polls = self.wallet_sync_polls.saturating_add(1);
        let peeked = self.wallet_service.peek_transfer();
        let terminal = peeked
            .as_deref()
            .is_some_and(WalletServiceClient::sync_terminal);
        if !terminal && self.wallet_sync_polls >= 40_000 {
            // A long scan is still in flight; do not replace its request or
            // mistake the UI polling budget for a completed sync.
            self.wallet_sync_polls = 0;
            WALLET_SERVICE.notify();
            return false;
        }
        if terminal {
            let synced = peeked.unwrap_or_else(|| "SYNC TIMEOUT".into());
            if let Some(request) = synced.split(" | REQUEST ").nth(1).and_then(|s| s.split(" | ").next()) {
                if wallet_send_payload(&self._ui).ok().as_deref() == Some(request) {
                    if let Some(fee) = parse_prepare_fee(&synced) {
                        let ready = synced.contains("TEST SEND READY");
                        self._ui.set_wallet_test_preview(ready);
                        self._ui.set_wallet_fee_ready(ready);
                        self._ui.set_wallet_send_status(wallet_preview_text(&synced, fee, self._ui.get_wallet_send_max(), ready).into());
                    }
                }
            }
            if synced.contains("DECOY DATA UNAVAILABLE") {
                self._ui.set_wallet_fee_ready(false);
                self._ui.set_wallet_send_status("Couldn’t prepare the fee · refresh and retry".into());
            } else if synced.contains("PREPARE INSUFFICIENT") {
                self._ui.set_wallet_fee_ready(false);
                self._ui.set_wallet_send_status("Not enough unlocked balance".into());
            } else if synced.contains("SEND OK") {
                self._ui.set_wallet_send_status("Relayed · awaiting confirmation".into());
            } else if synced.contains("SEND UNCONFIRMED") {
                self._ui.set_wallet_send_status("Relay outcome unknown · check transaction".into());
            } else if synced.contains("VALIDATION") || synced.contains("PREVIEW CHANGED") {
                self._ui.set_wallet_send_status("Payment changed · sync and preview again".into());
            } else if synced.contains("SAVE FAILED") {
                self._ui.set_wallet_send_status("Couldn’t save transaction · not relayed".into());
            }
            let cur = self._ui.get_wallet_status();
            if cur.as_str() != synced.as_str() {
                self._ui.set_wallet_status(synced.clone().into());
            }
            apply_wallet_face(&self._ui, &synced);
            debug_println!(
                "SLINT WALLET SYNC DONE polls={} terminal={}",
                self.wallet_sync_polls,
                terminal
            );
            self.wallet_sync_active = false;
            // Final status can carry the transaction list. A partial band
            // leaves the old "Waiting for a deposit…" pixels in that card.
            self.wallet_face_tab_dirty = true;
            self.publish_frame();
            return true;
        }
        if let Some(ref text) = peeked {
            // r18: host lsof showed Cake TCP ESTABLISHED while qemu usernet
            // stayed TCP[SYN_RCVD] for minutes — SYN-ACK never ACKed by guest.
            // ethernet_driver budget is tiny vs gpu_driver prio 254; any
            // publish_frame / face-carousel during CONNECTING/WAIT NET starves
            // RX and the handshake dies as Connection aborted.
            // Don't GPU-hold on CONNECTING when spent-gate sticky is present —
            // "Waiting for deposit…" must paint even while TCP reopens.
            let handshake_hold = (text.contains("CONNECTING") || text.contains("WAIT NET"))
                && !text.contains("NEED DEPOSIT");
            // Avoid full-frame paint on every poll — that starves dhcp on TCG.
            let cur = self._ui.get_wallet_status();
            let status_changed = cur.as_str() != text.as_str();
            if status_changed {
                // Stay on whichever app is open. A background scan must not
                // pull SimpleX or Settings back onto the wallet.
                self._ui.set_wallet_status(text.clone().into());
            }
            // r22/r29: always map BAL/ADDR — sticky BAL can arrive on an
            // unchanged CONNECTING tip line, and picture-cache must see the
            // new wallet_balance stamp. Hold GPU only while handshake has
            // no balance yet.
            let bal_before = self._ui.get_wallet_balance().to_string();
            let sync_before = self._ui.get_wallet_sync().to_string();
            let hist_before = self._ui.get_wallet_history().to_string();
            apply_wallet_face(&self._ui, text);
            let bal_after = self._ui.get_wallet_balance().to_string();
            let sync_after = self._ui.get_wallet_sync().to_string();
            let bal_changed = bal_after != bal_before;
            let sync_changed = sync_after != sync_before;
            let hist_changed = self._ui.get_wallet_history().as_str() != hist_before;
            // Balance and history changes can expose stale pixels in the
            // wallet cards. Sync text alone has its own Slint damage region;
            // repainting the entire screen for each scanned block competes
            // with input and network processing on the single-core UTM guest.
            if bal_changed || hist_changed {
                self.wallet_face_tab_dirty = true;
            }
            if bal_changed || sync_changed || hist_changed || (status_changed && !handshake_hold) {
                self.publish_frame();
            }
            // dump184: never refresh while CONNECTING/WAIT NET during tip
            // abandon window — shell rearm reset WaitConnect (dump183).
            // dump185: wallet now notifies shell on Idle/FAIL; keep a modest
            // safety rearm if CONNECTING is still stale past tip abandon.
            // dump204: SYNC RECV... can freeze the same way (hash 032ddbfd for
            // minutes) — tip hang-skip never paints past RECV, so await's
            // tip_mid_quiet parks forever. Treat bare RECV as stuck-connect
            // with a slightly longer poll gate so mid-window TCP can finish.
            // Tip mid "TIP · SCAN · RX" can freeze the same way as bare RECV...
            // when the shell↔wallet bounce dies mid get_block (hash 985fc0a8).
            let stuck_tip_rx = text.contains("TIP ")
                && text.contains("SCAN")
                && text.contains(" RX")
                && !text.contains("SKIP")
                && !text.contains("INSUFFICIENT");
            let stuck_connect = ((text.contains("CONNECTING")
                || text.contains("WAIT NET")
                || text.contains("RECV..."))
                && !text.contains("TIP ")
                && !text.contains("INSUFFICIENT"))
                || stuck_tip_rx;
            // dump208: 48 still left RECV frozen for minutes when pump notifies
            // were sparse — tighten so await tip_mid_quiet is not the only escape.
            let stuck_gate = if text.contains("RECV...") || stuck_tip_rx {
                24
            } else {
                32
            };
            if stuck_connect {
                if self.wallet_sync_polls % 2 == 0 {
                    SLINT_INPUT.notify();
                }
                WALLET_SERVICE.notify();
                if self.wallet_sync_polls >= stuck_gate {
                    // dump222: refresh() during SYNC CONNECTING / WAIT NET aborts
                    // WaitConnect every ~32 polls, so boot GetInfo never reaches
                    // dhcp settle (FB frozen on CONNECTING). Notify-only; let
                    // wallet connect_lim / tip-abandon finish the attempt.
                    // Same for tip RX / bare RECV — refresh aborts hang-skip mid
                    // get_block and left tip768 forever short of the deposit.
                    if text.contains("CONNECTING")
                        || text.contains("WAIT NET")
                        || text.contains("RECV...")
                        || stuck_tip_rx
                    {
                        self.wallet_connect_keep_cycles =
                            self.wallet_connect_keep_cycles.saturating_add(1);
                        // ~48×32 notify rounds of KEEP with no progress → soft
                        // refresh so WaitConnect/Recv can re-OPEN (TCG stalls
                        // leave FB on Syncing… with empty usernet forever).
                        if self.wallet_connect_keep_cycles >= 48
                            && (text.contains("CONNECTING")
                                || text.contains("WAIT NET")
                                || text.contains("RECV"))
                            && !stuck_tip_rx
                        {
                            debug_println!(
                                "SLINT WALLET CONNECT KEEP REARM cycles={}",
                                self.wallet_connect_keep_cycles
                            );
                            self.wallet_connect_keep_cycles = 0;
                            let queued = self.wallet_service.refresh();
                            self._ui.set_wallet_status(queued.into());
                            self.publish_frame();
                            self.wallet_sync_polls = 0;
                            WALLET_SERVICE.notify();
                            SLINT_INPUT.notify();
                            return false;
                        }
                        debug_println!(
                            "SLINT WALLET CONNECT KEEP polls={} cycles={}",
                            self.wallet_sync_polls,
                            self.wallet_connect_keep_cycles
                        );
                        self.wallet_sync_polls = 0;
                        WALLET_SERVICE.notify();
                        SLINT_INPUT.notify();
                        return false;
                    }
                    debug_println!(
                        "SLINT WALLET CONNECT STUCK REARM polls={}",
                        self.wallet_sync_polls
                    );
                    let queued = self.wallet_service.refresh();
                    self._ui.set_wallet_status(queued.into());
                    self.publish_frame();
                    self.wallet_sync_polls = 0;
                    WALLET_SERVICE.notify();
                    SLINT_INPUT.notify();
                    return false;
                }
            }
        }
        // Notify-only pump — STATUS Call nests shell→wallet→dhcp and stalls RECV.
        WALLET_SERVICE.notify();
        false
    }

    fn apply_nav_back(&mut self) -> bool {
        remember_app_page(self._ui.get_page(), &mut self.nav_last_app_page);
        let changed = shell_navigate_back(&self._ui, self.nav_drafts());
        #[cfg(feature = "utm-transcript-worker")]
        if changed { self.messages_service.borrow().navigation_queue.borrow_mut().cancel(); }
        if changed {
            debug_println!("SLINT NAV BACK -> {}", self._ui.get_page());
        }
        changed
    }

    fn apply_nav_home(&mut self) -> bool {
        remember_app_page(self._ui.get_page(), &mut self.nav_last_app_page);
        let changed = shell_navigate_home(&self._ui, self.nav_drafts());
        #[cfg(feature = "utm-transcript-worker")]
        if changed { self.messages_service.borrow().navigation_queue.borrow_mut().cancel(); }
        if changed {
            debug_println!("SLINT NAV HOME");
        }
        changed
    }

    fn apply_nav_recents(&mut self) -> bool {
        remember_app_page(self._ui.get_page(), &mut self.nav_last_app_page);
        let changed = shell_navigate_recents(&self._ui);
        #[cfg(feature = "utm-transcript-worker")]
        if changed { self.messages_service.borrow().navigation_queue.borrow_mut().cancel(); }
        if changed {
            debug_println!("SLINT NAV RECENTS");
        }
        changed
    }

    fn gesture_begin(&mut self, x: f32, y: f32) {
        let (w, h) = logical_screen();
        let from_edge = x <= NAV_EDGE_PX || x >= w - NAV_EDGE_PX;
        let from_home = y >= h - NAV_HOME_BAND_PX;
        self.nav_gesture = NavGesture {
            active: true,
            // Always defer the Slint press until release so a swipe can become
            // system nav without the UI consuming a drag first.
            defer_press: true,
            // Home cards and the chat list have release-driven actions. Give
            // them a cancellable visual press while retaining swipe detection.
            feedback_press: self._ui.get_page() == 0 || (self._ui.get_page() == 6
                && !self._ui.get_message_view() && !self._ui.get_message_join_view()
                && !self._ui.get_message_invitation_view()),
            start_x: x,
            start_y: y,
            last_x: x,
            last_y: y,
            from_edge,
            from_home,
        };
    }

    fn gesture_move(&mut self, x: f32, y: f32) {
        if self.nav_gesture.active {
            self.nav_gesture.last_x = x;
            self.nav_gesture.last_y = y;
        }
    }

    /// Returns true if a system nav gesture was consumed.
    fn gesture_end(&mut self) -> bool {
        if !self.nav_gesture.active {
            return false;
        }
        let g = self.nav_gesture;
        self.nav_gesture.active = false;
        let dx = g.last_x - g.start_x;
        let dy = g.last_y - g.start_y;
        let (w, h) = logical_screen();
        // Re-evaluate zones from the recorded start in case begin-time flags
        // were computed before the shared pointer settled.
        let from_edge = g.from_edge || g.start_x <= NAV_EDGE_PX || g.start_x >= w - NAV_EDGE_PX;
        let from_home = g.from_home || g.start_y >= h - NAV_HOME_BAND_PX;

        // Horizontal swipe → back (Android). Vertical from the lower screen →
        // home / recents (iPhone). Distance decides; bezel flags are hints
        // because the first tablet sample can lag the absolute event.
        if dx.abs() >= NAV_SWIPE_MIN_PX && dy.abs() <= NAV_SWIPE_MAX_CROSS_PX {
            let _ = w;
            return self.apply_nav_back();
        }
        if dy <= -NAV_SWIPE_MIN_PX && dx.abs() <= NAV_SWIPE_MAX_CROSS_PX {
            if from_home || g.start_y >= h * 0.75 {
                if dy <= -NAV_RECENTS_MIN_PX {
                    return self.apply_nav_recents();
                }
                return self.apply_nav_home();
            }
        }
        false
    }

    #[cfg(feature = "simplex-agent-ui")]
    fn refresh_live_chat(&mut self) {
        #[cfg(feature = "utm-transcript-worker")]
        {
            let service = self.messages_service.borrow();
            if !service.navigation_mailbox.available() {
                // Do not let a periodic refresh block input behind the
                // navigation request currently being served by Messages.
                return;
            }
            if !service.live_receive_mailbox.available() && !service.live_receive_mailbox.finished() {
                // The Messages PD is serving the worker. Avoid a competing
                // synchronous call from the UI and coalesce a follow-up poll.
                service.request_live_receive(false);
                return;
            }
        }
        #[cfg(feature = "desktop-real-app")]
        {
            // The phone has a manual Check action. Desktop dialogs complete
            // asynchronously while native text input retains keyboard focus.
            if self._ui.get_message_invitation_view() && self._ui.get_message_invitation_uri().is_empty() {
                let invitation = self.messages_service.borrow().invitation();
                if let Ok(uri) = invitation {
                    if !uri.is_empty() {
                        present_invitation_qr(&self._ui, uri.as_str());
                        self._ui.set_message_invitation_uri(uri.into());
                        self._ui.set_message_invitation_status("Connection link ready".into());
                    }
                }
            }
            if self._ui.get_message_join_view() && self._ui.get_message_join_requested() {
                self._ui.invoke_check_join_status();
            }
        }
        // Agent LIST finishes after the first Slint load. Pull CHAT rows once
        // the vault snapshot has names so unread badges have real slots.
        let listed = self._ui.get_message_chats().row_count()
            + self._ui.get_message_group_chats().row_count()
            + self._ui.get_message_archived_chats().row_count();
        #[cfg(feature = "utm-transcript-worker")]
        let previous_owners = personal_os_slint_notes_spike::transcript_queue::owners_from_snapshot(
            self._ui.get_message_service_state().as_str(),
        );
        #[cfg(feature = "utm-transcript-worker")]
        let snapshot_result = {
            let service = self.messages_service.borrow();
            match service.take_snapshot() {
                Some(result) => result,
                None => { service.request_snapshot(); return; }
            }
        };
        #[cfg(not(feature = "utm-transcript-worker"))]
        let snapshot_result = profile_live_call!("load", self.messages_service.borrow().load());
        if let Ok(snapshot) = snapshot_result {
            #[cfg(feature = "utm-transcript-worker")]
            let forced = self.messages_service.borrow().force_chat_refresh.replace(false);
            #[cfg(not(feature = "utm-transcript-worker"))]
            let forced = false;
            if forced || listed == 0 || snapshot.as_str() != self._ui.get_message_service_state().as_str() {
                #[cfg(feature = "utm-transcript-worker")]
                {
                    let current_owners = *self.messages_service.borrow().transcript_owners.borrow();
                    if previous_owners != current_owners {
                        let map = personal_os_slint_notes_spike::transcript_queue::remap_by_owner(&previous_owners, &current_owners);
                        let previous = self.chat_transcripts.borrow().clone();
                        *self.chat_transcripts.borrow_mut() = core::array::from_fn(|i| map[i].map_or_else(String::new, |old| previous[old].clone()));
                        let drafts = self.chat_drafts.borrow().clone();
                        *self.chat_drafts.borrow_mut() = core::array::from_fn(|i| map[i].map_or_else(String::new, |old| drafts[old].clone()));
                        let read = self.chat_read_len;
                        self.chat_read_len = core::array::from_fn(|i| map[i].map_or(0, |old| read[old]));
                        let baselined = self.history_baselined;
                        self.history_baselined = core::array::from_fn(|i| map[i].is_some_and(|old| baselined[old]));
                        let initial_unread = self.initial_unread;
                        self.initial_unread = core::array::from_fn(|i| map[i].map_or(0, |old| initial_unread[old]));
                        let unread = self.last_unread;
                        self.last_unread = core::array::from_fn(|i| map[i].map_or(0, |old| unread[old]));
                        if self._ui.get_message_view() {
                            let old_index = self._ui.get_message_chat_index().max(0) as usize;
                            if let Some(index) = map.iter().position(|old| *old == Some(old_index)) {
                                self._ui.set_message_chat_index(index as i32);
                            } else {
                                self._ui.set_message_view(false);
                                self._ui.set_message_transcript("".into());
                                self._ui.set_message_body("".into());
                            }
                        }
                    }
                }
                // The first vault load can fail while the agent is still
                // starting. A later snapshot must leave the placeholder list.
                self._ui.set_message_service_ready(true);
                let mask = profile_live_call!("archive", self.messages_service.borrow().archived_mask()).unwrap_or(0);
                apply_message_chats(
                    &self._ui,
                    &snapshot,
                    mask,
                    Some(&self.chat_transcripts.borrow()),
                );
                self._ui
                    .set_message_has_connections(has_simplex_connection(&snapshot));
                self._ui
                    .set_message_contact_name(contact_name_from_snapshot(&snapshot).into());
                let profile = profile_name_from_snapshot(&snapshot);
                if !profile.is_empty() {
                    self._ui.set_message_profile_name(profile.into());
                }
                self.transcripts_loaded = false;
                profile_live_call!("peer_photos_changed", present_peer_photos(
                    &self.messages_service.borrow(),
                    photo_mask_from_snapshot(&snapshot),
                ));
                self._ui.set_message_service_state(snapshot.into());
            }
        }
        #[cfg(not(feature = "utm-transcript-worker"))]
        let incoming = profile_live_call!("receive", self.messages_service.borrow().receive_status());
        #[cfg(feature = "utm-transcript-worker")]
        let incoming = {
            let service = self.messages_service.borrow();
            match service.take_live_receive() {
                Some(result) => result,
                None => {
                    service.request_live_receive(false);
                    Ok((0, String::new(), 0))
                }
            }
        };
        match incoming {
            Ok((2, text, chat_index)) => {
                keep_received_message(
                    &self._ui,
                    &text,
                    chat_index,
                    &self.chat_transcripts,
                    &self.messages_service,
                );
                present_message_image(&self._ui, &self.messages_service.borrow());
            }
            Ok((7, _, _)) => self._ui.set_message_receive_status("Couldn’t save message to vault. Retrying…".into()),
            Ok((6, _, chat_index)) => {
                // RECEIVE_STATUS already persisted the line; sync the Slint
                // copy so unread badges still move when the UI missed status 2.
                let index = chat_index.min(CHAT_SLOTS - 1);
                #[cfg(feature = "utm-transcript-worker")]
                {
                    let reading = self._ui.get_page() == 6 && self._ui.get_message_view()
                        && self._ui.get_message_chat_index().max(0) as usize == index;
                    if !self.history_baselined[index] && !reading {
                        self.initial_unread[index] = self.initial_unread[index].max(1);
                    }
                    self.messages_service.borrow().history_changed(index);
                }
                if let Ok(saved) = profile_live_call!("incoming_transcript", self.messages_service.borrow().transcript(index as u64)) {
                    let slot = &mut self.chat_transcripts.borrow_mut()[index];
                    if saved.as_str() != slot.as_str() {
                        *slot = saved;
                        debug_println!(
                            "SLINT SIMPLEX TRANSCRIPT_SYNC|{}|{}",
                            index,
                            slot.len()
                        );
                    }
                }
            }
            Err(error) => debug_println!("SLINT SIMPLEX RECEIVE STATUS ERR {}", error),
            _ => {}
        }
        let snapshot = self._ui.get_message_service_state();
        let photo_mask = photo_mask_from_snapshot(snapshot.as_str());
        if photo_mask != 0 {
            profile_live_call!("peer_photos", present_peer_photos(&self.messages_service.borrow(), photo_mask));
        }
        if self._ui.get_page() == 6 && self._ui.get_message_view() {
            #[cfg(feature = "utm-transcript-worker")]
            if self.messages_service.borrow().navigation_legacy_photo_pending.replace(false) {
                present_message_image(&self._ui, &self.messages_service.borrow());
            }
            profile_live_call!("group_invite", present_group_invite(&self._ui, &self.messages_service.borrow()));
            let send_status = profile_live_call!("send", self.messages_service.borrow().send_status());
            match send_status {
                Ok((2, _)) => self._ui.set_message_send_status("Sent".into()),
                Ok((5, _)) => self._ui.set_message_send_status("Photo transfer unverified".into()),
                Ok((7, _)) => self._ui.set_message_send_status("Upload failed · tap Retry".into()),
                Ok((4, _)) => self._ui.set_message_send_status("Send failed".into()),
                Ok((1, _)) => self._ui.set_message_send_status("Sending…".into()),
                _ => {}
            }
            let index = self._ui.get_message_chat_index().max(0) as usize;
            let saved = profile_live_call!("transcript", self.messages_service.borrow().transcript(index as u64)).ok();
            if let Some(saved) = saved {
                if saved.as_str() != self._ui.get_message_transcript().as_str() {
                    self.chat_transcripts.borrow_mut()[index.min(CHAT_SLOTS - 1)] = saved.clone();
                    self._ui.set_message_transcript(saved.clone().into());
                }
                let delivered = saved.lines().rev().find(|line| line.starts_with("NexatrodeOS: ")).is_some_and(|line| line.ends_with(" \u{2713}"));
                if delivered && self._ui.get_message_send_status().as_str() == "Sent" {
                    self._ui.set_message_send_status("Delivered".into());
                }
            }
        }
    }

    #[cfg(feature = "simplex-agent-ui")]
    fn poll_live_chat(&mut self) {
        #[cfg(feature = "utm-perf-trace")]
        let started = perf_time_us();
        self.refresh_live_chat();
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|LIVE_POLL|{}|{}|{}", started, perf_time_us(), u8::from(self.defer_frame));
    }

    /// Apply taps and keys the input PD queued in the focus page.
    /// A protected call cannot land while this handler is in the sync pump.
    fn drain_posted_input(&mut self) {
        let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
        if flag.is_null() {
            return;
        }
        let base = unsafe { flag.sub(1) };
        let head = unsafe { &*base.add(2).cast::<core::sync::atomic::AtomicU32>() };
        let tail_atom = unsafe { &*base.add(3).cast::<core::sync::atomic::AtomicU32>() };
        let mut tail = tail_atom.load(Ordering::Relaxed);
        let mut applied = 0u32;
        self.defer_frame = true;
        loop {
            let head_now = head.load(Ordering::Acquire);
            if tail == head_now || applied >= 400 {
                break;
            }
            let slot = (tail % 224) as usize;
            let record = unsafe { base.add(4 + slot * 4) };
            let dev_type = unsafe { core::ptr::read_volatile(record) };
            let code = unsafe { core::ptr::read_volatile(record.add(1)) } as u64;
            let value = unsafe { core::ptr::read_volatile(record.add(2)) } as u64;
            let position = unsafe { core::ptr::read_volatile(record.add(3)) } as u64;
            POSTED_DEVICE.store((dev_type & 0xffff) as u64, Ordering::Relaxed);
            POSTED_KIND.store((dev_type as u64) >> 16, Ordering::Relaxed);
            POSTED_CODE.store(code, Ordering::Relaxed);
            POSTED_VALUE.store(value, Ordering::Relaxed);
            POSTED_POSITION.store(position, Ordering::Relaxed);
            POSTED_ACTIVE.store(true, Ordering::Release);
            #[cfg(feature = "utm-device-lock")]
            let secret_input = self._ui.get_device_lock_screen();
            let _ = self.handle_input(SLINT_INPUT, MessageInfo::new(REQUEST_SLINT_EVENT, 3));
            #[cfg(feature = "utm-device-lock")]
            if secret_input || self._ui.get_device_lock_screen() {
                // The producer cannot reuse this slot until tail is advanced.
                for offset in 0..4 { unsafe { core::ptr::write_volatile(record.add(offset), 0); } }
                POSTED_DEVICE.store(0, Ordering::Relaxed);
                POSTED_KIND.store(0, Ordering::Relaxed);
                POSTED_CODE.store(0, Ordering::Relaxed);
                POSTED_VALUE.store(0, Ordering::Relaxed);
                POSTED_POSITION.store(0, Ordering::Relaxed);
            }
            tail = tail.wrapping_add(1);
            // Free the slot before the next key arrives so a full address fits.
            tail_atom.store(tail, Ordering::Release);
            applied += 1;
        }
        self.defer_frame = false;
        if self.frame_dirty {
            self.frame_dirty = false;
            self.publish_frame();
            // The input service may have woken the GPU before this queued
            // frame was published. Wake it again after the pixels are ready.
            SLINT_INPUT.notify();
        }
        // Platforms without a frame timer still perform the refresh after the
        // input frame rather than silently dropping incoming updates.
        #[cfg(feature = "simplex-agent-ui")]
        if self.frame_timer.is_none() && self.chat_refresh.take()
            && self._ui.get_page() == 6 && (cfg!(feature = "desktop-real-app") || self._ui.get_edit_target() == 0)
        {
            self.poll_live_chat();
            self.publish_frame();
            SLINT_INPUT.notify();
        }
    }
}

impl SlintService {
    #[cfg(all(feature = "simplex-agent-ui", not(feature = "utm-photo-worker")))]
    fn load_visible_chat_photo_chunk(&mut self) {
        use personal_os_slint_notes_spike::chat_photo_loader::PhotoOutcome;
        #[cfg(feature = "utm-photo-worker")]
        if self.pending_photo.is_some() { return; }
        if self._ui.get_page() != 6 || !self._ui.get_message_view() { return; }
        let chat = self._ui.get_message_chat_index().max(0) as u64;
        let transcript = self._ui.get_message_transcript();
        let stamp = transcript_stamp(transcript.as_str()).rotate_left(8) ^ chat;
        let request = {
            let loader = self.chat_photo_loader.borrow();
            if !loader.matches(stamp, chat) { return; }
            loader.next_request()
        };
        let Some((chat, id, part)) = request else { return };
        #[cfg(feature = "utm-perf-trace")]
        let started = perf_time_us();
        let chunk = {
            let service = self.messages_service.borrow();
            service.read_attachment_part(chat, id, part).and_then(|total| {
                unsafe { service.transfer.as_ref() }.payload().map(|bytes| (total, bytes.to_vec()))
            })
        };
        let result = self.chat_photo_loader.borrow_mut().accept(
            chunk.as_ref().map(|(total, bytes)| (*total, bytes.as_slice())),
        );
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|PHOTO_CHUNK|{}|{}|{}", started, perf_time_us(), part);
        let (row, image) = match result {
            PhotoOutcome::Pending => return,
            PhotoOutcome::Complete { row, bytes } => {
                #[cfg(feature = "utm-photo-worker")]
                {
                    if self.photo_mailbox.submit(|output| {
                        if bytes.len() > output.len() { return None; }
                        output[..bytes.len()].copy_from_slice(&bytes);
                        Some(bytes.len())
                    }) {
                        self.pending_photo = Some((stamp, chat, row, id));
                        #[cfg(feature = "utm-perf-trace")]
                        debug_println!("PERF|PHOTO_SUBMIT|{}|{}", perf_time_us(), id);
                        PHOTO_WORKER.notify();
                        return;
                    }
                    (row, None)
                }
                #[cfg(not(feature = "utm-photo-worker"))]
                {
                    let image = decode_jpeg_preview(&bytes);
                    if image.is_some() { debug_println!("SLINT SIMPLEX MESSAGE PHOTO {} {}", id, bytes.len()); }
                    (row, image)
                }
            },
            PhotoOutcome::Failed { row } => (row, None),
        };
        if let Some(image) = image {
            self._ui.get_message_bubble_photos().set_row_data(row, image);
            self._ui.get_message_bubble_photo_ready().set_row_data(row, true);
        }
        self._ui.get_message_bubble_photo_pending().set_row_data(row, false);
    }

    #[cfg(feature = "utm-photo-worker")]
    fn load_visible_chat_photo_chunk(&mut self) {
        if self.pending_photo.is_some() || self.pending_photo_read.is_some()
            || self._ui.get_page() != 6 || !self._ui.get_message_view() { return; }
        let chat = self._ui.get_message_chat_index().max(0) as u64;
        let stamp = transcript_stamp(self._ui.get_message_transcript().as_str()).rotate_left(8) ^ chat;
        let owner = {
            let service = self.messages_service.borrow();
            let history = service.history_cache.borrow();
            history.get(chat as usize).and_then(|entry| entry.as_ref().map(|entry| entry.0))
        };
        let job = {
            let mut loader = self.chat_photo_loader.borrow_mut();
            if !loader.matches(stamp, chat) { return; }
            loader.claim_visible()
        };
        let Some((_, id, row)) = job else { return; };
        let Some(owner) = owner.filter(|owner| *owner != 0 && *owner != u64::MAX) else {
            self._ui.get_message_bubble_photo_pending().set_row_data(row, false);
            debug_println!("SLINT SIMPLEX PHOTO OWNER UNAVAILABLE");
            return;
        };
        let cached = self.photo_cache.borrow_mut().get(owner, id);
        if let Some(image) = cached {
            self._ui.get_message_bubble_photos().set_row_data(row, image);
            self._ui.get_message_bubble_photo_ready().set_row_data(row, true);
            self._ui.get_message_bubble_photo_pending().set_row_data(row, false);
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|PHOTO_CACHE_HIT|{}|{}", perf_time_us(), id);
            return;
        }
        self._ui.get_message_bubble_photo_ready().set_row_data(row, false);
        self._ui.get_message_bubble_photo_pending().set_row_data(row, true);
        let request = personal_os_slint_notes_spike::photo_reader_ipc::request(owner, id);
        if self.photo_read_mailbox.submit(|bytes| { bytes[..request.len()].copy_from_slice(&request); Some(request.len()) }) {
            let cache_generation = self.photo_cache.borrow().generation();
            self.pending_photo_read = Some(PhotoTicket { stamp, chat, row, id, owner, cache_generation });
            #[cfg(feature = "utm-perf-trace")]
            debug_println!("PERF|PHOTO_READ_SUBMIT|{}|{}", perf_time_us(), id);
            PHOTO_READER.notify();
        } else {
            self._ui.get_message_bubble_photo_pending().set_row_data(row, false);
        }
    }

    #[cfg(feature = "utm-photo-worker")]
    fn photo_ticket_current(&self, ticket: PhotoTicket) -> bool {
        let chat = self._ui.get_message_chat_index().max(0) as u64;
        let stamp = transcript_stamp(self._ui.get_message_transcript().as_str()).rotate_left(8) ^ chat;
        self.photo_cache.borrow().generation() == ticket.cache_generation
            && personal_os_slint_notes_spike::photo_ipc::preview_matches(
                ticket.stamp, ticket.chat, stamp, chat,
                self.chat_photo_loader.borrow().matches(ticket.stamp, ticket.chat),
            )
    }

    #[cfg(feature = "utm-photo-worker")]
    fn finish_photo_read(&mut self) {
        use personal_os_slint_notes_spike::photo_mailbox::Completion;
        let Some(ticket) = self.pending_photo_read else { return; };
        let current = self.photo_ticket_current(ticket);
        let result = self.photo_read_mailbox.take(|bytes| {
            current && self.photo_mailbox.submit(|out| {
                if bytes.len() > out.len() { return None; }
                out[..bytes.len()].copy_from_slice(bytes);
                Some(bytes.len())
            })
        });
        let submitted = match result {
            Completion::Waiting => return,
            Completion::Failed => false,
            Completion::Ready(submitted) => submitted,
        };
        self.pending_photo_read = None;
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|PHOTO_READ_READY|{}|{}|{}", perf_time_us(), ticket.id, u8::from(submitted));
        if submitted {
            self.pending_photo = Some(ticket);
            PHOTO_WORKER.notify();
        } else if current {
            self.chat_photo_loader.borrow_mut().complete(ticket.row);
            self._ui.get_message_bubble_photo_pending().set_row_data(ticket.row, false);
        }
    }

    #[cfg(feature = "utm-photo-worker")]
    fn finish_photo_preview(&mut self) {
        use personal_os_slint_notes_spike::photo_mailbox::Completion;
        let Some(ticket) = self.pending_photo else { return; };
        let current = self.photo_ticket_current(ticket);
        let result = self.photo_mailbox.take(|data| {
            if !current { return None; }
            let (width, height, pixels) = personal_os_slint_notes_spike::photo_ipc::preview_parts(data)?;
            let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width as u32, height as u32);
            buffer.make_mut_bytes().copy_from_slice(pixels);
            Some((slint::Image::from_rgba8(buffer), pixels.len()))
        });
        let image = match result {
            Completion::Waiting => return,
            Completion::Failed => None,
            Completion::Ready(image) => image,
        };
        self.pending_photo = None;
        #[cfg(feature = "utm-perf-trace")]
        debug_println!("PERF|PHOTO_READY|{}|{}", perf_time_us(), ticket.id);
        if !current { return; }
        self.chat_photo_loader.borrow_mut().complete(ticket.row);
        if let Some((image, bytes)) = image {
            self.photo_cache.borrow_mut().insert(ticket.owner, ticket.id, bytes, image.clone());
            self._ui.get_message_bubble_photos().set_row_data(ticket.row, image);
            self._ui.get_message_bubble_photo_ready().set_row_data(ticket.row, true);
            self.window.request_redraw();
            debug_println!("SLINT SIMPLEX MESSAGE PHOTO {} worker", ticket.id);
        }
        self._ui.get_message_bubble_photo_pending().set_row_data(ticket.row, false);
    }

    fn arm_frame_timer(&mut self) {
        let Some(channel) = self.frame_timer else { return };
        let mut delay = personal_os_slint_notes_spike::guest_platform::next_wake(&self.window);
        if clock_view_address() != 0 {
            let poll = core::time::Duration::from_secs(1);
            delay = Some(delay.map_or(poll, |wake| wake.min(poll)));
        }
        #[cfg(feature = "utm-device-lock")]
        if self.device_lock.borrow().enabled() {
            let lock_poll = core::time::Duration::from_millis(250);
            delay = Some(delay.map_or(lock_poll, |wake| wake.min(lock_poll)));
        }
        #[cfg(feature = "simplex-agent-ui")]
        if self.chat_refresh.pending() {
            // Give scanout a chance to show the input frame before the
            // synchronous messages service calls begin.
            let refresh_delay = core::time::Duration::from_millis(16);
            delay = Some(delay.map_or(refresh_delay, |wake| wake.min(refresh_delay)));
        }
        #[cfg(feature = "utm-photo-worker")]
        let photo_worker_idle = self.pending_photo.is_none() && self.pending_photo_read.is_none();
        #[cfg(all(feature = "simplex-agent-ui", not(feature = "utm-photo-worker")))]
        let photo_worker_idle = true;
        #[cfg(feature = "simplex-agent-ui")]
        if photo_worker_idle && self._ui.get_page() == 6 && self._ui.get_message_view()
            && self.chat_photo_loader.borrow().next_request().is_some()
        {
            let photo_delay = core::time::Duration::from_millis(50);
            delay = Some(delay.map_or(photo_delay, |wake| wake.min(photo_delay)));
        }
        #[cfg(feature="desktop-simplex-frontend")]
        { let tick=core::time::Duration::from_millis(100); delay=Some(delay.map_or(tick,|d|d.min(tick))); }
        let Some(delay) = delay else {
            // The timer API has no cancellation; a stale one-shot can wake us once.
            return;
        };
        use personal_os_slint_notes_spike::{frame_clock::elapsed, guest_platform::{counter_frequency, counter_ticks}};
        let now = elapsed(counter_ticks(), counter_frequency());
        let deadline = now.saturating_add(delay);
        if self.frame_deadline.is_some_and(|armed| armed <= deadline) {
            return;
        }
        self.frame_deadline = Some(deadline);
        set_mr(0, delay.as_nanos().clamp(1, usize::MAX as u128) as _);
        let _ = channel.pp_call(MessageInfo::new(1, 1)); // sDDF SET_TIMEOUT, nanoseconds
    }

    fn handle_notification(&mut self, channels: ChannelSet) -> Result<(), Infallible> {
        #[cfg(feature = "utm-chat-sync-selftest")]
        {
            static STAGE: AtomicU64 = AtomicU64::new(0);
            if STAGE.load(Ordering::Acquire) == 0 {
                self.wallet_sync_active = true;
                self.wallet_boot_phase = 5;
                skia_guest::note_chat_name(0, b"Test contact");
                self._ui.set_page(6);
                self._ui.set_message_service_ready(true);
                self.publish_frame();
                skia_guest::flush_chat_sprites();
                debug_println!("SLINT CHAT SYNC SPRITE {}", if skia_guest::test_chat_name_ready(0) { "PASS" } else { "FAIL" });
                self.wallet_boot_phase = 0;
                self.needs_catch_up = true;
                STAGE.store(2, Ordering::Release);
                // Exercise the normal notification path while wallet sync is
                // active. ChannelSet wraps the seL4 badge in this test build.
                let badge: usize = 1 << SLINT_INPUT.index();
                let test_channels: ChannelSet = unsafe { core::mem::transmute(badge) };
                let _ = self.notified(test_channels);
                debug_println!("SLINT CHAT SYNC CATCHUP {}", if !self.needs_catch_up { "PASS" } else { "FAIL" });
                self.wallet_sync_active = true;
                self.wallet_sync_polls = 39_999;
                unsafe { self.wallet_service.transfer.as_mut() }.set_payload(b"NEXAWAL | FETCH TXS...");
                let _ = self.poll_wallet_sync_once();
                debug_println!("SLINT WALLET LONG SYNC {}", if self.wallet_sync_active { "PASS" } else { "FAIL" });
                debug_println!("SLINT WALLET FAILURE TERMINAL {}", if WalletServiceClient::sync_terminal("NEXAWAL | TLS TIMEOUT") && WalletServiceClient::sync_terminal("NEXAWAL | SYNC FAIL") && !WalletServiceClient::sync_terminal("NEXAWAL | FETCH TXS...") { "PASS" } else { "FAIL" });
                self._ui.set_wallet_sync("Syncing…".into());
                self.wallet_sync_active = true;
                unsafe { self.wallet_service.transfer.as_mut() }.set_payload(b"NEXAWAL | TLS TIMEOUT");
                let _ = self.poll_wallet_sync_once();
                debug_println!("SLINT WALLET ERROR DISPLAY {}", if self._ui.get_wallet_sync().as_str() != "Syncing…" { "PASS" } else { "FAIL" });
                WALLET_SERVICE.notify();
                let before = HEAP_USED.load(Ordering::Relaxed);
                for _ in 0..160 {
                    let mut temporary = alloc::vec![0u8; 256 * 1024];
                    temporary[0] = 1;
                    core::hint::black_box(&temporary);
                }
                let growth = HEAP_USED.load(Ordering::Relaxed).saturating_sub(before);
                debug_println!("SLINT ALLOC REUSE {} growth={}", if growth < 1024 * 1024 { "PASS" } else { "FAIL" }, growth);
                self.wallet_sync_active = false;
                self.needs_catch_up = false;
            }
        }
        #[cfg(feature = "utm-keyboard-selftest")]
        {
            static TESTED: AtomicBool = AtomicBool::new(false);
            if !TESTED.swap(true, Ordering::AcqRel) {
                self._ui.set_page(6);
                self._ui.set_message_view(true);
                self._ui.set_message_selected("Keyboard test".into());
                self._ui.set_message_body("Typing test".into());
                self._ui.set_edit_target(6);
                self._ui.set_wallet_available(false);
                self.wallet_boot_phase = 0;
                self.publish_frame();
            }
        }
        #[cfg(feature = "utm-chat-list-selftest")]
        {
            static TESTED: AtomicBool = AtomicBool::new(false);
            if !TESTED.swap(true, Ordering::AcqRel) {
                let initial = "CHAT TestA\nCHAT TestB\nCHAT TestC\nCHAT TestD\n";
                let updated = "CHAT TestA\nCHAT TestB\nCHAT TestC\nCHAT TestD\nCHAT TestE\n";
                self._ui.set_message_service_ready(true);
                apply_message_chats(&self._ui, initial, 0, None);
                apply_message_chats(&self._ui, updated, 0, None);
                self._ui.set_page(6);
                self._ui.set_message_list_offset(-72.0);
                self.wallet_boot_phase = 0;
                self.publish_frame();
                let count_ok = self._ui.get_message_chats().row_count() == 5;
                let scroll_ok = (self._ui.get_message_list_scroll() - 72.0).abs() < 0.5;
                debug_println!("SLINT CHAT_LIST_SELFTEST|{}|{}", if count_ok { "ROWS_PASS" } else { "ROWS_FAIL" }, if scroll_ok { "SCROLL_PASS" } else { "SCROLL_FAIL" });
            }
        }
        #[cfg(feature = "utm-files-selftest")]
        {
            static TESTED: AtomicBool = AtomicBool::new(false);
            if !TESTED.swap(true, Ordering::AcqRel) {
                // Start the agent as well as the file-library path so its
                // independent upload-stage self-test can run in this fixture.
                let _ = self.messages_service.borrow().load();
                self._ui.invoke_import_test_file();
                let imported = self._ui.get_files_names().row_count() > 0;
                self._ui.invoke_attach_picture(true);
                let picked = self._ui.get_message_photo_picker_labels().row_count() > 0;
                let preview = self._ui.get_message_photo_picker_images().row_data(0)
                    .is_some_and(|image| image.size().width > 0 && image.size().height > 0);
                debug_println!("SLINT FILES_SELFTEST|{}|{}|{}", if imported { "FILES_PASS" } else { "FILES_FAIL" }, if picked { "PICKER_PASS" } else { "PICKER_FAIL" }, if preview { "PREVIEW_PASS" } else { "PREVIEW_FAIL" });
                self._ui.set_page(9);
                self.wallet_boot_phase = 0;
                self.publish_frame();
            }
        }
        self.drain_posted_input();
        // Handle visible-app updates before the wallet pump can return early.
        #[cfg(feature = "simplex-agent-ui")]
        if channels.contains(SLINT_INPUT) && self.needs_catch_up {
            // Sync the screen the user just opened, then ask for one hidden
            // picture on the following turn. Do not paint that picture now:
            // a chain of them kept HOME waiting.
            self.needs_catch_up = false;
            debug_println!("SLINT PAGE SYNC {}", self.shown_view);
            self.publish_frame();
            if Self::picture_slot(self.shown_view).is_some() && self.next_background_view().is_some()
            {
                self.prepare_one = true;
                let flag = CATCH_UP_FLAG.load(Ordering::Acquire);
                if !flag.is_null() {
                    unsafe { flag.write_volatile(1) };
                }
            }
            SLINT_INPUT.notify();
        } else if channels.contains(SLINT_INPUT) && self.prepare_one && !self.wallet_sync_active {
            self.prepare_one = false;
            if self.prepare_next_home_card() {
                if self.shown_view == 6 {
                    self.overlay_chat_names();
                }
                self.publish_copied_frame();
            }
            SLINT_INPUT.notify();
        }
        #[cfg(feature = "simplex-agent-ui")]
        if channels.contains(MESSAGES_SERVICE) {
            // A service notification already provides the pending refresh.
            // Avoid repeating the same synchronous poll on the frame timer.
            self.chat_refresh.take();
            self.poll_live_chat();
            self.publish_frame();
            // Input owns the scanout. A message must show without another tap.
            SLINT_INPUT.notify();
        }
        // Boot + sync pumps: input catch-up (one-shot leave ARMED) and wallet pings.
        // Do NOT re-arm ui_focus[1] / SLINT_INPUT here — that bounce starves
        // gpu_client (prio 1) on 1-core TCG and leaves a black FB.
        let from_pump = channels.contains(SLINT_INPUT) || channels.contains(WALLET_SERVICE);
        // First handler entry after init open+queue: start the notify pump even
        // if only a catch-up / focus pulse arrived.
        if self.wallet_boot_phase == 5 && self.wallet_sync_polls == 0 {
            WALLET_SERVICE.notify();
            SLINT_INPUT.notify();
        }
        if from_pump && self._ui.get_wallet_node_status().as_str() == "Saving node…" {
            if let Some(status) = self.wallet_service.peek_transfer() {
                if status.contains("NODE SAVED") {
                    self._ui.set_wallet_node_status("Node saved · verifying network".into());
                    let queued = self.wallet_service.queue_refresh();
                    self._ui.set_wallet_status(queued.into());
                    self.begin_sync_poll();
                } else if status.contains("NODE SAVE FAILED") {
                    self._ui.set_wallet_node_status("Node save failed; check and retry".into());
                }
            }
            self.publish_frame();
            return Ok(());
        }
        if from_pump && (6..=9).contains(&self._ui.get_wallet_setup()) {
            if let Some(status) = self.wallet_service.peek_transfer() {
                match self._ui.get_wallet_setup() {
                    6 if !status.starts_with("NEXAWAL | ") && status.split_whitespace().count() == 25 => {
                        self._ui.set_wallet_backup(status.into());
                        unsafe { self.wallet_service.transfer.as_mut() }.set_payload(&[]);
                        self._ui.set_wallet_setup_status("Keep these words private".into());
                        self._ui.set_wallet_setup(2);
                        debug_println!("SLINT WALLET BACKUP READY");
                    }
                    mode @ (7 | 9) if status.contains("WALLET SAVED") => {
                        let words = if mode == 7 { self._ui.get_wallet_backup() } else { self._ui.get_wallet_words() };
                        let started = !words.is_empty()
                            && unsafe { self.wallet_service.transfer.as_mut() }.set_payload(words.as_bytes())
                            && self.wallet_service.call(0x50e).is_ok();
                        self._ui.set_wallet_backup("".into());
                        self._ui.set_wallet_words("".into());
                        debug_println!("SLINT WALLET IDENTITY SAVED");
                        if started {
                            self._ui.set_wallet_setup_status("Opening wallet…".into());
                            self._ui.set_wallet_setup(8);
                            WALLET_SERVICE.notify();
                        } else {
                            unsafe { self.wallet_service.transfer.as_mut() }.set_payload(&[]);
                            self._ui.set_wallet_setup_status("Saved. Enter your seed to unlock".into());
                            self._ui.set_wallet_setup(5);
                        }
                    }
                    8 if status.contains("OPEN ") => {
                        self._ui.set_wallet_setup(0);
                        debug_println!("SLINT WALLET UNLOCKED");
                        self._ui.set_wallet_status(status.clone().into());
                        apply_wallet_face(&self._ui, &status);
                        if let Ok(endpoint) = self.wallet_service.call(0x511) {
                            let kind = if endpoint.ends_with(":443") { "Saved HTTPS node · host:port" } else { "Saved HTTP node · host:port" };
                            #[cfg(feature = "utm-nexawal-rpc-bridge")]
                            let endpoint = if endpoint == "rpc.nexawal.com:443" || endpoint == "10.0.2.2:18881" { "rpc.nexawal.com".into() } else { endpoint };
                            self._ui.set_wallet_node(endpoint.into());
                            self._ui.set_wallet_node_status(kind.into());
                        }
                        let queued = self.wallet_service.queue_refresh();
                        self._ui.set_wallet_status(queued.into());
                        self.begin_sync_poll();
                        self.wallet_face_tab_dirty = true;
                    }
                    _ if status.contains("WALLET ALREADY EXISTS") || status.contains("IDENTITY CONFLICT") => {
                        let _ = self.wallet_service.call(0x50b);
                        self._ui.set_wallet_backup("".into());
                        self._ui.set_wallet_words("".into());
                        self._ui.set_wallet_setup(1);
                        self._ui.set_wallet_setup_status("Wallet storage is already in use. Unlock your existing wallet.".into());
                    }
                    mode if status.contains("FAILED") || status.contains("INVALID BACKUP") => {
                        if mode == 8 { debug_println!("SLINT WALLET UNLOCK FAILED"); }
                        if mode == 9 { let _ = self.wallet_service.call(0x50b); }
                        let message = if status.contains("CREATION HEIGHT FAILED") {
                            "Cannot get the current network height. Check your node and try again."
                        } else if status.contains("INVALID BACKUP") {
                            "Invalid recovery words. Check them and try again."
                        } else if mode == 8 {
                            "Could not unlock. Check your words or wallet storage."
                        } else {
                            "Wallet storage or operation failed. Your existing wallet was not replaced."
                        };
                        self._ui.set_wallet_setup_status(message.into());
                        self._ui.set_wallet_setup(match mode { 6 => 1, 7 => 2, 9 => 4, _ => 5 });
                    }
                    _ => (),
                }
            }
            if self._ui.get_wallet_setup() == 6 {
                WALLET_SERVICE.notify();
            }
            self.publish_frame();
            return Ok(());
        }
        if from_pump && self.wallet_tip_rearm_cool > 0 {
            self.wallet_tip_rearm_cool = self.wallet_tip_rearm_cool.saturating_sub(1);
        }
        // dump213: SYNC RECV... UI can lag after tip Idle+SCAN 0 while
        // wallet_sync_active or tip_rearm_cool still blocks the park rearm
        // arm — await then tip_mid_quiet forever. Always peek on RECV; paint
        // terminal immediately (ignore cool/active). Only refresh when the
        // transfer is still non-terminal mid-flight and cool is drained.
        // dump213b: same lag for BOOT PHASE ARMED after tip already finished
        // (OCR stayed ebfff333 until a tap painted INSUFFICIENT).
        if from_pump && self._ui.get_page() == 16
            && self._ui.get_wallet_send_status().as_str() == "Checking transaction…"
        {
            if let Some(status) = self.wallet_service.peek_transfer() {
                if status.contains("TRANSACTION IN POOL") {
                    self._ui.set_wallet_send_status("Node reports transaction in pool".into());
                } else if let Some(height) = status.split("TRANSACTION IN BLOCK ").nth(1) {
                    self._ui.set_wallet_send_status(alloc::format!("Node reports block {}", height).into());
                } else if status.contains("TRANSACTION NOT FOUND") {
                    self._ui.set_wallet_send_status("Not found at node · outcome unknown".into());
                } else if status.contains("FAIL") {
                    self._ui.set_wallet_send_status("Transaction check failed".into());
                } else {
                    WALLET_SERVICE.notify();
                }
            }
        }
        if from_pump && self._ui.get_page() == 16 {
            let s = self._ui.get_wallet_status();
            if s.as_str().contains("ARMED") || s.as_str().contains("RECV...") {
                if s.as_str().contains("ARMED") {
                    if let Some(peeked) = self.wallet_service.peek_transfer() {
                        if WalletServiceClient::sync_terminal(peeked.as_str())
                            || peeked.contains("CONNECTING")
                            || peeked.contains("QUEUED")
                            || peeked.contains("RECV...")
                            || (peeked.contains("TIP ") && peeked.contains("SCAN"))
                        {
                            self.wallet_boot_phase = 0;
                            self.wallet_sync_active = !WalletServiceClient::sync_terminal(
                                peeked.as_str(),
                            );
                            self._ui.set_wallet_status(peeked.clone().into());
                            if WalletServiceClient::sync_terminal(peeked.as_str()) {
                                self.wallet_tip_rearm_cool = 30;
                            } else {
                                self.begin_sync_poll();
                            }
                            self.publish_frame();
                            return Ok(());
                        }
                    }
                }
            }
            if s.as_str().contains("RECV...") {
                if let Some(peeked) = self.wallet_service.peek_transfer() {
                    if WalletServiceClient::sync_terminal(peeked.as_str()) {
                        self.wallet_sync_active = false;
                        self._ui.set_wallet_status(peeked.into());
                        self.wallet_tip_rearm_cool = 30;
                        self.publish_frame();
                        return Ok(());
                    }
                }
                if !self.wallet_sync_active && self.wallet_tip_rearm_cool == 0 {
                    let queued = self.wallet_service.refresh();
                    self._ui.set_wallet_status(queued.into());
                    self.publish_frame();
                    self.wallet_tip_rearm_cool = 45;
                    self.begin_sync_poll();
                    return Ok(());
                }
                // Live Recv: keep the notify bounce so tip hang-skip can finish.
                WALLET_SERVICE.notify();
                if self.wallet_sync_active {
                    let _ = self.poll_wallet_sync_once();
                }
                return Ok(());
            }
        }
        // Timer pulse after pump died mid TIP·SCAN / BAD HTTP — refresh with
        // cooldown so we do not abort get_block every pulse (dump105 thrash).
        // dump124/125: CONNECTING must NOT hard-refresh (self-thrash). Tip RX
        // is still in-flight — notify-only so hang-skip can reach SCAN 0.
        if from_pump
            && !self.wallet_sync_active
            && self.wallet_tip_rearm_cool == 0
            && self._ui.get_page() == 16
        {
            let s = self._ui.get_wallet_status();
            let tip_mid = s.as_str().contains("TIP ") && s.as_str().contains("SCAN");
            let tip_rx = tip_mid
                && (s.as_str().contains(" RX")
                    || s.as_str().contains(" FETCH")
                    || s.as_str().contains(" NET"));
            // Tip RX mid-flight: notify-only (refresh aborts hang-skip).
            if tip_rx && !s.as_str().contains("INSUFFICIENT") {
                WALLET_SERVICE.notify();
                self.begin_sync_poll();
                self.wallet_tip_rearm_cool = 120;
                return Ok(());
            }
            // dump156: bare SYNC CONNECTING + notify-only left wallet Idle with
            // a stale CONNECTING transfer — tip hunt never restarted. Re-queue.
            if s.as_str().contains("CONNECTING") && !s.as_str().contains("INSUFFICIENT")
            {
                let queued = self.wallet_service.refresh();
                self._ui.set_wallet_status(queued.into());
                self.publish_frame();
                self.wallet_tip_rearm_cool = 60;
                self.begin_sync_poll();
                return Ok(());
            }
            let rearm = (tip_mid && !WalletServiceClient::sync_terminal(s.as_str()))
                || s.as_str().contains("BAD HTTP")
                || s.as_str().contains("CONNECT FAIL")
                || s.as_str().contains("RECV FAIL")
                || s.as_str().contains("WAIT NET")
                || s.as_str().contains("NEED SCAN")
                || s.as_str().contains("SCAN FAIL")
                || s.as_str().contains("SYNC QUEUED")
                // Parked zero-bal / empty tip: keep tip-hunting for a deposit.
                || s.as_str().contains("INSUFFICIENT")
                || s.as_str().contains("SCAN 0")
                || s.as_str().contains("| SCAN 0")
                || s.as_str().contains("SCAN OK")
                || s.as_str().contains("| SCAN OK")
                || s.as_str().split("| SCAN ").nth(1).is_some_and(|rest| {
                    rest.trim_start()
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit())
                });
            if rearm {
                // dump193: SCAN OK already has outs — prepare, don't tip-refresh.
                // dump204: SCAN 0 was tip-refreshing forever (hash 90e2373b), so
                // prepare never painted INSUFFICIENT and tip-rearm had no park
                // state. Prepare first; INSUFFICIENT cool then tip-hunts deposit.
                // Standard face also publishes "| SCAN <n>" via format_status.
                let scan_done = scan_complete(s.as_str());
                if scan_done {
                    // Scan finished. Do not prepare or broadcast; Send does
                    // that only after the user previews the fee and confirms.
                    self.wallet_sync_active = false;
                    self.wallet_tip_rearm_cool = 60;
                    return Ok(());
                }
                let queued = self.wallet_service.refresh();
                self._ui.set_wallet_status(queued.into());
                self.publish_frame();
                // Longer cool on park states so we do not abort mid CONNECTING.
                // dump145: 180 pulses left tip hunt too sparse for a fresh
                // deposit; 90 still parks PREPARE INSUFFICIENT long enough.
                self.wallet_tip_rearm_cool = if s.as_str().contains("INSUFFICIENT") {
                    60
                } else {
                    45
                };
                self.begin_sync_poll();
                return Ok(());
            }
        }
        if from_pump && self.wallet_boot_phase != 0 {
            let phase_before = self.wallet_boot_phase;
            let _keep = self.advance_wallet_boot();
            // dump223: OPEN mainnet park — force tip queue even if phase-5
            // peek path missed (sync_active true blocked the later rearm).
            {
                let s = self._ui.get_wallet_status();
                let parked_open = (s.as_str().contains("OPEN mainnet")
                    || s.as_str().contains("H3519450"))
                    && !s.as_str().contains("CONNECTING")
                    && !s.as_str().contains("QUEUED")
                    && !s.as_str().contains("RECV")
                    && !s.as_str().contains("WAIT NET")
                    && !s.as_str().contains("TIP ")
                    && !s.as_str().contains("SCAN");
                if parked_open {
                    let queued = self.wallet_service.queue_refresh();
                    self._ui.set_wallet_status(
                        alloc::format!("{} · {queued}", s.as_str()).into(),
                    );
                    self.publish_frame();
                    self.begin_sync_poll();
                    return Ok(());
                }
            }
            if phase_before == 5 || self.wallet_boot_phase == 5 {
                // Keep polling wallet; also nudge input so status frames scan out
                // (otherwise FB freezes on READY while sync runs).
                WALLET_SERVICE.notify();
                if self.wallet_sync_polls % 8 == 0 {
                    SLINT_INPUT.notify();
                }
            } else if self.wallet_boot_phase != 0 {
                // Ask input to scan out this phase; gpu pulses the next phase.
                SLINT_INPUT.notify();
            } else if phase_before != 0 {
                // dump222: boot phase 4→0 after CONNECT FAIL stopped the notify
                // bounce, so tip rearm never ran and FB froze on CONNECT FAIL.
                let s = self._ui.get_wallet_status();
                if s.as_str().contains("CONNECT FAIL")
                    || s.as_str().contains("RECV FAIL")
                    || s.as_str().contains("WAIT NET")
                {
                    self.wallet_tip_rearm_cool = 0;
                    let queued = self.wallet_service.refresh();
                    self._ui.set_wallet_status(queued.into());
                    self.publish_frame();
                    self.begin_sync_poll();
                } else {
                    WALLET_SERVICE.notify();
                    SLINT_INPUT.notify();
                }
            }
            return Ok(());
        }
        // After PREPARE→SEND / READY, start or resume the poll pump.
        // dump223: also when sync_active — OPEN mainnet park used to require
        // !sync_active and never rearmed.
        if from_pump && self._ui.get_page() == 16 {
            let s = self._ui.get_wallet_status();
            let parked_open = (s.as_str().contains("OPEN mainnet")
                || s.as_str().contains("H3519450"))
                && !s.as_str().contains("CONNECTING")
                && !s.as_str().contains("QUEUED")
                && !s.as_str().contains("RECV")
                && !s.as_str().contains("WAIT NET")
                && !s.as_str().contains("TIP ")
                && !s.as_str().contains("SCAN");
            if parked_open
                || (!self.wallet_sync_active
                    && (s.as_str().contains("SEND QUEUED")
                        || s.as_str().contains("PREPARING DECOYS")
                        || s.as_str().contains("VALIDATING PREVIEW")
                        || s.as_str().contains("SYNC QUEUED")
                        || s.as_str().contains("READY TO SYNC")
                        || s.as_str().contains("CONNECT FAIL")
                        || s.as_str().contains("WAIT NET")
                        || s.as_str().contains("NEED SCAN")
                        || s.as_str().contains("SCAN FAIL")))
            {
                if parked_open
                    || s.as_str().contains("READY TO SYNC")
                    || s.as_str().contains("CONNECT FAIL")
                    || s.as_str().contains("WAIT NET")
                    || s.as_str().contains("NEED SCAN")
                    || s.as_str().contains("SCAN FAIL")
                {
                    let queued = self.wallet_service.refresh();
                    self._ui.set_wallet_status(
                        alloc::format!("{} · {queued}", self.wallet_boot_opened).into(),
                    );
                    self.publish_frame();
                }
                self.begin_sync_poll();
                return Ok(());
            }
        }
        // Manual / post-boot sync: wallet↔shell notify only (poll already pokes wallet).
        if from_pump && self.wallet_sync_active {
            let _ = self.poll_wallet_sync_once();
            return Ok(());
        }
        #[cfg(not(feature = "simplex-agent-ui"))]
        let _ = channels;
        Ok(())
    }

    fn handle_input(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Infallible> {
        if channel != SLINT_INPUT || request.label() != REQUEST_SLINT_EVENT || request.count() != 3
        {
            return Ok(MessageInfo::new(0x532, 0));
        }
        let posted = POSTED_ACTIVE.swap(false, Ordering::Acquire);
        let device = if posted { POSTED_DEVICE.load(Ordering::Relaxed) } else { get_mr(0) as u64 };
        let kind_and_code = if posted { 0 } else { get_mr(1) as u64 };
        let event_type = if posted { POSTED_KIND.load(Ordering::Relaxed) } else { kind_and_code >> 32 };
        let code = if posted { POSTED_CODE.load(Ordering::Relaxed) } else { kind_and_code as u32 as u64 };
        let value = if posted { POSTED_VALUE.load(Ordering::Relaxed) } else { get_mr(2) as u64 };
        #[cfg(feature = "desktop-real-app")]
        return self.desktop_input(device, event_type, code, value);
        #[cfg(feature = "utm-perf-trace")]
        if device == DEVICE_TABLET && event_type == EVENT_KEY && code == BTN_LEFT {
            debug_println!("PERF|POINTER|{}|{}", perf_time_us(), value);
        }
        #[cfg(feature = "utm-device-lock")]
        {
            if (device == DEVICE_TABLET && event_type == EVENT_KEY && code == BTN_LEFT && value != 0)
                || (device == DEVICE_KEYBOARD && event_type == EVENT_KEY && value != 0) {
                self.device_lock.borrow_mut().activity(&self._ui);
            }
            if self._ui.get_device_lock_screen() {
                if device == DEVICE_TABLET && event_type == EVENT_KEY && code == BTN_LEFT {
                    let ptr = self.pointer.as_ptr();
                    let packed = POSTED_POSITION.load(Ordering::Relaxed);
                    let x = if posted { (packed & 0xffff) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).x).read_volatile() } };
                    let y = if posted { (packed >> 16) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).y).read_volatile() } };
                    let position = slint::LogicalPosition::new(x as f32 / SCALE, y as f32 / SCALE);
                    let event = if value != 0 {
                        slint::platform::WindowEvent::PointerPressed { position, button: slint::platform::PointerEventButton::Left }
                    } else {
                        slint::platform::WindowEvent::PointerReleased { position, button: slint::platform::PointerEventButton::Left }
                    };
                    self.window.window().dispatch_event(event);
                    self.publish_frame(); SLINT_INPUT.notify();
                } else if device == DEVICE_TABLET && (event_type == EVENT_SYN || event_type == EVENT_ABS) {
                    let ptr = self.pointer.as_ptr();
                    let packed = POSTED_POSITION.load(Ordering::Relaxed);
                    let x = if posted { (packed & 0xffff) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).x).read_volatile() } };
                    let y = if posted { (packed >> 16) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).y).read_volatile() } };
                    let position = slint::LogicalPosition::new(x as f32 / SCALE, y as f32 / SCALE);
                    self.window.window().dispatch_event(slint::platform::WindowEvent::PointerMoved { position });
                } else if device == DEVICE_KEYBOARD && event_type == EVENT_KEY {
                    if matches!(code, 42 | 54) { self.shift_down = value != 0; }
                    else if value != 0 && !(self._ui.get_device_lock_stage() == 1 && self._ui.get_device_lock_welcome()) {
                        if code == 28 { self._ui.invoke_device_lock_submit(); }
                        else if let Some(key) = hardware_character(code, self.shift_down) { self._ui.invoke_device_lock_key(key.into()); }
                        self.publish_frame(); SLINT_INPUT.notify();
                    }
                }
                return Ok(MessageInfo::new(REPLY_SLINT_EVENT_OK, 0));
            }
        }
        // Timer callbacks and wallet IPC may reuse the message registers.
        // Capture the input packet above before invoking either.
        slint::platform::update_timers_and_animations();
        // Keep wallet boot recoverable, but don't synchronously poll or paint
        // the active scan before handling each tap or keystroke. The normal
        // wallet notification pump owns phase 5; a nudge is enough here.
        if self.wallet_boot_phase == 5 {
            WALLET_SERVICE.notify();
        } else if self.wallet_boot_phase != 0 {
            let _ = self.advance_wallet_boot();
        }
        if device == DEVICE_TABLET && event_type == EVENT_KEY && code == BTN_LEFT {
            let ptr = self.pointer.as_ptr();
            let position = POSTED_POSITION.load(Ordering::Relaxed);
            let x = if posted { (position & 0xffff) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).x).read_volatile() } };
            let y = if posted { (position >> 16) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).y).read_volatile() } };
            let position = slint::LogicalPosition::new(x as f32 / SCALE, y as f32 / SCALE);
            if value != 0 {
                self.gesture_begin(position.x, position.y);
                if !self.nav_gesture.defer_press || self.nav_gesture.feedback_press {
                    if self.nav_gesture.feedback_press { self.feedback_frame_dirty = true; }
                    self.window.window().dispatch_event(
                        slint::platform::WindowEvent::PointerPressed {
                            position,
                            button: slint::platform::PointerEventButton::Left,
                        },
                    );
                    self.publish_frame();
                }
                return Ok(MessageInfo::new(REPLY_SLINT_EVENT_OK, 0));
            }
            // Release may arrive without intervening ABS samples; use the
            // up position so edge/home swipes still resolve. Prefer system
            // nav over a Slint click when the gesture qualifies.
            self.gesture_move(position.x, position.y);
            let deferred = self.nav_gesture.defer_press;
            let feedback_press = self.nav_gesture.feedback_press;
            if deferred && feedback_press {
                // Cancel the visual press before deciding whether this was a
                // system swipe. A real click is synthesized only below.
                self.window.window().dispatch_event(slint::platform::WindowEvent::PointerExited);
                self.window.window().dispatch_event(slint::platform::WindowEvent::PointerReleased {
                    position: slint::LogicalPosition::new(-1.0, -1.0),
                    button: slint::platform::PointerEventButton::Left,
                });
                self.window.window().request_redraw();
            }
            let navigated = if deferred {
                self.gesture_end()
            } else {
                self.nav_gesture.active = false;
                false
            };
            if navigated {
                self.publish_frame();
                return Ok(MessageInfo::new(REPLY_SLINT_EVENT_OK, 0));
            }
            if deferred {
                // Not a swipe — synthesize the click Slint never saw.
                if feedback_press {
                    self.window.window().dispatch_event(slint::platform::WindowEvent::PointerMoved { position });
                }
                self.window.window().dispatch_event(
                    slint::platform::WindowEvent::PointerPressed {
                        position,
                        button: slint::platform::PointerEventButton::Left,
                    },
                );
            }
            let edit_before = self._ui.get_edit_target();
            let page_before = self._ui.get_page();
            #[cfg(feature = "utm-transcript-worker")]
            let chat_view_before = self._ui.get_message_view();
            let setup_before = self._ui.get_wallet_setup();
            let setup_network_before = self._ui.get_wallet_network();
            let setup_actions_before = WALLET_SETUP_ACTIONS.load(Ordering::Relaxed);
            let node_actions_before = WALLET_NODE_ACTIONS.load(Ordering::Relaxed);
            let wallet_tab_before = self._ui.get_wallet_tab();
            self.window.window().dispatch_event(
                slint::platform::WindowEvent::PointerReleased {
                    position,
                    button: slint::platform::PointerEventButton::Left,
                },
            );
            // The setup panel owns these rows. Some UTM pointer sequences do
            // not activate a Slint TouchArea, so route the same visible bounds
            // once, only if the component did not already handle the click.
            if page_before == 16 && matches!(setup_before, 1 | 2 | 4 | 5)
                && self._ui.get_wallet_setup() == setup_before
                && WALLET_SETUP_ACTIONS.load(Ordering::Relaxed) == setup_actions_before
            {
                let (w, _) = logical_screen();
                if (16.0..w - 16.0).contains(&position.x) {
                    let y = position.y;
                    if (setup_before == 1 || setup_before == 4) && (132.0..170.0).contains(&y)
                        && self._ui.get_wallet_network() == setup_network_before
                    {
                        self._ui.set_wallet_network((setup_network_before + 1) % 3);
                    } else if (setup_before == 1 || setup_before == 4) && (176.0..214.0).contains(&y) {
                        self._ui.set_edit_target(15);
                    } else if setup_before == 1 && (233.0..275.0).contains(&y) {
                        self._ui.invoke_wallet_setup_action(0);
                    } else if setup_before == 1 && (283.0..325.0).contains(&y) {
                        self._ui.set_wallet_setup(4);
                    } else if setup_before == 1 && (333.0..375.0).contains(&y) {
                        self._ui.set_wallet_setup(5);
                    } else if setup_before == 5 && (136.0..248.0).contains(&y)
                        || setup_before == 4 && (224.0..336.0).contains(&y)
                    {
                        self._ui.set_edit_target(14);
                    } else if (setup_before == 4 && (390.0..432.0).contains(&y))
                        || (setup_before != 1 && setup_before != 4 && (358.0..400.0).contains(&y))
                    {
                        let action = match setup_before { 2 => 1, 4 => 3, 5 => 4, _ => unreachable!() };
                        self._ui.invoke_wallet_setup_action(action);
                    } else if (setup_before == 4 && (438.0..476.0).contains(&y))
                        || (setup_before != 1 && setup_before != 4 && (406.0..444.0).contains(&y))
                    {
                        self._ui.invoke_wallet_setup_action(6);
                    }
                }
            }
            if page_before == 16 && setup_before == 0 && wallet_tab_before == 3
                && self._ui.get_wallet_tab() == 3 && self._ui.get_wallet_setup() == 0
            {
                let (w, _) = logical_screen();
                if (16.0..w - 16.0).contains(&position.x) {
                    if (204.0..252.0).contains(&position.y) {
                        self._ui.set_edit_target(16);
                    } else if (262.0..302.0).contains(&position.y)
                        && WALLET_NODE_ACTIONS.load(Ordering::Relaxed) == node_actions_before
                    {
                        self._ui.invoke_set_wallet_node();
                    } else if (478.0..520.0).contains(&position.y)
                        && WALLET_SETUP_ACTIONS.load(Ordering::Relaxed) == setup_actions_before
                    {
                        self._ui.invoke_wallet_setup_action(5);
                    }
                }
            }
            // The profile keyboard's Enter key is a fixed rectangle. A miss
            // leaves the typed name on screen and never tells the agent.
            if page_before == 6
                && !self._ui.get_message_view()
                && edit_before == 13
                && self._ui.get_edit_target() == 13
            {
                let (w, _) = logical_screen();
                let kb_w = w - 34.0;
                let key_w = (kb_w - 20.0) / 3.0;
                let enter_x = 17.0 + 10.0 + key_w;
                let enter_y = self._ui.get_top_inset() + self._ui.get_chat_keyboard_top() + 181.0;
                if key_w > 0.0
                    && (enter_x..enter_x + key_w).contains(&position.x)
                    && (enter_y..enter_y + 44.0).contains(&position.y)
                {
                    self._ui.set_edit_target(0);
                    self._ui.invoke_save_profile();
                    debug_println!("SLINT UTM PROFILE HIT enter");
                }
            }
            // The Send fields are fixed rectangles. A tap inside one still
            // focuses it when the TouchArea does not take the click.
            if self._ui.get_page() == 16
                && self._ui.get_wallet_tab() == 2
                && self._ui.get_edit_target() == edit_before
            {
                let (w, _) = logical_screen();
                let x = position.x;
                let y = position.y;
                let in_x = x >= 16.0 && x <= w - 16.0;
                // Rows measured on the 414×897 logical Send page. The old
                // confirm band started at 398, which is the Preview Fee
                // button, so a preview tap could broadcast once the fee
                // was armed. Keep a gap between Preview, Send, and Send Max.
                if in_x && (150.0..222.0).contains(&y) {
                    self._ui.set_edit_target(11);
                } else if in_x && (258.0..306.0).contains(&y) {
                    self._ui.set_edit_target(12);
                    debug_println!("SLINT UTM SEND HIT amount");
                } else if in_x && (376.0..420.0).contains(&y) {
                    preview_wallet_fee(&self._ui, &mut self.wallet_service);
                    debug_println!("SLINT UTM SEND HIT preview");
                } else if in_x && (430.0..478.0).contains(&y) && self._ui.get_wallet_fee_ready()
                {
                    // Fallback when the Send TouchArea does not take the
                    // click. Only the cyan Send button, and only after
                    // Preview Fee armed the signed tx.
                    let sent = self.wallet_service.send_tx();
                    self._ui.set_wallet_fee_ready(false);
                    apply_wallet_face(&self._ui, &sent);
                    self._ui
                        .set_wallet_send_status(humanize_wallet_status(&sent).into());
                    debug_println!("SLINT UTM SEND HIT confirm");
                } else if in_x && (488.0..532.0).contains(&y) {
                    let bal = self._ui.get_wallet_balance().to_string();
                    if bal == "—" || bal.is_empty() {
                        self._ui.set_wallet_send_max(false);
                        self._ui.set_wallet_fee_ready(false);
                        self._ui
                            .set_wallet_send_status("No balance to send".into());
                    } else {
                        self._ui.set_wallet_send_max(true);
                        self._ui.set_wallet_fee_ready(false);
                        self._ui.set_wallet_send_amount(bal.into());
                        self._ui
                            .set_wallet_send_status("Send max · preview the fee".into());
                    }
                    debug_println!("SLINT UTM SEND HIT max");
                }
            }
            if self._ui.get_wallet_send_status().contains("Sending") {
                self.wallet_sync_polls = 0;
                self.wallet_sync_active = true;
                debug_println!("SLINT UTM SEND POLL");
            }
            // Home cards are fixed rectangles. A TouchArea miss leaves the
            // shell on the launcher, so match the same geometry the cards use.
            // Only when the launcher was already showing and its TouchArea
            // missed. A HOME tap from another app lands in this band too, and
            // must not immediately reopen Contacts.
            if page_before == 0 && self._ui.get_page() == 0 {
                let (w, h) = logical_screen();
                let x = position.x;
                let y = position.y;
                let col_w = (w - 44.0) / 2.0;
                let left = 8.0..(17.0 + col_w + 8.0);
                let right = (19.0 + col_w)..(27.0 + col_w + col_w + 8.0);
                let row = |top_frac: f32, height_frac: f32| {
                    (h * top_frac - 8.0)..(h * top_frac + h * height_frac + 36.0)
                };
                if col_w > 0.0 && row(0.14, 0.19).contains(&y) && left.contains(&x) {
                    self._ui.set_page(6);
                    self._ui.set_message_view(false);
                    self._ui.set_message_invitation_view(false);
                    self._ui.set_message_join_view(false);
                    self._ui.set_edit_target(0);
                    debug_println!("SLINT UTM HOME HIT simplex");
                } else if col_w > 0.0 && row(0.14, 0.19).contains(&y) && right.contains(&x) {
                    self._ui.set_page(2);
                    self._ui.set_contact_detail(false);
                    self._ui.set_edit_target(0);
                    debug_println!("SLINT UTM HOME HIT contacts");
                } else if self._ui.get_wallet_available() && col_w > 0.0 && row(0.34, 0.19).contains(&y) && left.contains(&x) {
                    self._ui.set_page(16);
                    self._ui.set_wallet_tab(0);
                    self._ui.set_edit_target(0);
                    debug_println!("SLINT UTM HOME HIT nexawal");
                } else if col_w > 0.0 && row(0.34, 0.19).contains(&y) && right.contains(&x) {
                    self._ui.set_page(10);
                    self._ui.set_edit_target(0);
                    debug_println!("SLINT UTM HOME HIT tools");
                } else if row(0.55, 0.11).contains(&y) && (8.0..w - 8.0).contains(&x) {
                    self._ui.set_settings_selected(-1);
                    self._ui.set_page(8);
                    self._ui.set_edit_target(0);
                    debug_println!("SLINT UTM HOME HIT settings");
                }
            }
            #[cfg(feature = "utm-transcript-worker")]
            if page_before == 6 && (self._ui.get_page() != 6 || (chat_view_before && !self._ui.get_message_view())) {
                self.messages_service.borrow().navigation_queue.borrow_mut().cancel();
            }
            // Wallet HOME sits in the title row. A miss leaves the user inside
            // NexaWal with no shell chrome.
            if self._ui.get_page() == 16
                && (8.0..80.0).contains(&position.x)
                && (48.0..84.0).contains(&position.y)
            {
                let _ = shell_navigate_home(&self._ui, self.nav_drafts());
                debug_println!("SLINT UTM HOME HIT wallet");
            }
            // Copy Address sits lower than its Slint y on the Receive page.
            // A TouchArea miss leaves the tap with no clipboard line.
            if self._ui.get_page() == 16 && self._ui.get_wallet_tab() == 1 {
                let (w, _) = logical_screen();
                let x = position.x;
                let y = position.y;
                if w > 32.0
                    && (16.0..w - 16.0).contains(&x)
                    && (588.0..640.0).contains(&y)
                    && self._ui.get_wallet_sync().as_str() != "Address copied"
                {
                    self._ui.invoke_copy_wallet_address();
                    debug_println!("SLINT UTM WALLET HIT copy");
                }
            }
            // The balance-card Send and Receive buttons are fixed rectangles.
            // A Slint TouchArea miss leaves the wallet tab selected, so match
            // the same geometry the card uses.
            if self._ui.get_page() == 16 && self._ui.get_wallet_setup() == 0 && self._ui.get_wallet_tab() == 0 {
                let (w, _) = logical_screen();
                let x = position.x;
                let y = position.y;
                let button_w = (w - 80.0) / 2.0;
                let top = 88.0 + 8.0 + 102.0;
                if button_w > 0.0 && (top..top + 36.0).contains(&y) {
                    let send_left = 34.0;
                    let receive_left = 46.0 + button_w;
                    if (send_left..send_left + button_w).contains(&x) {
                        self._ui.set_wallet_tab(2);
                        self._ui.set_edit_target(0);
                        debug_println!("SLINT UTM WALLET HIT send");
                    } else if (receive_left..receive_left + button_w).contains(&x) {
                        self._ui.set_wallet_tab(1);
                        self._ui.set_edit_target(0);
                        debug_println!("SLINT UTM WALLET HIT receive");
                    }
                }
            }
            // Bottom nav is a 64px band. Slint's click lands on the host at
            // these coordinates; if the guest dispatch missed, select the tab
            // from the same geometry so Receive/Send/Settings still open.
            // The address keyboard's Shift key sits in that band, so leave
            // the tab alone while a send field is being typed.
            let send_keyboard = self._ui.get_wallet_tab() == 2
                && (self._ui.get_edit_target() == 11 || self._ui.get_edit_target() == 12);
            if self._ui.get_page() == 16 && self._ui.get_wallet_setup() == 0 && !send_keyboard {
                let (w, h) = logical_screen();
                if position.y >= h - 64.0 && w > 0.0 {
                    let tab = ((position.x / w) * 4.0) as i32;
                    if (0..4).contains(&tab) {
                        self._ui.set_wallet_tab(tab);
                        self._ui.set_edit_target(0);
                    }
                }
                // The wallet tab panels have opaque backgrounds, so Slint
                // can repaint their damaged area without clearing the status
                // bar and unrelated pixels above the app.
            }
            debug_println!(
                "SLINT UTM TAB {} EDIT {}",
                self._ui.get_wallet_tab(),
                self._ui.get_edit_target()
            );
            self.publish_frame();
            // The display polls this frame. It runs above the wallet, so a
            // later status publish cannot tear the copy.
            SLINT_INPUT.notify();
            {
                // Page-16 taps: first open queues once. Frozen TIP·SCAN must
                // refresh even when wallet_sync_active (notify-only resume is
                // not enough if the wallet PD went Idle mid-recv).
                // A navigation tap must not synchronously restart or poll a
                // scan. Background wallet notifications drive sync on their
                // own, and PPC refreshes here can block the UI on network IO.
                if self._ui.get_page() == 16
                    && self._ui.get_wallet_tab() == wallet_tab_before
                    && (cfg!(feature = "desktop-real-app") || self._ui.get_edit_target() == 0) {
                    if !self.wallet_opened {
                        self.wallet_opened = true;
                        let opened = self.wallet_service.smoke_round_trip();
                        self._ui.set_wallet_status(opened.clone().into());
                        self.publish_frame();
                        let queued = self.wallet_service.refresh();
                        self._ui
                            .set_wallet_status(alloc::format!("{opened} · {queued}").into());
                        self.publish_frame();
                        self.begin_sync_poll();
                    } else {
                        let status = self._ui.get_wallet_status();
                        let s = status.as_str();
                        let tip_mid = s.contains("TIP ") && s.contains("SCAN");
                        let fail_or_need = s.contains("RECV FAIL")
                            || s.contains("SEND FAIL")
                            || s.contains("BAD HTTP")
                            || s.contains("BAD JSON")
                            || s.contains("READY TO SYNC")
                            || s.contains("WAIT NET")
                            || s.contains("NEED SCAN")
                            || s.contains("SCAN FAIL")
                            || s.contains("SCAN NO ")
                            || s.contains("SCAN BAD");
                        let mid_flight = s.contains("QUEUED")
                            || s.contains("CONNECT")
                            || s.contains("FETCH")
                            || s.contains("SCAN BLOCK")
                            || s.contains("SCAN TX")
                            || s.contains("WORKER")
                            || s.contains("DECOY")
                            || s.contains("SYNCING")
                            || s.contains("RECV...")
                            || tip_mid;
                        // dump182: tip CONNECT FAIL must NOT instant-refresh on
                        // tip-mid taps — that thrashed WaitConnect→FAIL→CONNECTING
                        // forever (OCR only ever saw CONNECTING). Park + cool;
                        // from_pump tip-rearm retries after abandon settles.
                        if s.contains("CONNECT FAIL") {
                            self.wallet_sync_active = false;
                            if self.wallet_tip_rearm_cool == 0 {
                                self.wallet_tip_rearm_cool = 45;
                            }
                            WALLET_SERVICE.notify();
                        } else if s.contains("RECV...") {
                            // dump208: RECV+wallet_sync_active matched no tap arm
                            // (only CONNECTING tip-mid notify). PREPARE button
                            // then blocked in prepare_send. Always tip-refresh.
                            self.wallet_sync_active = false;
                            self._ui.set_wallet_status("NEXAWAL | TIP HUNT…".into());
                            self.publish_frame();
                            let queued = self.wallet_service.refresh();
                            self._ui.set_wallet_status(
                                alloc::format!("TIP HUNT · {queued}").into(),
                            );
                            self.publish_frame();
                            self.wallet_tip_rearm_cool = 45;
                            self.begin_sync_poll();
                        } else if fail_or_need {
                            // Failures always re-queue (even if sync_active was left
                            // true — dump116 stuck on BAD HTTP with taps ignored).
                            let queued = self.wallet_service.refresh();
                            self._ui.set_wallet_status(queued.into());
                            self.publish_frame();
                            self.begin_sync_poll();
                        } else if s.contains("PREPARE") && s.contains("INSUFFICIENT") {
                            // dump149: tip-mid taps used to re-prepare + arm cool
                            // without draining it (cool only ticks in notified),
                            // so tip hunt never ran while parked INSUFFICIENT.
                            // dump150: "PREPARE INSUFFICIENT · CONNECTING" still
                            // matched this arm and abort-refreshed mid tip — skip
                            // when tip/sync is already in flight; notify only.
                            // dump151: do NOT treat wallet_sync_active alone as
                            // busy — parked INSUFFICIENT with active stuck true
                            // blocked cool drain + tip refresh forever.
                            // dump195: bare `TIP ` matched UI chrome / stale
                            // labels and left tip_busy notify-only, so parked
                            // INSUFFICIENT never tip-refreshed (hash a1748fdd).
                            // Only treat real tip-mid (TIP+SCAN) or TCP mid-flight.
                            let tip_busy = s.contains("CONNECTING")
                                || s.contains("QUEUED")
                                || (s.contains("TIP ") && s.contains("SCAN"))
                                || s.contains(" RX")
                                || s.contains(" NET")
                                || s.contains(" TCP")
                                || s.contains("FETCH");
                            if tip_busy {
                                WALLET_SERVICE.notify();
                                self.begin_sync_poll();
                            } else {
                                // dump152/rearm-proof: cool-drain+prepare left the
                                // UI parked on INSUFFICIENT with no tip frames —
                                // deposit never findable. Always tip-refresh here;
                                // funded prepare is driven from SCAN 0 / FUNDED
                                // await taps after outs land.
                                self.wallet_sync_active = false;
                                self._ui.set_wallet_status("NEXAWAL | TIP HUNT…".into());
                                self.publish_frame();
                                let queued = self.wallet_service.refresh();
                                self._ui.set_wallet_status(
                                    alloc::format!("TIP HUNT · {queued}").into(),
                                );
                                self.publish_frame();
                                self.wallet_tip_rearm_cool = 60;
                                self.begin_sync_poll();
                            }
                        } else if self.wallet_sync_active && (tip_mid || s.contains("CONNECTING"))
                        {
                            // dump176: CONNECTING tip-mid must not abort-refresh
                            // mid TCP (escape cool==0 thrashed dump171/175).
                            // dump179: notify-only left stuck-connect rearm
                            // unarmed — tip taps never called poll_wallet_sync_once,
                            // so wallet_sync_polls stayed 0 while UI froze on
                            // SYNC CONNECTING (wallet Idle + stale transfer).
                            WALLET_SERVICE.notify();
                            if s.contains("CONNECTING") {
                                SLINT_INPUT.notify();
                                let _ = self.poll_wallet_sync_once();
                            }
                        } else if !self.wallet_sync_active && mid_flight {
                            let connecting = s.contains("CONNECTING");
                            let tip_rx = tip_mid
                                && (s.contains(" RX")
                                    || s.contains(" FETCH")
                                    || s.contains(" NET"));
                            // dump126: tap refresh on tip RX / CONNECTING aborted
                            // get_block before hang-skip could publish SCAN 0.
                            // dump182: begin_sync_poll() zeroes wallet_sync_polls
                            // — CONNECTING taps must resume without reset so
                            // stuck-connect rearm can fire.
                            if tip_rx || connecting {
                                self.wallet_sync_active = true;
                                WALLET_SERVICE.notify();
                                SLINT_INPUT.notify();
                                let _ = self.poll_wallet_sync_once();
                                if self.wallet_tip_rearm_cool == 0 {
                                    self.wallet_tip_rearm_cool = 120;
                                }
                            } else if tip_mid || s.contains("QUEUED") || s.contains("READY") {
                                let queued = self.wallet_service.refresh();
                                self._ui.set_wallet_status(queued.into());
                                self.publish_frame();
                                self.begin_sync_poll();
                            } else {
                                self.begin_sync_poll();
                            }
                        }
                    }
                }
                #[cfg(feature = "simplex-agent-ui")]
                if self._ui.get_page() == 6
                    && !self._ui.get_message_view()
                    && self._ui.get_message_invitation_view()
                    && !self._ui.get_message_address_view()
                {
                    match self.messages_service.borrow().invitation() {
                        Ok(uri) if !uri.is_empty() => {
                            log_simplex_link(uri.as_str());
                            debug_println!("SLINT SIMPLEX INVITATION READY LEN {}", uri.len());
                            present_invitation_qr(&self._ui, uri.as_str());
                            self._ui.set_message_invitation_uri(uri.into());
                            self._ui.set_message_invitation_status(
                                "INVITATION READY · SCAN QR OR COPY LINK".into(),
                            );
                        }
                        Ok(_) => {
                            self._ui.set_message_invitation_qr_visible(false);
                            self._ui.set_message_invitation_status(
                                "STILL CREATING · TAP CHECK AGAIN".into(),
                            );
                        }
                        Err(error) => {
                            debug_println!("SLINT SIMPLEX INVITATION REFRESH FAIL {}", error)
                        }
                    }
                    self.publish_frame();
                }
            }
        } else if device == DEVICE_TABLET && (event_type == EVENT_SYN || event_type == EVENT_ABS) {
            let ptr = self.pointer.as_ptr();
            let position = POSTED_POSITION.load(Ordering::Relaxed);
            let x = if posted { (position & 0xffff) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).x).read_volatile() } };
            let y = if posted { (position >> 16) as u32 } else { unsafe { core::ptr::addr_of!((*ptr).y).read_volatile() } };
            #[cfg(feature = "utm-perf-trace")]
            if posted { debug_println!("PERF|MOVE|{}|{}|{}", perf_time_us(), x, y); }
            let position = slint::LogicalPosition::new(x as f32 / SCALE, y as f32 / SCALE);
            self.gesture_move(position.x, position.y);
            if self.nav_gesture.active && self.nav_gesture.defer_press
                && self._ui.get_page() == 6 && self._ui.get_message_view()
            {
                let (width, height) = logical_screen();
                if personal_os_slint_notes_spike::chat_drag::starts_chat_scroll(
                    self.nav_gesture.start_x, self.nav_gesture.start_y, position.x, position.y,
                    width, height, self._ui.get_top_inset(), self._ui.get_keyboard_travel(),
                ) {
                    self.nav_gesture.defer_press = false;
                    let start = slint::LogicalPosition::new(self.nav_gesture.start_x, self.nav_gesture.start_y);
                    self.window.window().dispatch_event(slint::platform::WindowEvent::PointerPressed {
                        position: start, button: slint::platform::PointerEventButton::Left,
                    });
                }
            }
            if !(self.nav_gesture.active && self.nav_gesture.defer_press) {
                self.window
                    .window()
                    .dispatch_event(slint::platform::WindowEvent::PointerMoved { position });
                self.publish_frame();
            }
        } else if device == DEVICE_KEYBOARD && event_type == EVENT_KEY {
            #[cfg(feature = "utm-perf-trace")]
            if value != 0 { debug_println!("PERF|KEY|{}", perf_time_us()); }
            #[cfg(feature = "utm-perf-trace")]
            if code == 66 && value == 1 && self._ui.get_page() == 6 {
                self._ui.set_edit_target(if self._ui.get_edit_target() == 6 { 0 } else { 6 });
                self.publish_frame();
                if !self.defer_frame { SLINT_INPUT.notify(); }
                return Ok(MessageInfo::new(REPLY_SLINT_EVENT_OK, 0));
            }
            match code {
                42 | 54 => self.shift_down = value != 0,
                125 | 126 => self.meta_down = value != 0,
                29 | 97 => self.ctrl_down = value != 0,
                KEY_ESC | KEY_BACK if value != 0 => {
                    if self.apply_nav_back() {
                        self.publish_frame();
                    }
                }
                KEY_HOME if value != 0 => {
                    if self.apply_nav_home() {
                        self.publish_frame();
                    }
                }
                _ if value != 0 => {
                    if self.meta_down || self.ctrl_down {
                        if code == 46 && value == 1 {
                            copy_active_field(&self._ui);
                            self.publish_frame();
                        }
                    } else if apply_hardware_key(&self._ui, code, self.shift_down) {
                        self.publish_frame();
                        if !self.defer_frame {
                            // Publish before requesting scanout: otherwise the
                            // GPU can consume the key wakeup on the old frame.
                            SLINT_INPUT.notify();
                        }
                    }
                }
                _ => {}
            }
        }
        // Inbox poll is a blocking messages PPC. Running it on every abs/press
        // starved HOME (search keyboard + mouse move → freeze before release).
        // Only poll after a click/key while Messages is still the open page.
        #[cfg(feature = "simplex-agent-ui")]
        if self._ui.get_page() == 6 && (cfg!(feature = "desktop-real-app") || self._ui.get_edit_target() == 0)
            && ((device == DEVICE_TABLET
                && event_type == EVENT_KEY
                && code == BTN_LEFT
                && value == 0)
                || (device == DEVICE_KEYBOARD && event_type == EVENT_KEY && value != 0))
        {
            if self.chat_refresh.request(self.defer_frame) {
                self.poll_live_chat();
            }
        }
        Ok(MessageInfo::new(REPLY_SLINT_EVENT_OK, 0))
    }
}

impl Handler for SlintService {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        slint::platform::update_timers_and_animations();
        #[cfg(feature = "utm-device-lock")]
        {
            let was_locked = self._ui.get_device_lock_screen();
            self.device_lock.borrow_mut().tick(&self._ui);
            if !was_locked && self._ui.get_device_lock_screen() {
                self.scanout_hold = 0;
                self.publish_frame();
                SLINT_INPUT.notify();
            }
        }
        let timer_fired = self.frame_timer.is_some_and(|channel| channels.contains(channel));
        if timer_fired {
            #[cfg(feature = "utm-clock-selftest")]
            debug_println!("SLINT CLOCK ALARM");
            self.frame_deadline = None;
            self.refresh_wall_clock();
        }
        self.handle_notification(channels)?;
        #[cfg(feature="desktop-simplex-frontend")]
        if timer_fired { self.chat_refresh.request(true); }
        #[cfg(feature = "utm-wrapped-vault-experiment")]
        if device_lock_utm::take_vault_unlocked() {
            self.load_vault_apps();
            self.publish_frame();
            SLINT_INPUT.notify();
        }
        #[cfg(feature = "simplex-agent-ui")]
        if timer_fired && self.chat_refresh.take()
            && self._ui.get_page() == 6 && (cfg!(feature = "desktop-real-app") || self._ui.get_edit_target() == 0)
        {
            self.poll_live_chat();
        }
        #[cfg(feature = "utm-photo-worker")]
        let photo_read_finished = channels.contains(PHOTO_READER);
        #[cfg(not(feature = "utm-photo-worker"))]
        let photo_read_finished = false;
        #[cfg(feature = "utm-photo-worker")]
        if photo_read_finished { self.finish_photo_read(); }
        #[cfg(feature = "utm-photo-worker")]
        let photo_finished = channels.contains(PHOTO_WORKER);
        #[cfg(not(feature = "utm-photo-worker"))]
        let photo_finished = false;
        #[cfg(feature = "utm-photo-worker")]
        if photo_finished { self.finish_photo_preview(); }
        #[cfg(feature = "utm-transcript-worker")]
        let transcript_finished = channels.contains(TRANSCRIPT_WORKER);
        #[cfg(not(feature = "utm-transcript-worker"))]
        let transcript_finished = false;
        #[cfg(feature = "utm-transcript-worker")]
        let live_receive_finished = channels.contains(LIVE_RECEIVE_WORKER);
        #[cfg(not(feature = "utm-transcript-worker"))]
        let live_receive_finished = false;
        #[cfg(feature = "utm-transcript-worker")]
        if live_receive_finished {
            self.poll_live_chat();
            self.messages_service.borrow().start_live_receive();
        }
        #[cfg(feature = "utm-transcript-worker")]
        let navigation_finished = channels.contains(NAVIGATION_WORKER);
        #[cfg(not(feature = "utm-transcript-worker"))]
        let navigation_finished = false;
        #[cfg(feature = "utm-transcript-worker")]
        if navigation_finished {
            use personal_os_slint_notes_spike::navigation_selection::Completion as NavigationCompletion;
            use personal_os_slint_notes_spike::photo_mailbox::Completion as MailboxCompletion;
            let service = self.messages_service.borrow();
            let result = service.navigation_mailbox.take(|data| data == [1]);
            if let Some(ticket) = service.navigation_ticket.take() {
                let success = matches!(result, MailboxCompletion::Ready(true));
                let completion = service.navigation_queue.borrow_mut().finish(ticket, success);
                if let NavigationCompletion::Open { owner, .. } = completion {
                    let index = service.transcript_owners.borrow().iter().position(|&candidate| candidate == owner);
                    if let Some(index) = index.filter(|&i| self._ui.get_page() == 6 && self._ui.get_message_view()
                        && self._ui.get_message_chat_index() == i as i32) {
                        self._ui.set_message_receive_status("Loading conversation…".into());
                        self._ui.set_message_target_ready(true);
                        debug_println!("SLINT SIMPLEX SELECT CHAT {}", index);
                        service.navigation_legacy_photo_pending.set(true);
                        service.request_live_receive(true);
                    }
                } else if completion == NavigationCompletion::Failed && self._ui.get_page() == 6 {
                    self._ui.set_message_receive_status("Couldn’t open conversation. Try again.".into());
                }
            }
            service.start_navigation();
        }
        #[cfg(feature = "utm-transcript-worker")]
        let snapshot_finished = channels.contains(SNAPSHOT_WORKER);
        #[cfg(not(feature = "utm-transcript-worker"))]
        let snapshot_finished = false;
        #[cfg(feature = "utm-transcript-worker")]
        if snapshot_finished && self._ui.get_page() == 6 { self.poll_live_chat(); }
        #[cfg(feature = "utm-transcript-worker")]
        if transcript_finished {
            let completed = self.messages_service.borrow().finish_transcript();
            match completed {
            Some(TranscriptCompletion::Loaded(index, saved)) => {
                if !self.history_baselined[index] {
                    let reading = self._ui.get_page() == 6 && self._ui.get_message_view()
                        && self._ui.get_message_chat_index().max(0) as usize == index;
                    self.initial_unread[index] = if reading { 0 } else {
                        self.initial_unread[index].max(personal_os_slint_notes_spike::chat_unread::pending_before_baseline(
                            &self.chat_transcripts.borrow()[index], false,
                        ))
                    };
                    self.chat_read_len[index] = saved.len();
                    self.history_baselined[index] = true;
                }
                self.chat_transcripts.borrow_mut()[index] = saved.clone();
                if self._ui.get_page() == 6 && self._ui.get_message_view()
                    && self._ui.get_message_chat_index().max(0) as usize == index
                {
                    self._ui.set_message_transcript(saved.into());
                    if matches!(self._ui.get_message_receive_status().as_str(), "Loading conversation…" | "Couldn’t load conversation. Tap to retry.") {
                        self._ui.set_message_receive_status("".into());
                    }
                }
            }
            Some(TranscriptCompletion::Failed(index)) => {
                if self._ui.get_page() == 6 && self._ui.get_message_view()
                    && self._ui.get_message_chat_index().max(0) as usize == index
                {
                    self._ui.set_message_receive_status("Couldn’t load conversation. Tap to retry.".into());
                }
            }
            None => {}
            }
        }
        if timer_fired || photo_finished || photo_read_finished || transcript_finished || live_receive_finished || navigation_finished || snapshot_finished {
            #[cfg(feature = "simplex-agent-ui")]
            if timer_fired { self.load_visible_chat_photo_chunk(); }
            let revision = unsafe { self.header.as_ref() }.revision.load(Ordering::Acquire);
            self.publish_frame();
            if unsafe { self.header.as_ref() }.revision.load(Ordering::Acquire) != revision {
                SLINT_INPUT.notify();
            }
        }
        #[cfg(feature="desktop-simplex-frontend")]
        {
            #[cfg(not(feature = "desktop-real-app"))]
            frontend_probe::publish(&self._ui);
        }
        self.arm_frame_timer();
        Ok(())
    }

    fn protected(&mut self, channel: Channel, request: MessageInfo) -> Result<MessageInfo, Self::Error> {
        let reply = self.handle_input(channel, request)?;
        #[cfg(feature = "utm-wrapped-vault-experiment")]
        if device_lock_utm::take_vault_unlocked() {
            self.load_vault_apps();
            self.publish_frame();
            SLINT_INPUT.notify();
        }
        self.arm_frame_timer();
        Ok(reply)
    }
}
