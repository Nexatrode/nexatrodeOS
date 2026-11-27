use super::*;
impl<I:Io> Adapter<I>{
    pub(crate) fn enable_efs_reply_overlap(&mut self){self.session.enable_efs_reply_overlap();}
    pub fn efs_overlap_trace(&self)->crate::qcom_glink_qmi::EfsOverlapTrace{self.session.efs_overlap_trace()}
}
