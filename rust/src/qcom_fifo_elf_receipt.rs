//! Exact first published ELF reply: transport consumption and RX_DONE visibility.
//! Observations only; no selector, buffer-retirement or credit decisions.
use super::*;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ElfReplyFifo {
    pub remote_intent:u32,
    pub armed_at:Option<u64>,
    pub unread_at_arm:u32,
    pub core_done_at:Option<u64>,
    pub fifo:RegistryReplyFifo,
}
pub(super) struct ElfReplyWatch {
    pub(super) window:RegistryReplyWindow,
    request:RequestHandle,
    remote_intent:u32,
    armed_at:Option<u64>,
    unread_at_arm:u32,
    core_done_at:Option<u64>,
}
impl<I:Io> Adapter<I> {
    pub fn elf_reply_fifo(&self)->Option<ElfReplyFifo> {
        self.ring.elf_reply.as_ref().map(|w|ElfReplyFifo {
            remote_intent:w.remote_intent,armed_at:w.armed_at,
            unread_at_arm:w.unread_at_arm,core_done_at:w.core_done_at,fifo:w.window.trace,
        })
    }
    pub(super) fn arm_elf_reply_fifo(&mut self,published:Transmit) {
        if self.ring.elf_reply.is_some(){return;}
        let Transmit::Session(Published::Data{remote_intent,last_fragment:true})=published else{return;};
        let Some((request,_))=self.session.elf_reply_fifo_target() else{return;};
        let Some(length)=self.ring.rx_bytes else{return;};
        // Reuse the successful checked writer's validated cursor pair.
        let unread=ring_distance(self.ring.words[2],self.ring.words[3],length);
        let at=self.ring.first_server_check_clock.map(|clock|clock());
        self.ring.elf_reply=Some(ElfReplyWatch {
            window:RegistryReplyWindow::new(distance(self.ring.words[0],self.ring.words[1]),unread,at),
            request,remote_intent,armed_at:at,unread_at_arm:unread as u32,core_done_at:None,
        });
    }
    pub(super) fn observe_elf_reply_receipt(&mut self,event:&Receive,size:usize) {
        let Some(w)=self.ring.elf_reply.as_mut().filter(|w|!w.window.trace.finished)else{return;};
        // Accept only the normal decoder's generation/session-bound receipt.
        // A remote-intent number alone can be reused and is not an identity.
        if !matches!(event,Receive::Session(Event::ReplyRxDone(h))if *h==w.request){return;}
        let Some(length)=self.ring.rx_bytes else{return;};
        w.core_done_at=self.session.elf_window_trace()
            .and_then(|t|t.receipts.iter().flatten().find(|r|r.target)).and_then(|r|r.at);
        w.window.finish(ring_distance(self.ring.words[2],self.ring.words[3],length),size);
    }
}
const _:()={assert!(::core::mem::size_of::<ElfReplyWatch>()<=4096);};
