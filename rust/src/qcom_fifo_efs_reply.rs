//! First modem_efs ERROR transport consumption and completion visibility only.
use super::*;
/* EFS_REPLY_OVERLAP_BEGIN */
pub(super) fn watched_index(session:&core::Session)->usize {
 #[cfg(feature="sunfish-efs-reply-overlap")]
 if session.efs_overlap_trace().flags&1!=0{return 3;}
 let _=session;2
}
/* EFS_REPLY_OVERLAP_END */
#[derive(Clone,Copy,Debug)]
pub struct EfsReplyFifo{pub record:u64,pub intent:u32,pub armed:Option<u64>,pub unread_at_arm:u32,pub unread_at_close:Option<u32>,pub fifo:RegistryReplyFifo}
pub(super) struct EfsReplyWatch{pub(super) window:RegistryReplyWindow,record:u64,intent:u32,armed:Option<u64>,unread:u32,close_unread:Option<u32>}
impl<I:Io> Adapter<I>{
 pub fn efs_reply_fifo(&self)->Option<EfsReplyFifo>{self.ring.efs_reply.as_ref().map(|w|EfsReplyFifo{record:w.record,intent:w.intent,armed:w.armed,unread_at_arm:w.unread,unread_at_close:w.close_unread,fifo:w.window.trace})}
 pub(super) fn arm_efs_reply_fifo(&mut self){
  if self.ring.efs_reply.is_some(){return;}let Some(r)=self.session.startup_prerequisites().reads[efs_reply::watched_index(&self.session)].filter(|r|r.kind==2&&r.flags&48==16&&r.opcode==5&&r.number==1&&r.reply_bytes==19)else{return;};
  let(Some(record),Some(length))=(r.record_sequence(),self.ring.rx_bytes)else{return;};let unread=ring_distance(self.ring.words[2],self.ring.words[3],length);let at=self.ring.first_server_check_clock.map(|c|c());
  self.ring.efs_reply=Some(EfsReplyWatch{window:RegistryReplyWindow::new(distance(self.ring.words[0],self.ring.words[1]),unread,at),record,intent:r.remote_intent,armed:at,unread:unread as u32,close_unread:None});
 }
 pub(super) fn observe_efs_reply_rx(&mut self,before:Option<(Option<u64>,u32)>,size:usize){
  let Some(w)=self.ring.efs_reply.as_mut().filter(|w|!w.window.trace.finished)else{return;};let Some(r)=self.session.startup_prerequisites().reads[efs_reply::watched_index(&self.session)].filter(|r|r.record_sequence()==Some(w.record))else{return;};let Some((Some(key),flags))=before.filter(|(key,_)|*key==Some(w.record))else{return;};let _=key;
  let Some(length)=self.ring.rx_bytes else{return;};let unread=ring_distance(self.ring.words[2],self.ring.words[3],length);
  if flags&128==0&&r.flags&128!=0{w.window.rx.close_head=Some(w.window.rx.head);w.close_unread=Some(unread as u32);}
  if flags&32==0&&r.flags&32!=0&&r.remote_intent==w.intent&&size==8&&matches!(self.ring.record[0],6|14)&&word(&self.ring.record,4)==w.intent{w.window.finish(unread,size);}
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn efs_reply_fifo_peer_consumes_before_late_visible_receipt(){let mut w=RegistryReplyWindow::new(72,0,Some(1));w.sample(72,0,0,Some(2),Some(3));assert_eq!(w.trace.peer_tail_at,Some(3));w.sample(0,72,72,Some(4),Some(5));w.rx.close_head=Some(w.rx.head);w.sample(0,8,8,Some(6),Some(7));w.finish(8,8);let b=w.trace.completion.unwrap();assert_eq!((b.last_absent,b.first_visible,b.bytes_ahead,b.visible_at_close),(Some(4),Some(7),Some(0),Some(false)));assert!(w.trace.finished);}
 #[test]
 fn efs_reply_fifo_clockless_eviction_leaves_bounds_unknown(){let mut w=RegistryReplyWindow::new(0,0,None);for _ in 0..40{w.sample(0,8,8,None,None);}assert_eq!(w.trace.history_evictions,8);w.finish(320,8);let b=w.trace.completion.unwrap();assert_eq!((b.last_absent,b.first_visible,b.bytes_ahead),(None,None,None));assert!(w.trace.finished);}
}
