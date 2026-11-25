//! Explicit runtime opt-in; retained response/cache state supplies the scope.
use super::*;
impl<I:Io> Owner<I>{
    pub fn enable_mcfg_queued_turn(&mut self){self.mcfg_queued_enabled=true;}
    pub fn mcfg_queued_turn_trace(&self)->fifo::McfgQueuedTurnTrace{self.adapter.mcfg_queued_turn_trace()}
}
