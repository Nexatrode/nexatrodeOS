//! Bounded first-two ruleset/modem_efs reply observations, independent of ELF/MCFG.
use super::*;
#[derive(Clone,Copy,Debug)]
pub struct StartupRead {
 handle:TftpHandle,record:Option<crate::qcom_qrtr_flow::Reservation>,slot:Option<usize>,
 pub kind:u32,pub peer:Address,pub flags:u32,pub request_bytes:u32,
 pub options_present:u32,pub options_flags:u32,pub rsize:u32,
/* STARTUP_COVERAGE_BEGIN */
 #[cfg(feature="sunfish-startup-reply-coverage")]
 pub timeout_ms:u32,
/* STARTUP_COVERAGE_END */
 pub result:u32,pub opcode:u32,pub number:u32,pub reply_bytes:u32,pub local_port:u32,pub remote_intent:u32,
 pub attempts:u32,pub waits:[u32;2],pub client:u32,
 pub received:Option<u64>,pub dispatched:Option<u64>,pub queued:Option<u64>,pub published:Option<u64>,pub done:Option<u64>,pub close:Option<u64>,
}
/* ELF_DIGEST_BEGIN */
#[cfg(not(feature="sunfish-elf-digest-outcome"))]
/* ELF_DIGEST_END */
#[derive(Default)]
pub struct StartupPrerequisites {pub reads:[Option<StartupRead>;4],pub total:[u32;2],pub omitted:[u32;2]}
/* ELF_DIGEST_BEGIN */
#[cfg(feature="sunfish-elf-digest-outcome")]
/* STARTUP_COVERAGE_BEGIN */
#[cfg(not(feature="sunfish-startup-reply-coverage"))]
/* STARTUP_COVERAGE_END */
#[derive(Default)]
pub struct StartupPrerequisites {pub reads:[Option<StartupRead>;6],pub total:[u32;2],pub omitted:[u32;2],pub digest_total:u32,pub digest_omitted:u32}
/* ELF_DIGEST_END */
/* STARTUP_COVERAGE_BEGIN */
#[cfg(feature="sunfish-startup-reply-coverage")]
#[derive(Default)]
pub struct StartupPrerequisites {pub reads:[Option<StartupRead>;8],pub total:[u32;2],pub omitted:[u32;2],pub digest_total:u32,pub digest_omitted:u32}
/* STARTUP_COVERAGE_END */
fn matches_tx(r:&StartupRead,t:&Transmit,local:u32)->bool{
 r.record.is_some()&&r.record==t.reference_flow&&r.slot.is_some()
 &&matches!(t.kind,TxKind::TftpReply{source,destination}if source==Address{node:local,port:r.local_port}&&destination==r.peer)
 &&t.len>=32&&word(&t.bytes,4)==1&&word(&t.bytes,20)==r.reply_bytes
}
impl StartupRead {pub fn record_sequence(&self)->Option<u64>{self.record.map(|r|r.sequence())}}
/* RULESET_CONTROL_BEGIN */
#[cfg(feature="sunfish-ruleset-control-window")]
impl StartupRead {pub(crate) fn observation_handle(&self)->TftpHandle{self.handle}}
/* RULESET_CONTROL_END */

impl Session {
 pub fn startup_prerequisites(&self)->&StartupPrerequisites{&self.startup_prerequisites}
 pub(super) fn observe_startup_read(&mut self,index:usize,handle:TftpHandle){
  let rx=&self.receive[index];let end=32usize.checked_add(word(&rx.bytes,20)as usize);let Some(body)=end.filter(|n|*n<=rx.used).and_then(|n|rx.bytes.get(32..n))else{return;};
  let peer=Address{node:word(&rx.bytes,8),port:word(&rx.bytes,12)};let local=word(&rx.bytes,28);
  if body.len()<2{return;}let op=u16::from_be_bytes([body[0],body[1]]);
  if op!=1||local!=crate::qcom_tftp_server_check::SERVICE_PORT{
   if matches!(op,4|5)&&body.len()>=4{for r in self.startup_prerequisites.reads.iter_mut().flatten().filter(|r|r.peer==peer&&r.local_port==local&&r.flags&128==0){r.flags|=64;r.client=(u32::from(op)<<16)|u32::from(u16::from_be_bytes([body[2],body[3]]));}}return;
  }
  let path=body[2..].split(|b|*b==0).next();
  let kind=match path{Some(b"/readwrite/ota_firewall/ruleset")=>1,Some(b"/readwrite/modem_efs")=>2,
/* ELF_DIGEST_BEGIN */
  #[cfg(feature="sunfish-elf-digest-outcome")]
  Some(b"/readonly/vendor/mbn/mcfg_hw/mbn_hw.dig")=>3,
/* ELF_DIGEST_END */
  _=>return};
  if self.startup_prerequisites.reads.iter().flatten().any(|r|r.handle==handle){return;}
  let k=kind as usize-1;let at=self.intent_ack_clock.map(|c|c());let opts=McfgOptionIds::read(body,true);let c=&mut self.startup_prerequisites;
/* ELF_DIGEST_BEGIN */
  #[cfg(feature="sunfish-elf-digest-outcome")]
  if kind==3{c.digest_total=c.digest_total.saturating_add(1);}else{c.total[k]=c.total[k].saturating_add(1);}
  #[cfg(not(feature="sunfish-elf-digest-outcome"))]
/* ELF_DIGEST_END */
  {c.total[k]=c.total[k].saturating_add(1);}
  let i=k*2+usize::from(c.reads[k*2].is_some());
/* STARTUP_COVERAGE_BEGIN */
  #[cfg(feature="sunfish-startup-reply-coverage")]
  let i=if kind==1&&c.reads[i].is_some(){6+usize::from(c.reads[6].is_some())}else{i};
/* STARTUP_COVERAGE_END */
  if c.reads[i].is_some(){
/* ELF_DIGEST_BEGIN */
  #[cfg(feature="sunfish-elf-digest-outcome")]
  if kind==3{c.digest_omitted=c.digest_omitted.saturating_add(1);}else{c.omitted[k]=c.omitted[k].saturating_add(1);}
  #[cfg(not(feature="sunfish-elf-digest-outcome"))]
/* ELF_DIGEST_END */
  {c.omitted[k]=c.omitted[k].saturating_add(1);}return;}
  c.reads[i]=Some(StartupRead{handle,record:None,slot:None,kind,peer,flags:1,request_bytes:body.len()as u32,options_present:opts.present,options_flags:opts.flags,rsize:opts.values[4],
/* STARTUP_COVERAGE_BEGIN */
    #[cfg(feature="sunfish-startup-reply-coverage")] timeout_ms:opts.values[1],
/* STARTUP_COVERAGE_END */
    result:u32::MAX,opcode:u32::MAX,number:u32::MAX,reply_bytes:0,local_port:u32::MAX,remote_intent:u32::MAX,attempts:0,waits:[0;2],client:u32::MAX,received:at,dispatched:None,queued:None,published:None,done:None,close:None});
 }
 pub(crate) fn observe_startup_dispatch(&mut self,h:&TftpHandle,result:Result<crate::qcom_tftp_server_check::Reply,crate::qcom_tftp_server_check::Error>,bytes:&[u8]){
  let Some(r)=self.startup_prerequisites.reads.iter_mut().flatten().find(|r|r.handle==*h&&r.flags&2==0)else{return;};r.flags|=2;r.dispatched=self.intent_ack_clock.map(|c|c());r.result=result.err().map_or(0,|e|e.code());
  if let Ok(reply)=result&&let Some(b)=bytes.get(..reply.len).filter(|b|b.len()>=2){r.flags|=4;r.reply_bytes=b.len()as u32;r.local_port=reply.route.source.port;r.opcode=u32::from(u16::from_be_bytes([b[0],b[1]]));if matches!(r.opcode,3|4|5)&&b.len()>=4{r.number=u32::from(u16::from_be_bytes([b[2],b[3]]));}}
 }
 pub(super) fn observe_startup_queue(&mut self,h:&TftpHandle,route:TftpReplyRoute,result:Result<usize,Error>){
  let Some(r)=self.startup_prerequisites.reads.iter_mut().flatten().find(|r|r.handle==*h&&r.flags&12==4&&r.peer==route.destination&&r.local_port==route.source.port)else{return;};r.attempts=r.attempts.saturating_add(1);
  match result{Ok(i)=>{r.flags|=8;r.queued=self.intent_ack_clock.map(|c|c());r.record=self.transmit[i].reference_flow;r.slot=Some(i);if r.record.is_none(){r.flags|=512;}},Err(Error::FlowBusy)=>r.waits[0]=r.waits[0].saturating_add(1),Err(Error::NoCapacity)=>r.waits[1]=r.waits[1].saturating_add(1),_=>{}}
 }
 pub(super) fn observe_startup_publication(&mut self,index:usize,remote:usize){
  let tx=&self.transmit[index];for r in self.startup_prerequisites.reads.iter_mut().flatten().filter(|r|r.slot==Some(index)&&r.flags&16==0&&matches_tx(r,tx,self.config.local_node)){r.flags|=16;r.published=self.intent_ack_clock.map(|c|c());r.remote_intent=self.remote[remote].as_ref().map_or(u32::MAX,|i|i.id);}
 }
 pub(super) fn observe_startup_done(&mut self,index:usize,id:u32){
  let tx=&self.transmit[index];for r in self.startup_prerequisites.reads.iter_mut().flatten().filter(|r|r.slot==Some(index)&&r.flags&48==16&&r.remote_intent==id&&matches_tx(r,tx,self.config.local_node)){r.flags|=32;r.done=self.intent_ack_clock.map(|c|c());r.slot=None;}
 }
 pub(super) fn observe_startup_close(&mut self,port:u32){
  for r in self.startup_prerequisites.reads.iter_mut().flatten().filter(|r|r.peer==Address{node:self.config.peer_node,port}&&r.flags&128==0){r.flags|=128;r.close=self.intent_ack_clock.map(|c|c());}
 }
}
/* STARTUP_COVERAGE_BEGIN */
#[cfg(not(feature="sunfish-startup-reply-coverage"))]
/* STARTUP_COVERAGE_END */
const _:()={assert!(::core::mem::size_of::<StartupPrerequisites>()<=1536);};
/* STARTUP_COVERAGE_BEGIN */
#[cfg(feature="sunfish-startup-reply-coverage")]
const _:()={assert!(::core::mem::size_of::<StartupPrerequisites>()<=2304);};
/* STARTUP_COVERAGE_END */
#[cfg(test)]
#[path="../../tests/sunfish_startup_prerequisites_cases.rs"]
mod tests;
