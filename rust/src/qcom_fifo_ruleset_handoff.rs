//! At most four ordinary checked publications at the original ruleset admission.
use super::*;
pub type RulesetWriterTrace=EfsErrorTurnTrace;
impl<I:Io> Adapter<I>{
 pub(crate) fn publish_ruleset_error_turn(&mut self,g:u64,t:&mut RulesetWriterTrace)->Result<u32,Error>{
  self.check(g)?;if self.mcfg_guard{return Err(Error::NotReady);}t.calls=t.calls.saturating_add(1);let before=t.writes;let mut notifications=0;
  for i in 0..4{
   self.mcfg_guard=true;let result=self.publish_next(g);self.mcfg_guard=false;
   match result{
    Ok(Transmit::Session(op))=>{t.writes+=1;t.commands[i]=match op{Published::ReceiveDone(_)=>6,Published::ReceiveIntent(_)=>5,Published::IntentRequestAck{..}=>8,Published::Data{..}=>9,_=>u16::MAX};if !matches!(op,Published::ReceiveDone(_)|Published::ReceiveIntent(_)|Published::IntentRequestAck{..}){t.stop=1;break;}},
    Ok(Transmit::Blocked{read_notification_sent})=>{notifications+=u32::from(read_notification_sent);t.blocked+=1;t.stop=3;break;},
    Ok(Transmit::Idle)=>{let pending=self.tftp_rx_snapshot().is_some_and(|v|v!=(0,0));t.rx_veto+=u32::from(pending);t.stop=if pending{2}else{5};break;},
    Ok(Transmit::AwaitingIntent)=>{t.stop=4;break;},
    Err(error)=>{t.stop=6;return Err(error);},
   }
  }
  Ok(t.writes-before+notifications)
 }
}
#[cfg(all(test,mpss_glink_transport_tests))]
mod tests{
 use super::*;
 #[test]
 fn ruleset_writer_keeps_return_advertisement_ack_and_data_order(){let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(None);let mut t=RulesetWriterTrace::default();assert_eq!(a.publish_ruleset_error_turn(1,&mut t).unwrap(),4);assert_eq!(t.commands,[6,5,8,9]);let w=p.take_tx();assert_eq!([w[0],w[8],w[24],w[32]],[6,5,8,9]);assert_eq!((t.calls,t.writes,t.stop),(1,4,1));assert_eq!(a.efs_error_turn_trace().calls,0);}
 #[test]
 fn ruleset_writer_blocked_keeps_read_notification_fallback(){let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(Some(16));let mut t=RulesetWriterTrace::default();assert_eq!(a.publish_ruleset_error_turn(1,&mut t).unwrap(),1);assert_eq!((t.writes,t.blocked,t.stop),(0,1,3));let w=p.take_tx();assert_eq!(&w[w.len()-8..],&[13,0,0,0,0,0,0,0]);}
}
