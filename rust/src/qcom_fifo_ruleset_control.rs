//! Bounded accepted RX / committed TX context. No scheduling or ownership authority.
use super::*;
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct RulesetControlEntry {
 pub at:Option<u64>,pub flags:u8,pub command:u16,pub channel:u16,pub parameter:u32,pub bytes:u16,
 pub kind:u32,pub a:u32,pub b:u32,pub tag:u8,pub c:u32,pub d:u32,
}
impl RulesetControlEntry {
 fn read(bytes:&[u8],tx:bool,cached:bool,at:Option<u64>)->Option<Self>{
  let h=RegistryWireHeader::read(bytes,tx,at)?;
  let mut e=Self{at,flags:u8::from(tx)|(u8::from(cached)<<1),command:h.command,channel:h.channel,parameter:h.parameter,bytes:h.bytes,
   kind:u32::MAX,a:u32::MAX,b:u32::MAX,tag:0xf,c:u32::MAX,d:u32::MAX};
  if let Some([kind,a,b])=h.qrtr{e.kind=kind;e.a=a;e.b=b;}
  if h.command==5&&h.parameter!=0&&bytes.len()>=16 {e.tag=5;e.c=word(bytes,8);e.d=word(bytes,12);}
  else if let Some([c,d])=h.tftp{e.tag=0xa;e.c=c;e.d=d;}
  else if let Some([c,d])=h.del_client{e.tag=0xd;e.c=c;e.d=d;}
  else if let Some((flag,txn,msg))=h.qmi{e.tag=flag;e.c=u32::from(txn);e.d=u32::from(msg);}
  Some(e)
 }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct RulesetRequestVisibility {pub record:Option<u64>,pub bounds:Option<ServerCheckRxDone>,handle:TftpHandle}
pub struct RulesetControlTrace {
 pub entries:[Option<RulesetControlEntry>;64],pub requests:[Option<RulesetRequestVisibility>;2],
 pub total:u32,pub omitted:u32,pub unparsed:u32,pub rx:u32,pub tx:u32,pub done_mask:u8,
 active:bool,
}
impl Default for RulesetControlTrace {fn default()->Self{Self{entries:[None;64],requests:[None;2],total:0,omitted:0,unparsed:0,rx:0,tx:0,done_mask:0,active:false}}}
impl RulesetControlTrace {
 fn record(&mut self,e:Option<RulesetControlEntry>){
  if !self.active||self.done_mask==3{return;}
  let Some(e)=e else{self.unparsed=self.unparsed.saturating_add(1);return;};
  self.total=self.total.saturating_add(1);
  if e.flags&1!=0{self.tx=self.tx.saturating_add(1);}else{self.rx=self.rx.saturating_add(1);}
  if let Some(slot)=self.entries.iter_mut().find(|s|s.is_none()){*slot=Some(e);}
  else{self.entries[32..].rotate_left(1);self.entries[63]=Some(e);self.omitted=self.omitted.saturating_add(1);}
 }
}
impl<I:Io> Adapter<I>{
 pub fn ruleset_control_trace(&self)->&RulesetControlTrace{&self.ring.ruleset_control}
 pub(super) fn bind_ruleset_control_records(&mut self){
  for(n,index)in ruleset_reply::INDICES.iter().enumerate(){if let Some(r)=self.session.startup_prerequisites().reads[*index]
   && let Some(q)=self.ring.ruleset_control.requests[n].as_mut().filter(|q|q.handle==r.observation_handle()){q.record=r.record_sequence();}}
 }
 pub(super) fn observe_ruleset_control_rx(&mut self,size:usize,cached:bool,before:[Option<(Option<u64>,u32)>;2]){
  let reads=self.session.startup_prerequisites();
  let t=&mut self.ring.ruleset_control;
  if t.done_mask==3{return;}
  // Include the original WRQ grant/credit work; also support an EFS-first or
  // ruleset-first stream without an original server-check request.
  if !t.active&&self.session.first_server_check().is_none()&&reads.reads[0].is_none()&&reads.reads[2].is_none(){return;}
  t.active=true;
  let bytes=if cached {&self.ring.control.as_ref().unwrap().early[0].as_ref().unwrap().bytes[..size]}else{&self.ring.record[..size]};
  let at=self.ring.first_server_check_clock.map(|c|c());
  let e=RulesetControlEntry::read(bytes,false,cached,at);
  t.record(e);
  for(n,index)in ruleset_reply::INDICES.iter().enumerate(){
   let Some(r)=reads.reads[*index] else{continue;};
   if t.requests[n].is_none()&&before[n].is_none(){
    let bounds=if cached{None}else{self.ring.rx_bytes.and_then(|length|self.ring.registration_rx.as_ref().and_then(|w|w.bounds(ring_distance(self.ring.words[2],self.ring.words[3],length),size)))};
    t.requests[n]=Some(RulesetRequestVisibility{record:r.record_sequence(),bounds,handle:r.observation_handle()});
   }
   let Some(record)=r.record_sequence()else{continue;};
   if let Some(q)=t.requests[n].as_mut().filter(|q|q.handle==r.observation_handle()){q.record=Some(record);}
   // The FIFO watch already required the original Core done-bit transition,
   // reservation/intent and actual raw receipt. Never freeze on an ID alone.
   if self.ring.ruleset_reply[n].as_ref().is_some_and(|w|w.window.trace.finished&&w.binding()==(record,r.remote_intent))
    && e.is_some_and(|e|matches!(e.command,6|14)&&e.parameter==r.remote_intent)
    && before[n].is_some_and(|(key,flags)|key==Some(record)&&flags&32==0)
    && size==8&&r.flags&32!=0 {t.done_mask|=1<<n;}
  }
 }
}
impl<I:Io> Ring<I>{
 pub(super) fn observe_ruleset_control_tx(&mut self,bytes:&[u8]){
  if self.ruleset_control.active&&self.ruleset_control.done_mask!=3{
   let e=RulesetControlEntry::read(bytes,true,false,self.first_server_check_clock.map(|c|c()));
   self.ruleset_control.record(e);
  }
 }
}
const _:()={assert!(::core::mem::size_of::<RulesetControlEntry>()<=80);assert!(::core::mem::size_of::<RulesetControlTrace>()<=8192);};
#[cfg(all(test,mpss_glink_transport_tests))]
mod tests {
 use super::*;
 #[test]fn ruleset_control_keeps_first_half_and_recent_half_then_freezes(){
  let mut t=RulesetControlTrace::default();t.active=true;
  for id in 0..70{let b=[6,0,2,0,id,0,0,0];t.record(RulesetControlEntry::read(&b,false,false,None));}
  assert_eq!((t.total,t.omitted,t.rx,t.tx),(70,6,70,0));assert_eq!(t.entries[31].unwrap().parameter,31);assert_eq!(t.entries[32].unwrap().parameter,38);assert_eq!(t.entries[63].unwrap().parameter,69);
  t.done_mask=3;t.record(None);assert_eq!((t.total,t.omitted,t.unparsed),(70,6,0));
 }
 #[test]fn ruleset_control_advertisement_exposes_first_size_id_not_fabricated_pool(){
  let mut b=vec![5,0,2,0,2,0,0,0];for x in [4096u32,17,8192,18]{b.extend(x.to_le_bytes());}
  let e=RulesetControlEntry::read(&b,true,false,Some(9)).unwrap();assert_eq!((e.command,e.parameter,e.tag,e.c,e.d,e.flags),(5,2,5,4096,17,1));assert_eq!(e.at,Some(9));assert!(RulesetControlEntry::read(&b[..7],false,false,None).is_none());
 }
 #[test]fn ruleset_control_actual_writer_keeps_return_ad_ack_and_data_order(){
  let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(None);a.ring.ruleset_control.active=true;
  let mut t=RulesetWriterTrace::default();assert_eq!(a.publish_ruleset_error_turn(1,&mut t).unwrap(),4);
  let e:Vec<_>=a.ruleset_control_trace().entries.iter().flatten().copied().collect();assert_eq!(e.iter().map(|e|e.command).collect::<Vec<_>>(),[6,5,8,9]);assert!(e.iter().all(|e|e.flags==1));assert_eq!((e[1].tag,e[1].c),(5,4096));let w=p.take_tx();assert_eq!([w[0],w[8],w[24],w[32]],[6,5,8,9]);
 }
 #[test]fn ruleset_control_actual_reset_clears_only_after_fresh_valid_empty_transport(){
  let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(None);a.ring.ruleset_control.active=true;a.ring.ruleset_control.record(RulesetControlEntry::read(&[6,0,2,0,8,0,0,0],false,false,None));
  assert!(a.reset_after_transport_reset(1).is_err());assert_eq!(a.ruleset_control_trace().total,1);
  {let mut m=p.0.borrow_mut();m.observation.generation=2;m.words[1]=m.words[0];m.words[2]=m.words[3];}
  a.reset_after_transport_reset(2).unwrap();assert_eq!((a.ruleset_control_trace().total,a.ruleset_control_trace().done_mask),(0,0));assert!(a.ruleset_control_trace().entries.iter().all(Option::is_none));assert!(a.ruleset_control_trace().requests.iter().all(Option::is_none));
 }
 #[test]fn ruleset_control_blocked_writer_records_only_real_read_notification_fallback(){
  let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(Some(16));a.ring.ruleset_control.active=true;
  let mut t=RulesetWriterTrace::default();assert_eq!(a.publish_ruleset_error_turn(1,&mut t).unwrap(),1);assert_eq!((t.writes,t.blocked),(0,1));let e:Vec<_>=a.ruleset_control_trace().entries.iter().flatten().copied().collect();assert_eq!(e.len(),1);assert_eq!((e[0].flags,e[0].command),(1,13));let w=p.take_tx();assert_eq!(&w[w.len()-8..],&[13,0,0,0,0,0,0,0]);
 }
}
