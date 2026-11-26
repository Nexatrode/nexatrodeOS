//! Explicit runtime opt-in, consumed at first exact EFS error queue admission.
use super::*;
impl<I:Io> Owner<I>{
 pub fn enable_efs_error_turn(&mut self){self.efs_error_turn_enabled=true;}
 pub fn efs_error_turn_trace(&self)->fifo::EfsErrorTurnTrace{self.adapter.efs_error_turn_trace()}
}
