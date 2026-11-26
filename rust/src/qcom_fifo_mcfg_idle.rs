//! Same bounded checked writer, with committed operations reported to the owner.
use super::*;
#[derive(Clone,Copy,Debug,Default,Eq,PartialEq)]
pub struct McfgIdleTurnTrace{pub calls:u32,pub writes:u32,pub rx_veto:u32,pub blocked:u32}
#[derive(Default)]
pub(super) struct TurnProgress{writes:u32,rx_veto:bool,blocked:bool}
impl<I:Io> Adapter<I>{
 pub(crate) fn mcfg_idle_turn_trace(&self)->McfgIdleTurnTrace{self.mcfg_idle}
 pub(crate) fn publish_mcfg_idle_turn(&mut self,g:u64)->Result<u32,Error>{
  let mut p=TurnProgress::default();self.mcfg_idle.calls=self.mcfg_idle.calls.saturating_add(1);
  let result=self.publish_mcfg_checked_turn(g,&mut p);
  self.mcfg_idle.writes=self.mcfg_idle.writes.saturating_add(p.writes);self.mcfg_idle.rx_veto=self.mcfg_idle.rx_veto.saturating_add(u32::from(p.rx_veto));self.mcfg_idle.blocked=self.mcfg_idle.blocked.saturating_add(u32::from(p.blocked));
  result.map(|_|p.writes)
 }
 pub(super) fn publish_mcfg_checked_turn(&mut self,g:u64,p:&mut TurnProgress)->Result<(),Error>{
  self.check(g)?;self.mcfg_queued.calls=self.mcfg_queued.calls.saturating_add(1);
  for _ in 0..4 {
   self.mcfg_guard=true;let result=self.publish_next(g);self.mcfg_guard=false;
   match result?{
    Transmit::Session(op)=>{p.writes+=1;self.mcfg_queued.writes=self.mcfg_queued.writes.saturating_add(1);if !matches!(op,Published::ReceiveDone(_)|Published::ReceiveIntent(_)|Published::IntentRequestAck{..}){break;}},
    Transmit::Blocked{..}=>{p.blocked=true;self.mcfg_queued.blocked=self.mcfg_queued.blocked.saturating_add(1);break;},
    Transmit::Idle=>{p.rx_veto=self.tftp_rx_snapshot().is_some_and(|v|v!=(0,0));break;},
    Transmit::AwaitingIntent=>break,
   }
  }
  Ok(())
 }
}
