//! One bounded ordinary writer group; no direct packet/intent selection.
use super::*;
#[derive(Clone,Copy,Debug,Default)]
pub struct EfsErrorTurnTrace{pub calls:u32,pub writes:u32,pub rx_veto:u32,pub blocked:u32,pub stop:u32,pub commands:[u16;4]}
impl<I:Io> Adapter<I>{
 pub fn efs_error_turn_trace(&self)->EfsErrorTurnTrace{self.ring.efs_error_turn}
 pub(crate) fn publish_efs_error_turn(&mut self,g:u64)->Result<u32,Error>{
  self.check(g)?;if self.mcfg_guard{return Err(Error::NotReady);}self.ring.efs_error_turn.calls=self.ring.efs_error_turn.calls.saturating_add(1);let before=self.ring.efs_error_turn.writes;let mut notifications=0;
  for i in 0..4{
   self.mcfg_guard=true;let result=self.publish_next(g);self.mcfg_guard=false;
   match result{
    Ok(Transmit::Session(op))=>{let t=&mut self.ring.efs_error_turn;t.writes+=1;t.commands[i]=match op{Published::ReceiveDone(_)=>6,Published::ReceiveIntent(_)=>5,Published::IntentRequestAck{..}=>8,Published::Data{..}=>9,_=>u16::MAX};if !matches!(op,Published::ReceiveDone(_)|Published::ReceiveIntent(_)|Published::IntentRequestAck{..}){t.stop=1;break;}},
    Ok(Transmit::Blocked{read_notification_sent})=>{notifications+=u32::from(read_notification_sent);self.ring.efs_error_turn.blocked+=1;self.ring.efs_error_turn.stop=3;break;},
    Ok(Transmit::Idle)=>{let pending=self.tftp_rx_snapshot().is_some_and(|v|v!=(0,0));self.ring.efs_error_turn.rx_veto+=u32::from(pending);self.ring.efs_error_turn.stop=if pending{2}else{5};break;},
    Ok(Transmit::AwaitingIntent)=>{self.ring.efs_error_turn.stop=4;break;},
    Err(error)=>{self.ring.efs_error_turn.stop=6;return Err(error);},
   }
  }
  Ok(self.ring.efs_error_turn.writes-before+notifications)
 }
}
#[cfg(all(test,mpss_glink_transport_tests))]
mod tests{
 use super::*;
 #[test]
 fn efs_early_adapter_keeps_return_advertisement_ack_and_data_order(){let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(None);let n=a.publish_efs_error_turn(1).unwrap();assert_eq!(n,4);let t=a.efs_error_turn_trace();assert_eq!(t.commands,[6,5,8,9]);let w=p.take_tx();assert_eq!([w[0],w[8],w[24],w[32]],[6,5,8,9]);assert_eq!((t.writes,t.stop),(4,1));}
 #[test]
 fn efs_early_adapter_blocked_writer_preserves_read_notification_fallback(){let(mut a,p,_,_)=crate::qcom_glink_fifo::mcfg_idle_fixture(Some(16));assert_eq!(a.publish_efs_error_turn(1).unwrap(),1);let t=a.efs_error_turn_trace();assert_eq!((t.writes,t.blocked,t.stop),(0,1,3));let w=p.take_tx();assert_eq!(&w[w.len()-8..],&[13,0,0,0,0,0,0,0]);}

}
