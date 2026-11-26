//! Observation of existing validated receive work after a strict writer veto.
use super::*;
#[derive(Clone,Copy)]
pub struct McfgRxEvent{
 pub at:Option<u64>,pub tag:u32,pub call:u32,pub unread:u32,pub early:u32,
 pub watched_sequence:Option<u64>,pub header:Option<RegistrationHeader>,
}
pub struct McfgRxVetoTrace{active:bool,pub total:u32,pub omitted:u32,pub records:[Option<McfgRxEvent>;12]}
impl Default for McfgRxVetoTrace{fn default()->Self{Self{active:false,total:0,omitted:0,records:[None;12]}}}
impl McfgRxVetoTrace{
 fn push(&mut self,event:McfgRxEvent){let index=if self.total<12{self.total as usize}else{self.records.copy_within(5..12,4);self.omitted=self.omitted.saturating_add(1);11};self.records[index]=Some(event);self.total=self.total.saturating_add(1);}
}
impl<I:Io> Adapter<I>{
 pub(crate) fn mcfg_rx_veto_trace(&self)->&McfgRxVetoTrace{&self.mcfg_rx_veto}
 pub(super) fn record_mcfg_rx_event(&mut self,tag:u32,header:Option<RegistrationHeader>){
/* MCFG_IDLE_DETAIL_BEGIN */
#[cfg(feature="sunfish-mcfg-idle-detail")]
  self.observe_idle_detail_rx(tag,header);
/* MCFG_IDLE_DETAIL_END */
  let m=self.session.post_elf_mcfg().and_then(|c|c.mcfg.as_ref());
  if m.is_some_and(|m|m.flags&0x130!=0)||(tag>2&&!self.mcfg_rx_veto.active){return;}
  if tag<=2{self.mcfg_rx_veto.active=true;}
  let watched_sequence=m.and_then(|m|m.record_sequence());let(unread,early)=self.tftp_rx_snapshot().unwrap_or((u32::MAX,u32::MAX));
  let at=self.ring.first_server_check_clock.map(|c|c());
  self.mcfg_rx_veto.push(McfgRxEvent{at,tag,call:self.mcfg_queued.calls,unread,early,watched_sequence,header});
  if tag==5{self.mcfg_rx_veto.active=false;}
 }
}
#[cfg(all(test,mpss_glink_transport_tests))]
mod tests{
 use super::*;
 #[test]
 fn mcfg_rx_veto_bounded_opening_and_tail_keep_clockless_unknowns(){
  let mut t=McfgRxVetoTrace::default();for n in 0..20{t.push(McfgRxEvent{at:None,tag:1,call:n,unread:8,early:0,watched_sequence:None,header:None});}
  assert_eq!((t.total,t.omitted),(20,8));assert_eq!(t.records.iter().flatten().map(|e|e.call).collect::<Vec<_>>(),[0,1,2,3,12,13,14,15,16,17,18,19]);assert!(t.records.iter().flatten().all(|e|e.at.is_none()&&e.header.is_none()&&e.watched_sequence.is_none()));
 }
 #[test]
 fn mcfg_rx_veto_header_uses_initial_data_only_and_exact_del_client(){
  let b=crate::data(7,&crate::owner_control_frame(6,[0,0x4a,0,0]));let h=RegistrationHeader::read(&b,false,None);assert_eq!(h.del_client,Some([0,0x4a]));assert_eq!(h.qrtr.unwrap()[0],6);
  let mut continuation=b.clone();continuation[0]=12;let h=RegistrationHeader::read(&continuation,false,None);assert!(h.qrtr.is_none()&&h.del_client.is_none());
 }
}
