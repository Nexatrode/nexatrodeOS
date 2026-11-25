//! Observation only: exact first ELF admission and both earlier same-peer owners.
use super::*;
#[cfg(feature="sunfish-elf-writer-trace")]
#[path="qcom_elf_publication.rs"]
mod publication;
#[cfg(feature="sunfish-elf-writer-trace")]
pub use publication::{ElfWriterTrace,ElfWriterRecord};
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ElfWindowEntry {
    request:Option<RequestHandle>,pub transaction:u16,pub flags:u8,pub kind:u8,pub remote_intent:u32,
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ElfWindowSnapshot {
    pub at:Option<u64>,pub sequence:u32,pub reason:u8,pub count:u8,pub gates:u16,
    pub entries:[Option<ElfWindowEntry>;2],
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct ElfWindowReceipt {
    pub at:Option<u64>,pub sequence:u32,pub transaction:u16,pub remote_intent:u32,
    pub remaining:u8,pub matched_last_failure:bool,pub target:bool,
}
#[derive(Clone,Copy,Debug)]
pub struct ElfWindowTrace {
    request:RequestHandle,local:Address,peer:Address,sequence:u32,
    pub attempts:u32,pub first_failure:Option<ElfWindowSnapshot>,pub last_failure:Option<ElfWindowSnapshot>,
    pub admitted:Option<ElfWindowSnapshot>,pub receipts:[Option<ElfWindowReceipt>;4],
    pub receipt_count:u32,pub receipt_omitted:u32,pub last_before_admission:Option<u16>,
    /// Started1, failed admission2, admitted4, a last-snapshot member completed8, exact ELF completed16.
    pub flags:u32,pub finished:bool,
    #[cfg(feature="sunfish-elf-writer-trace")]
    pub writer:ElfWriterTrace,
}
const _:()={assert!(::core::mem::size_of::<ElfWindowTrace>()<=3072);};
impl Session {
    #[cfg(feature="sunfish-elf-tx-slots")]
    pub(super) fn elf_window_request_matches(&self,h:RequestHandle)->bool{self.elf_window.as_ref().is_some_and(|w|w.request==h)}
    pub fn elf_window_trace(&self)->Option<&ElfWindowTrace>{self.elf_window.as_ref()}
    pub(super) fn start_elf_window(&mut self,h:RequestHandle,index:usize){
        if self.elf_window.is_some(){return;}
        let b=&self.receive[index].bytes;
        self.elf_window=Some(ElfWindowTrace{request:h,local:Address{node:word(b,24),port:word(b,28)},peer:Address{node:word(b,8),port:word(b,12)},sequence:0,
            attempts:0,first_failure:None,last_failure:None,admitted:None,receipts:[None;4],receipt_count:0,receipt_omitted:0,last_before_admission:None,flags:1,finished:false,
            #[cfg(feature="sunfish-elf-writer-trace")]
            writer:ElfWriterTrace::default()});
    }
    fn elf_window_snapshot(&self,c:&ElfWindowTrace,reason:u8)->ElfWindowSnapshot{
        let mut s=ElfWindowSnapshot{at:None,sequence:0,reason,count:0,gates:0,entries:[None;2]};
        for tx in self.transmit.iter().filter(|tx|tx.len!=0 && tx.kind.qrtr_addresses().is_some_and(|(_,p)|p==c.peer)){
            let request=match tx.kind {TxKind::Reply{receive,..}=>Some(RequestHandle{owner:self.owner,generation:self.generation,intent:self.receive[receive].id}),_=>None};
            if request==Some(c.request){continue;}
            let(mut transaction,mut same_endpoint)=(u16::MAX,false);
            if let TxKind::Reply{route,..}=tx.kind {
                transaction=route.transaction;same_endpoint=route.source==c.local;
                if route.service!=Service::ServiceRegistry{s.gates|=2;}
            }else{s.gates|=2;}
            if !same_endpoint{s.gates|=4;}
            if tx.sent!=tx.len{s.gates|=8;}
            if tx.remote_slot.is_none(){s.gates|=16;}
            if tx.glink_done{s.gates|=32;}
            if !tx.qrtr_done{s.gates|=64;}
            if tx.reference_flow.is_none(){s.gates|=128;}
            let flags=u8::from(tx.sent!=0)|(u8::from(tx.sent==tx.len)<<1)|(u8::from(tx.glink_done)<<2)|(u8::from(tx.qrtr_done)<<3)|(u8::from(tx.remote_slot.is_some())<<4)|(u8::from(tx.reference_flow.is_some())<<5);
            if let Some(slot)=s.entries.get_mut(usize::from(s.count)){
                *slot=Some(ElfWindowEntry{request,transaction,flags,kind:tx.kind.diagnostic_kind(),remote_intent:tx.remote_slot.and_then(|i|self.remote[i]).map_or(u32::MAX,|r|r.id)});
            }
            s.count=s.count.saturating_add(1);
        }
        if s.count>1{s.gates|=1;}
        s
    }
    pub(super) fn observe_elf_window_admission(&mut self,h:RequestHandle,route:ReplyRoute,reason:u8){
        let Some(c)=self.elf_window.as_ref().filter(|c|c.request==h && !c.finished && c.admitted.is_none()
            && route.source==c.local && route.destination==c.peer && route.service==Service::ServiceRegistry)else{return;};
        let mut snapshot=self.elf_window_snapshot(c,reason);snapshot.at=self.intent_ack_clock.map(|f|f());
        let c=self.elf_window.as_mut().unwrap();c.sequence=c.sequence.saturating_add(1);snapshot.sequence=c.sequence;c.attempts=c.attempts.saturating_add(1);
        if reason==0 {c.admitted=Some(snapshot);c.flags|=4;c.last_before_admission=c.receipts.iter().flatten().max_by_key(|r|r.sequence).map(|r|r.transaction);}
        else {c.first_failure.get_or_insert(snapshot);c.last_failure=Some(snapshot);c.flags|=2;}
    }
    pub(super) fn observe_elf_window_receipt(&mut self,event:Event,id:u32,route:ReplyRoute){
        let Event::ReplyRxDone(h)=event else{return;};
        let Some(c)=self.elf_window.as_ref().filter(|c|!c.finished && route.source==c.local && route.destination==c.peer)else{return;};
        let remaining=self.elf_window_snapshot(c,0).count;
        let matched=c.last_failure.is_some_and(|s|s.entries.iter().flatten().any(|e|e.request==Some(h)));
        let at=self.intent_ack_clock.map(|f|f());let c=self.elf_window.as_mut().unwrap();c.sequence=c.sequence.saturating_add(1);
        let r=ElfWindowReceipt{at,sequence:c.sequence,transaction:route.transaction,remote_intent:id,remaining,matched_last_failure:matched,target:h==c.request};
        if c.receipt_count<4{c.receipts[c.receipt_count as usize]=Some(r);}else{c.receipts.copy_within(3..4,2);c.receipts[3]=Some(r);c.receipt_omitted=c.receipt_omitted.saturating_add(1);}
        c.receipt_count=c.receipt_count.saturating_add(1);if matched{c.flags|=8;}
        if r.target{c.finished=true;c.flags|=16;}
    }
}
