//! Observation only: one blocked ELF receipt, Owner return and next registry entry.
use super::*;
#[derive(Clone,Copy,Debug,Default,Eq,PartialEq)]
pub struct ElfRetryTrace {
    request:Option<RequestHandle>,slot_completed:Option<RequestHandle>,
    pub flags:u32,pub blocker_transaction:Option<u16>,
    pub receipt_at:Option<u64>,pub work_returned_at:Option<u64>,
    pub registry_entry_at:Option<u64>,pub registry_selected_at:Option<u64>,
    pub slot_at:Option<u64>,pub slot_transaction:Option<u16>,pub slot_sequence:Option<u32>,
    pub receipt_counts:[u32;2],pub return_counts:[u32;2],
}
const _:()={assert!(core::mem::size_of::<ElfRetryTrace>()<=256);};
impl<I:Io> Owner<I> {
    pub fn elf_retry_trace(&self)->ElfRetryTrace{self.elf_retry}
    fn elf_retry_clock(&mut self)->Option<u64>{
        let clock=self.rmtfs_event_clock?;
        let stopped=self.stopped;self.stopped=true;
        let at=clock();
        self.stopped=stopped;Some(at)
    }
    fn elf_retry_bind(&mut self)->bool{
        let Some(h)=self.elf_lookup_handle.filter(|h|self.requests.contains(&Some(*h)))else{return false;};
        if self.elf_retry.request.is_some_and(|old|old!=h){return false;}
        self.elf_retry.request=Some(h);self.elf_retry.flags|=1;true
    }
    pub(super) fn observe_elf_retry_receipt(&mut self,completed:RequestHandle,progress:Progress){
        let Some(b)=self.service_registry_trace.elf.and_then(|e|e.blocker).filter(|b|b.handle==completed)else{return;};
        if self.elf_retry.flags&2!=0 || !self.elf_retry_bind(){return;}
        self.elf_retry.flags|=2|(u32::from(self.elf_slot_turn_taken)<<6);
        self.elf_retry.blocker_transaction=Some(b.transaction);
        // Reuse the existing exact Owner completion clock; no second receipt sample.
        self.elf_retry.receipt_at=b.done;
        self.elf_retry.receipt_counts=[progress.received,progress.published];
    }
    pub(super) fn observe_elf_retry_return(&mut self,progress:Progress){
        if self.elf_retry.flags&2==0 || self.elf_retry.flags&4!=0 || !self.elf_retry_bind(){return;}
        self.elf_retry.work_returned_at=self.elf_retry_clock();
        self.elf_retry.return_counts=[progress.received,progress.published];
        self.elf_retry.flags|=4|(u32::from(progress.deferred)<<8);
    }
    pub(super) fn observe_elf_retry_entry(&mut self){
        if self.elf_retry.flags&2==0 || self.elf_retry.flags&8!=0 || !self.elf_retry_bind(){return;}
        self.elf_retry.registry_entry_at=self.elf_retry_clock();
        self.elf_retry.flags|=8|(u32::from(self.elf_retry.flags&4==0)<<9);
    }
    pub(super) fn observe_elf_retry_selection(&mut self,selected:RequestHandle){
        if self.elf_retry.flags&8==0 || self.elf_retry.flags&16!=0 || !self.elf_retry_bind(){return;}
        self.elf_retry.registry_selected_at=self.elf_retry_clock();
        self.elf_retry.flags|=16|(u32::from(self.elf_retry.request==Some(selected))<<7);
    }
    pub(super) fn observe_elf_retry_slot(&mut self,completed:RequestHandle){
        if self.elf_retry.flags&32!=0 || !self.elf_retry_bind(){return;}
        let r=self.adapter.elf_window_trace().and_then(|w|w.receipts.iter().flatten().max_by_key(|r|r.sequence)).copied();
        self.elf_retry.slot_completed=Some(completed);
        self.elf_retry.slot_at=self.elf_retry_clock();
        self.elf_retry.slot_transaction=r.map(|r|r.transaction);
        self.elf_retry.slot_sequence=r.map(|r|r.sequence);
        self.elf_retry.flags|=32;
    }
}
