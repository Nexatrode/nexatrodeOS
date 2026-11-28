//! Two original ruleset replies: reuse checked cursor history, never select I/O.
use super::*;
pub(super) const INDICES:[usize;2]=[1,6];
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct RulesetReplyFifo {
 pub ordinal:u32,pub record:u64,pub intent:u32,pub armed:Option<u64>,
 pub pending_tx_bytes:u32,pub unread_at_arm:u32,pub unread_at_close:Option<u32>,pub fifo:RegistryReplyFifo,
}
pub(super) struct RulesetReplyWatch {
 pub(super) window:RegistryReplyWindow,record:u64,intent:u32,armed:Option<u64>,unread:u32,close_unread:Option<u32>,
}
/* RULESET_CONTROL_BEGIN */
#[cfg(feature="sunfish-ruleset-control-window")]
impl RulesetReplyWatch {pub(super) fn binding(&self)->(u64,u32){(self.record,self.intent)}}
/* RULESET_CONTROL_END */
impl<I:Io> Adapter<I>{
 pub fn ruleset_reply_fifo(&self,index:usize)->Option<RulesetReplyFifo>{
  let w=self.ring.ruleset_reply.get(index)?.as_ref()?;
  Some(RulesetReplyFifo{ordinal:if index==0{2}else{3},record:w.record,intent:w.intent,armed:w.armed,
   pending_tx_bytes:w.window.remaining as u32,unread_at_arm:w.unread,unread_at_close:w.close_unread,fifo:w.window.trace})
 }
 pub(super) fn arm_ruleset_reply_fifo(&mut self,waiting:[bool;2],published:Transmit){
  let Transmit::Session(Published::Data{remote_intent,last_fragment:true})=published else{return;};
  let Some(length)=self.ring.rx_bytes else{return;};
  for (n,index) in INDICES.iter().enumerate(){
   if !waiting[n]||self.ring.ruleset_reply[n].is_some(){continue;}
   let Some(r)=self.session.startup_prerequisites().reads[*index].filter(|r|r.kind==1&&r.flags&48==16&&r.opcode==5&&r.number==1&&r.reply_bytes==19&&r.remote_intent==remote_intent)else{continue;};
   let Some(record)=r.record_sequence()else{continue;};
   let unread=ring_distance(self.ring.words[2],self.ring.words[3],length);let at=self.ring.first_server_check_clock.map(|c|c());
   self.ring.ruleset_reply[n]=Some(RulesetReplyWatch{window:RegistryReplyWindow::new(distance(self.ring.words[0],self.ring.words[1]),unread,at),record,intent:remote_intent,armed:at,unread:unread as u32,close_unread:None});
  }
 }
 pub(super) fn observe_ruleset_reply_rx(&mut self,before:[Option<(Option<u64>,u32)>;2],size:usize){
  let Some(length)=self.ring.rx_bytes else{return;};
  let unread=ring_distance(self.ring.words[2],self.ring.words[3],length);
  for(n,index)in INDICES.iter().enumerate(){
   let Some(w)=self.ring.ruleset_reply[n].as_mut().filter(|w|!w.window.trace.finished)else{continue;};
   let Some(r)=self.session.startup_prerequisites().reads[*index].filter(|r|r.record_sequence()==Some(w.record))else{continue;};
   let Some((Some(_),flags))=before[n].filter(|(key,_)|*key==Some(w.record))else{continue;};
   if flags&128==0&&r.flags&128!=0{w.window.rx.close_head=Some(w.window.rx.head);w.close_unread=Some(unread as u32);}
   // Core sets the done bit only for the original reservation/slot/route/length
   // and intent. Also require this normal decoder pass's exact raw RX_DONE.
   if flags&32==0&&r.flags&32!=0&&r.remote_intent==w.intent&&size==8&&matches!(self.ring.record[0],6|14)&&word(&self.ring.record,4)==w.intent{w.window.finish(unread,size);}
  }
 }
}
const _:()={assert!(::core::mem::size_of::<RulesetReplyWatch>()<=4096);};
#[cfg(all(test,mpss_glink_transport_tests))]
mod tests{
 use super::*;
 fn cached(record:u64)->RulesetReplyWatch{RulesetReplyWatch{window:RegistryReplyWindow::new(0,0,None),record,intent:7,armed:None,unread:0,close_unread:None}}
 #[test]
 fn ruleset_fifo_actual_reset_clears_both_cached_records_only_after_fresh_generation(){
  let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(None);a.ring.ruleset_reply=[Some(cached(41)),Some(cached(42))];assert!(a.reset_after_transport_reset(1).is_err());assert!(a.ruleset_reply_fifo(0).is_some());
  {let mut m=p.0.borrow_mut();m.observation.generation=2;m.words[1]=m.words[0];m.words[2]=m.words[3];}
  a.reset_after_transport_reset(2).unwrap();assert!(a.ruleset_reply_fifo(0).is_none()&&a.ruleset_reply_fifo(1).is_none());
 }
}
