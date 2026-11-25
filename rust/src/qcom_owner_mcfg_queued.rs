//! Explicit runtime opt-in; retained response/cache state supplies the scope.
use super::*;
impl<I:Io> Owner<I>{
/* MCFG_RX_VETO_BEGIN */
#[cfg(feature="sunfish-mcfg-rx-veto-trace")]
    pub fn mcfg_rx_veto_trace(&self)->&fifo::McfgRxVetoTrace{self.adapter.mcfg_rx_veto_trace()}
/* MCFG_RX_VETO_END */
    pub fn enable_mcfg_queued_turn(&mut self){self.mcfg_queued_enabled=true;}
    pub fn mcfg_queued_turn_trace(&self)->fifo::McfgQueuedTurnTrace{self.adapter.mcfg_queued_turn_trace()}
}
