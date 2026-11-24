//! Offline integration driver. Uses the real receive/transcript/photo workers;
//! only the decoded incoming relay event and JPEG are generated fixture data.
#![no_std]
#![no_main]
#[allow(dead_code, unexpected_cfgs)]
#[path = "../../rust/src/wallet_scan_mailbox.rs"]
mod photo_mailbox;
#[path = "../slint-notes/src/photo_ipc.rs"] mod photo_ipc;
#[path = "../slint-notes/src/photo_preview.rs"] mod photo_preview;
#[path = "../slint-notes/src/photo_reader_ipc.rs"] mod photo_reader_ipc;
#[path = "../slint-notes/src/photo_read.rs"] mod photo_read;
#[path = "../slint-notes/src/transcript_ipc.rs"] mod transcript_ipc;
#[path = "../slint-notes/src/live_receive.rs"] mod live_receive;
extern crate alloc;
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use photo_mailbox::{Mailbox, Completion};
use sel4_microkit::{Channel, ChannelSet, Handler, Infallible, MessageInfo,
    debug_println, get_mr, set_mr, memory_region_symbol, protection_domain};
const BODY: &[u8] = b"disposable receive after unlock";
const JPEG: &[u8] = include_bytes!("fixture.jpg");
const FIRST_PHOTO: u64 = (1u64 << 32) | 1; // First allocation generation and slot on a fresh disk.
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<sel4_microkit::_private::heap::RawOneShotMutex> = sel4_dlmalloc::DeferredStaticDlmalloc::new();
struct Client {
    phase: u8,
    epoch: u64,
    history_rows: usize,
    photo: &'static photo_ipc::PhotoMailbox,
    reader: &'static photo_reader_ipc::PhotoReadMailbox,
    transcript: &'static transcript_ipc::TranscriptMailbox,
    live: &'static Mailbox<1,4096>,
    page: &'static mut BlobTransferPage,
    result: *mut u64,
}
impl Client {
    fn progress(&self, phase: u64) {
        unsafe {
            self.result.write_volatile(0x4e58534d54455354);
            self.result.add(1).write_volatile(self.epoch);
            self.result.add(3).write_volatile(self.history_rows as u64);
            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
            self.result.add(2).write_volatile(phase);
        }
    }
}
fn timer() { set_mr(0, 50_000_000); Channel::new(0).pp_call(MessageInfo::new(1, 1)); }
#[protection_domain]
fn init() -> Client {
    let state = memory_region_symbol!(session_state: *mut u64);
    let epoch = unsafe { state.as_ptr().read_volatile() };
    assert!(epoch > 0 && unsafe { state.as_ptr().add(1).read_volatile() } == 1);
    let heap = memory_region_symbol!(session_client_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(heap.as_ptr(), 0x200000)).unwrap();
    let photo = memory_region_symbol!(photo_mailbox_region: *mut photo_ipc::PhotoMailbox);
    let reader = memory_region_symbol!(photo_read_mailbox: *mut photo_reader_ipc::PhotoReadMailbox);
    let transcript = memory_region_symbol!(transcript_mailbox: *mut transcript_ipc::TranscriptMailbox);
    let live = memory_region_symbol!(live_receive_mailbox: *mut Mailbox<1,4096>);
    let page = memory_region_symbol!(messages_ui_transfer: *mut BlobTransferPage);
    let result = memory_region_symbol!(session_result: *mut u64);
    let client = Client { phase:0, epoch, history_rows:0, photo:unsafe {photo.as_ref()}, reader:unsafe {reader.as_ref()},
        transcript:unsafe {transcript.as_ref()}, live:unsafe {live.as_ref()}, page:unsafe { &mut *page.as_ptr() }, result:result.as_ptr() };
    assert!(client.photo.available() && client.reader.available() && client.transcript.available() && client.live.available());
    debug_println!("SIMPLEX_TEST|FRESH_QUEUES|epoch={}", epoch);
    client.progress(1);
    timer();
    client
}
impl Handler for Client {
    type Error = Infallible;
    fn notified(&mut self, _: ChannelSet) -> Result<(), Self::Error> {
        match self.phase {
            0 => {
                assert!(self.page.set_payload(BODY)); set_mr(0, self.epoch);
                let reply = Channel::new(1).pp_call(MessageInfo::new(0x7c0, 1));
                if reply.label() == 1 { timer(); return Ok(()); }
                assert_eq!(reply.label(), 0);
                assert!(self.live.submit(|out| {out[0]=0;Some(1)}));
                self.phase=1; Channel::new(5).notify();
            }
            1 => if let Completion::Ready(valid) = self.live.take(|out| {
                live_receive::decode(out).is_some_and(|event| event.status==2 && event.owner==4242 && event.text.as_bytes()==BODY)
            }) {
                assert!(valid);
                debug_println!("SIMPLEX_TEST|RECEIVED_AND_PERSISTED|epoch={}", self.epoch);
                self.progress(2);
                assert!(self.transcript.submit(|out| {out.copy_from_slice(&0u64.to_le_bytes());Some(8)}));
                self.phase=2; Channel::new(4).notify();
            },
            2 => if let Completion::Ready(rows) = self.transcript.take(|out| {
                let (owner,_,body) = transcript_ipc::parse_response(out).expect("transcript format");
                assert_eq!(owner,4242);
                body.as_bytes().windows(BODY.len()).filter(|w| *w==BODY).count()
            }) {
                assert!(rows >= self.epoch.min(3) as usize);
                self.history_rows=rows;
                debug_println!("SIMPLEX_TEST|TRANSCRIPT_RECOVERED|epoch={}", self.epoch);
                self.progress(3);
                // Create the photo once, then recover that same encrypted
                // object on every later unlock and across the guest reboot.
                let id = if rows==1 {
                    assert!(self.page.set_payload(JPEG));
                    let reply = Channel::new(1).pp_call(MessageInfo::new(0x7c1, 0));
                    assert_eq!((reply.label(),reply.count()), (0x300,1));
                    let id=get_mr(0);assert_eq!(id,FIRST_PHOTO);id
                } else {FIRST_PHOTO};
                assert!(self.reader.submit(|out| {out.copy_from_slice(&photo_reader_ipc::request(4242,id));Some(16)}));
                self.phase=3; Channel::new(3).notify();
            },
            3 => if let Completion::Ready(queued) = self.reader.take(|out| {
                assert_eq!(out,JPEG);
                self.photo.submit(|dst| {dst[..out.len()].copy_from_slice(out);Some(out.len())})
            }) {
                assert!(queued); self.phase=4; Channel::new(2).notify();
            },
            4 => if let Completion::Ready(valid) = self.photo.take(|out| {
                photo_ipc::preview_parts(out).is_some_and(|(w,h,p)| w==64 && h==64 && p.len()==64*64*4)
            }) {
                assert!(valid);
                debug_println!("SIMPLEX_TEST|PHOTO_DECODED|epoch={}", self.epoch);
                // Deliberately leave work queued at lock. Stopping the whole
                // group and resetting these pages must discard it, not replay
                // it when a new session starts. Include a recognizable marker.
                assert!(self.photo.submit(|out| {out[..JPEG.len()].copy_from_slice(JPEG);Some(JPEG.len())}));
                assert!(self.transcript.submit(|out| {out.copy_from_slice(&0u64.to_le_bytes());Some(8)}));
                assert!(self.live.submit(|out| {out[0]=0;Some(1)}));
                self.page.set_payload(b"disposable pending plaintext canary");
                self.phase=5;
                self.progress(5);
                debug_println!("SIMPLEX_TEST|PENDING_AT_LOCK|epoch={}", self.epoch);
            },
            _ => {}
        }
        Ok(())
    }
}
