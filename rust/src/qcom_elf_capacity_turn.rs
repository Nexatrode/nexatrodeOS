//! One service opportunity after a real full-pool queue rejection. No writer bypass.
use super::*;
#[derive(Clone,Copy,Debug,Default)]
pub struct ElfCapacityTurnTrace{
 pub armed:Option<u64>,pub entered:Option<u64>,pub ended:Option<u64>,pub flags:u32,
 pub free_slot:Option<u8>,pub result:u8,pub arms:u32,pub full_skips:u32,
}
impl<I:Io> Owner<I>{
 pub fn elf_capacity_turn_trace(&self)->ElfCapacityTurnTrace{self.elf_capacity_trace}
 pub(super) fn arm_elf_capacity_turn(&mut self,h:RequestHandle){
  if self.elf_capacity_taken || self.elf_lookup_handle!=Some(h) || self.adapter.normal_tx_free().is_some(){return;}
  if self.elf_capacity_waiting.is_none(){self.elf_capacity_waiting=Some(h);self.elf_capacity_trace.arms=self.elf_capacity_trace.arms.saturating_add(1);self.elf_capacity_trace.armed=self.rmtfs_event_clock.map(|c|c());self.elf_capacity_trace.flags|=1;}
 }
 pub fn dispatch_elf_capacity_retry(&mut self,interrupts:&mut impl Interrupts)->Result<Option<ServiceProgress>,Error>{
  self.check()?;
  let Some(h)=self.elf_capacity_waiting.filter(|_|!self.elf_capacity_taken)else{return Ok(None);};
  let Some(slot)=self.adapter.normal_tx_free()else{self.elf_capacity_trace.full_skips=self.elf_capacity_trace.full_skips.saturating_add(1);return Ok(None);};
  if self.elf_lookup_handle!=Some(h) || !self.requests.contains(&Some(h)){self.elf_capacity_waiting=None;self.elf_capacity_trace.flags|=64;return Ok(None);}
  // Preserve the normal pending-request order and all request/queue checks.
  if self.pending_request_for_service(Service::ServiceRegistry)?!=Some(h){return Ok(None);}
  self.elf_capacity_taken=true;self.elf_capacity_waiting=None;
  self.elf_capacity_trace.entered=self.rmtfs_event_clock.map(|c|c());self.elf_capacity_trace.free_slot=Some(slot as u8);self.elf_capacity_trace.flags|=2;
  let result=self.dispatch_service_registry_step(interrupts);
  self.elf_capacity_trace.ended=self.rmtfs_event_clock.map(|c|c());self.elf_capacity_trace.flags|=4;
  self.elf_capacity_trace.result=match &result{Ok(ServiceProgress::ReplyQueued)=>{self.elf_capacity_trace.flags|=8;1},Ok(ServiceProgress::WaitingReply)=>{self.elf_capacity_trace.flags|=16;2},Ok(ServiceProgress::Idle)=>3,Ok(ServiceProgress::Progress)=>4,Err(_)=>{self.elf_capacity_trace.flags|=32;5}};
  result.map(Some)
 }
}
