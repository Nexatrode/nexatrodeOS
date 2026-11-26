use super::*;
impl<I:Io> Owner<I>{
    pub fn enable_efs_reply_overlap(&mut self){self.adapter.enable_efs_reply_overlap();}
    pub fn efs_overlap_trace(&self)->crate::qcom_glink_qmi::EfsOverlapTrace{self.adapter.efs_overlap_trace()}
}
