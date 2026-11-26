//! First actual idle loan only; no packet, queue or device policy.
use super::*;
#[derive(Clone,Copy)]
pub struct IdleAttempt{pub start:Option<u64>,pub end:Option<u64>,pub header:Option<RegistrationHeader>,pub result:u32,pub unread:u32,pub advertisement:Option<[u32;2]>}
#[derive(Clone,Copy)]
pub struct IdleRx{pub at:Option<u64>,pub tag:u32,pub unread:u32,pub early:u32,pub header:Option<RegistrationHeader>,pub advertisement:Option<[u32;2]>}
pub struct McfgIdleDetail{
 pub captured:bool,pub meta:[u32;4],peer:[u32;2],pub call:u32,pub start:Option<u64>,pub end:Option<u64>,pub summary:[u32;3],
 pub attempts:[Option<IdleAttempt>;4],pub rx:[Option<IdleRx>;8],pub rx_total:u32,pub rx_omitted:u32,
 pub(super) writer_active:bool,rx_active:bool,next:usize,current:Option<usize>,
}
impl Default for McfgIdleDetail{fn default()->Self{Self{captured:false,meta:[u32::MAX;4],peer:[u32::MAX;2],call:0,start:None,end:None,summary:[0;3],attempts:[None;4],rx:[None;8],rx_total:0,rx_omitted:0,writer_active:false,rx_active:false,next:0,current:None}}}
impl McfgIdleDetail{
 pub(super) fn header(&mut self,h:RegistrationHeader,raw:&[u8]){if self.writer_active&&let Some(i)=self.current{let e=self.attempts[i].as_mut().unwrap();e.advertisement=(h.command==5&&raw.len()>=16).then(||[word(raw,12),word(raw,8)]);e.header=Some(h);}}
 fn receive(&mut self,event:IdleRx){
  let i=if self.rx_total<8{self.rx_total as usize}else{self.rx.copy_within(5..8,4);self.rx_omitted=self.rx_omitted.saturating_add(1);7};
  let closed=event.header.and_then(|h|h.del_client)==Some(self.peer);
  self.rx[i]=Some(event);self.rx_total=self.rx_total.saturating_add(1);if event.tag==5||closed{self.rx_active=false;}
 }
}
impl<I:Io> Adapter<I>{
 pub(crate) fn mcfg_idle_detail(&self)->&McfgIdleDetail{&self.idle_detail}
 pub(crate) fn arm_idle_detail(&mut self,r:core::TftpReplyRoute,bytes:usize,slot:usize){
  if self.idle_detail.captured{return;}let at=self.ring.first_server_check_clock.map(|c|c());
  self.idle_detail=McfgIdleDetail{captured:true,meta:[r.destination.port,r.source.port,bytes as u32,slot as u32],peer:[r.destination.node,r.destination.port],call:self.mcfg_idle.calls.saturating_add(1),start:at,writer_active:true,..Default::default()};
 }
 pub(super) fn begin_idle_detail_attempt(&mut self){
  if !self.idle_detail.writer_active{return;}let at=self.ring.first_server_check_clock.map(|c|c());let d=&mut self.idle_detail;let i=d.next;if i>=4{return;}d.next+=1;d.current=Some(i);d.attempts[i]=Some(IdleAttempt{start:at,end:None,header:None,result:0,unread:u32::MAX,advertisement:None});
 }
 pub(super) fn end_idle_detail_attempt(&mut self,r:&Result<Transmit,Error>){
  if !self.idle_detail.writer_active{return;}let at=self.ring.first_server_check_clock.map(|c|c());let (unread,early)=self.tftp_rx_snapshot().unwrap_or((u32::MAX,u32::MAX));let d=&mut self.idle_detail;
  if let Some(i)=d.current.take(){let e=d.attempts[i].as_mut().unwrap();e.end=at;e.unread=unread;e.result=match r{Ok(Transmit::Session(_))=>1,Ok(Transmit::Blocked{..})=>2,Err(_)=>3,Ok(Transmit::Idle)if unread!=0||early!=0=>4,Ok(Transmit::Idle)=>5,Ok(Transmit::AwaitingIntent)=>6};}
 }
 pub(super) fn finish_idle_detail(&mut self,writes:u32,veto:u32,blocked:u32){
  if !self.idle_detail.writer_active{return;}let at=self.ring.first_server_check_clock.map(|c|c());let d=&mut self.idle_detail;d.end=at;d.summary=[writes,veto,blocked];d.writer_active=false;d.rx_active=true;
 }
 pub(super) fn observe_idle_detail_rx(&mut self,tag:u32,header:Option<RegistrationHeader>){
  if !self.idle_detail.rx_active||tag<3{return;}let at=self.ring.first_server_check_clock.map(|c|c());let(unread,early)=self.tftp_rx_snapshot().unwrap_or((u32::MAX,u32::MAX));let advertisement=(tag==3&&header.is_some_and(|h|h.command==5&&h.bytes>=16)).then(||[word(&self.ring.record,12),word(&self.ring.record,8)]);self.idle_detail.receive(IdleRx{at,tag,header,unread,early,advertisement});
 }
}
#[cfg(all(test,mpss_glink_transport_tests))]
mod tests{
 use super::*;
 #[test]
 fn mcfg_idle_detail_bounded_opening_tail_and_close_scope(){
  let mut d=McfgIdleDetail{rx_active:true,peer:[0,0x4a],..Default::default()};for n in 0..12{d.receive(IdleRx{at:None,tag:3,unread:n,early:0,header:None,advertisement:None});}assert_eq!((d.rx_total,d.rx_omitted),(12,4));assert_eq!(d.rx.iter().flatten().map(|e|e.unread).collect::<Vec<_>>(),[0,1,2,3,8,9,10,11]);
  let mut h=RegistrationHeader::read(&crate::data(1,&crate::owner_control_frame(6,[0,0x4b,0,0])),false,None);d.receive(IdleRx{at:None,tag:3,unread:72,early:0,header:Some(h),advertisement:None});assert!(d.rx_active);h.del_client=Some([0,0x4a]);d.receive(IdleRx{at:None,tag:3,unread:72,early:0,header:Some(h),advertisement:None});assert!(!d.rx_active);assert!(d.rx.iter().flatten().all(|e|e.at.is_none()));
 }
}
