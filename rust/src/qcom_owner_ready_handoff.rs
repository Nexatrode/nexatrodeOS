//! One exact-blocker service handoff; diagnostic fields never authorize work.
use super::*;
#[derive(Clone,Copy,Debug,Default,Eq,PartialEq)]
pub struct ReadyHandoffTrace{pub flags:u32,pub arms:u32,pub receipts:u32,pub services:u32,pub result:u32}
#[derive(Default)]
pub(super) struct ReadyHandoff{
    dependency:Option<(RequestHandle,RequestHandle)>,ready:Option<RequestHandle>,taken:bool,
    trace:ReadyHandoffTrace,
}
impl<I:Io> Owner<I>{
    pub fn ready_handoff_trace(&self)->ReadyHandoffTrace{self.elf_ready_handoff.trace}
    pub(super) fn bind_ready_blocker(&mut self,h:RequestHandle,result:&Result<(),Error>){
        if self.elf_lookup_handle!=Some(h) || self.elf_ready_handoff.taken{return;}
        self.elf_ready_handoff.dependency=None;
        if matches!(result,Err(Error::Transport(fifo::Error::Session(crate::qcom_glink_qmi::Error::FlowBusy))))
            && let Some(blocker)=self.adapter.registry_unpublished_blocker(&h)
        {
            self.elf_ready_handoff.dependency=Some((h,blocker));
            self.elf_ready_handoff.trace.flags|=1;
            self.elf_ready_handoff.trace.arms=self.elf_ready_handoff.trace.arms.saturating_add(1);
        }
    }
    pub(super) fn take_ready_blocker_receipt(&mut self,completed:RequestHandle)->bool{
        if !self.elf_slot_turn_taken || self.elf_ready_handoff.taken{return false;}
        let Some((waiting,blocker))=self.elf_ready_handoff.dependency else{return false;};
        if blocker!=completed{return false;}
        self.elf_ready_handoff.dependency=None;
        if self.elf_lookup_handle!=Some(waiting) || !self.requests.contains(&Some(waiting))
            || !self.adapter.take_elf_slot_turn(&completed,&waiting){return false;}
        self.elf_ready_handoff.taken=true;self.elf_ready_handoff.ready=Some(waiting);
        self.elf_ready_handoff.trace.flags|=2;
        self.elf_ready_handoff.trace.receipts=self.elf_ready_handoff.trace.receipts.saturating_add(1);
        true
    }
    pub fn dispatch_ready_blocker(&mut self,interrupts:&mut impl Interrupts)->Result<Option<ServiceProgress>,Error>{
        self.check()?;
        if self.pending_mcfg_close.is_some() || self.pending_firmware_close.is_some(){return Ok(None);}
        #[cfg(feature="sunfish-tftp-close-handoff-prototype")]
        if self.pending_tftp_close.is_some(){return Ok(None);}
        let Some(h)=self.elf_ready_handoff.ready.take()else{return Ok(None);};
        // Consume before normal validation, clocks or queue callbacks. A wait,
        // fault or unwind cannot reuse this handoff; ordinary service continues.
        self.elf_ready_handoff.trace.flags|=4;
        self.elf_ready_handoff.trace.services=self.elf_ready_handoff.trace.services.saturating_add(1);
        if self.elf_lookup_handle!=Some(h) || !self.requests.contains(&Some(h))
            || self.pending_request_for_service(Service::ServiceRegistry)?!=Some(h){
            self.elf_ready_handoff.trace.flags|=32;return Ok(None);
        }
        let result=self.dispatch_service_registry_step(interrupts);
        self.elf_ready_handoff.trace.flags|=8;
        self.elf_ready_handoff.trace.result=match &result{
            Ok(ServiceProgress::ReplyQueued)=>{self.elf_ready_handoff.trace.flags|=16;1},
            Ok(ServiceProgress::WaitingReply)=>2,Ok(ServiceProgress::Idle)=>3,Ok(ServiceProgress::Progress)=>4,Err(_)=>5};
        result.map(Some)
    }
}
