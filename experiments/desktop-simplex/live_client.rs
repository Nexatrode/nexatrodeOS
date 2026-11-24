//! Opt-in live-relay driver. Only public Messages APIs and the real receive /
//! transcript worker are used: there is no decoded-event injection seam.
#![no_std]
#![no_main]
extern crate alloc;
#[allow(dead_code, unexpected_cfgs)]
#[path = "../../rust/src/wallet_scan_mailbox.rs"] mod photo_mailbox;
#[path = "../slint-notes/src/transcript_ipc.rs"] mod transcript_ipc;
#[path = "../slint-notes/src/live_receive.rs"] mod live_receive;
use personal_os_rust_server::vault_blob_transfer::BlobTransferPage;
use photo_mailbox::{Mailbox, Completion};
use sel4_microkit::{Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, get_mr, set_mr, memory_region_symbol, protection_domain};
#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<sel4_microkit::_private::heap::RawOneShotMutex> = sel4_dlmalloc::DeferredStaticDlmalloc::new();
struct Client {
    epoch: u64, phase: u64, owner: u64,
    transcript: &'static transcript_ipc::TranscriptMailbox,
    live: &'static Mailbox<1,4096>,
    page: &'static mut BlobTransferPage,
    result: *mut u64,
}
fn call(label: u64, count: usize) -> MessageInfo { Channel::new(1).pp_call(MessageInfo::new(label,count)) }
fn timer() { set_mr(0, 100_000_000); Channel::new(0).pp_call(MessageInfo::new(1, 1)); }
impl Client {
    fn publish(&mut self, phase: u64) {
        if self.phase != phase { debug_println!("SIMPLEX_LIVE|PHASE|{}|{}",self.epoch,phase); }
        self.phase=phase;
        unsafe {
            self.result.write_volatile(0x4e584c4956453031);
            self.result.add(1).write_volatile(self.epoch);
            self.result.add(6).write_volatile(self.owner);
            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
            self.result.add(2).write_volatile(phase);
        }
    }
    fn expected(&self) -> &'static [u8] { if self.epoch==1 { b"host-before-lock" } else { b"host-while-locked" } }
    fn step(&mut self) {
        match self.phase {
            0 => {
                // LOAD refreshes the connection handles used by SELECT_CHAT.
                let r=call(0x350,0); assert_eq!((r.label(),r.count()),(0x352,1));
                if self.epoch==1 {
                    let r=call(0x353,0); assert_eq!((r.label(),r.count()),(0x354,0));
                    self.publish(1);
                } else { self.publish(2); }
            }
            1 => {
                let r=call(0x355,0); assert_eq!((r.label(),r.count()),(0x356,1));
                let n=get_mr(0) as usize;
                if n>0 {
                    let uri=self.page.payload().expect("invitation");assert_eq!(uri.len(),n);assert!(n<=3000);
                    unsafe { for (i,b) in uri.iter().enumerate() { (self.result as *mut u8).add(64+i).write_volatile(*b); }
                        self.result.add(5).write_volatile(n as u64); }
                    self.publish(2);
                }
            }
            2 => {
                let r=call(0x350,0);assert_eq!(r.label(),0x352);
                set_mr(0,0);let r=call(0x367,1);
                if r.label()==0x368 {
                    self.publish(3);
                }
            }
            3 => {
                if let Completion::Ready(event) = self.live.take(|out| {
                    let e=live_receive::decode(out).expect("receive result");
                    (e.status,e.owner,e.text.as_bytes()==self.expected())
                }) {
                    if event.0==2 {
                        self.owner=event.1;
                        assert!(event.2,"unexpected fixture message");
                        debug_println!("SIMPLEX_LIVE|RECEIVED|epoch={}",self.epoch);
                        assert!(self.transcript.submit(|out| {out.copy_from_slice(&0u64.to_le_bytes());Some(8)}));
                        Channel::new(4).notify();self.publish(4);
                        return;
                    }
                }
                if self.live.available() {
                    assert!(self.live.submit(|out| {out[0]=1;Some(1)}));Channel::new(5).notify();
                }
            }
            4 => if let Completion::Ready(rows) = self.transcript.take(|out| {
                let (owner,_,body)=transcript_ipc::parse_response(out).expect("transcript");assert_eq!(owner,self.owner);
                assert!(body.as_bytes().windows(self.expected().len()).any(|w|w==self.expected()));
                if self.epoch>1 { assert!(body.contains("host-before-lock")); }
                body.matches("host-").count()
            }) {
                unsafe {self.result.add(3).write_volatile(rows as u64);}
                set_mr(0,self.owner);let r=call(0x363,1);assert_eq!(r.label(),0x364);
                self.publish(5);
            },
            5 => {
                let r=call(0x365,0);assert_eq!(r.label(),0x366);
                if get_mr(0)==2 {self.publish(6);}
            }
            6 => {
                let body=if self.epoch==1 {b"guest-before-lock".as_slice()} else {b"guest-after-unlock".as_slice()};
                assert!(self.page.set_payload(body));set_mr(0,body.len() as u64);
                let r=call(0x35b,1);assert_eq!(r.label(),0x35c);self.publish(7);
            }
            7 => {
                let r=call(0x35d,0);assert_eq!(r.label(),0x35e);
                let status=get_mr(0);unsafe {self.result.add(4).write_volatile(status);}
                assert!(status!=4,"send failed");
                if status==2 {self.publish(8);debug_println!("SIMPLEX_LIVE|SEND_ACCEPTED|epoch={}",self.epoch);}
            }
            _ => { // Keep processing real receipts until the host locks us.
                if let Completion::Ready(_) = self.live.take(|_| ()) {}
                if self.live.available() { self.live.submit(|out| {out[0]=1;Some(1)});Channel::new(5).notify(); }
            }
        }
    }
}
#[protection_domain]
fn init() -> Client {
    let state=memory_region_symbol!(session_state: *mut u64);
    let epoch=unsafe {state.as_ptr().read_volatile()};assert!(epoch>0 && unsafe{state.as_ptr().add(1).read_volatile()}==1);
    let heap=memory_region_symbol!(session_client_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(heap.as_ptr(),0x200000)).unwrap();
    let transcript=memory_region_symbol!(transcript_mailbox: *mut transcript_ipc::TranscriptMailbox);
    let live=memory_region_symbol!(live_receive_mailbox: *mut Mailbox<1,4096>);
    let page=memory_region_symbol!(messages_ui_transfer: *mut BlobTransferPage);
    let result=memory_region_symbol!(session_result: *mut u64);
    let mut c=Client {epoch,phase:0,owner:0,transcript:unsafe{transcript.as_ref()},live:unsafe{live.as_ref()},page:unsafe{&mut *page.as_ptr()},result:result.as_ptr()};
    assert!(c.transcript.available() && c.live.available());c.publish(0);timer();c
}
impl Handler for Client {
    type Error=Infallible;
    fn notified(&mut self,_:ChannelSet)->Result<(),Self::Error>{self.step();timer();Ok(())}
}
