//! One exact repeated EFS error may coexist with its fully published predecessor.
//! Authority comes from accepted request bytes and committed transmit identity.
use super::*;
const REQUEST_LIMIT:usize=256;
pub(super) struct EfsOverlap {
    enabled:bool,
    first:Option<TftpHandle>,second:Option<TftpHandle>,peer:Address,
    bytes:[u8;REQUEST_LIMIT],len:usize,exact:bool,closed:bool,second_queued:bool,
    first_tx:Option<(usize,crate::qcom_qrtr_flow::Reservation)>,
    offers:u32,admitted:u32,
}
impl EfsOverlap {
    pub(super) const EMPTY:Self=Self{enabled:false,first:None,second:None,peer:Address{node:0,port:0},bytes:[0;REQUEST_LIMIT],len:0,exact:false,closed:false,second_queued:false,first_tx:None,offers:0,admitted:0};
}
#[derive(Clone,Copy,Debug)]
pub struct EfsOverlapTrace {pub flags:u32,pub offers:u32,pub admitted:u32}
impl Session {
    pub(crate) fn enable_efs_reply_overlap(&mut self){self.efs_overlap.enabled=true;}
    pub fn efs_overlap_trace(&self)->EfsOverlapTrace{
        let p=&self.efs_overlap;
        EfsOverlapTrace{flags:u32::from(p.enabled)|(u32::from(p.first.is_some())<<1)|(u32::from(p.second.is_some())<<2)|(u32::from(p.exact)<<3)|(u32::from(p.first_tx.is_some())<<4)|(u32::from(p.closed)<<5)|(u32::from(p.second_queued)<<6),offers:p.offers,admitted:p.admitted}
    }
    pub(super) fn capture_efs_overlap_request(&mut self,index:usize,handle:TftpHandle){
        if !self.efs_overlap.enabled{return;}
        let rx=&self.receive[index];let end=32usize.checked_add(word(&rx.bytes,20)as usize);
        let Some(body)=end.filter(|n|*n<=rx.used).and_then(|n|rx.bytes.get(32..n))else{return;};
        if word(&rx.bytes,28)!=crate::qcom_tftp_server_check::SERVICE_PORT || !body.starts_with(b"\0\x01/readwrite/modem_efs\0"){return;}
        let peer=Address{node:word(&rx.bytes,8),port:word(&rx.bytes,12)};let p=&mut self.efs_overlap;
        if p.first==Some(handle)||p.second==Some(handle){return;}
        if p.first.is_none(){
            p.first=Some(handle);p.peer=peer;
            if body.len()<=REQUEST_LIMIT {p.len=body.len();p.bytes[..p.len].copy_from_slice(body);}
        }else if p.second.is_none(){
            p.second=Some(handle);p.exact=p.len!=0&&peer==p.peer&&body==&p.bytes[..p.len];
        }
    }
    pub(super) fn close_efs_overlap(&mut self,port:u32){
        let p=&mut self.efs_overlap;if p.first.is_some()&&p.peer.node==self.config.peer_node&&p.peer.port==port{p.closed=true;}
    }
    pub(super) fn efs_overlap_candidate(&mut self,handle:&TftpHandle,route:TftpReplyRoute,packet:&[u8])->Option<usize>{
        let p=&self.efs_overlap;
        if !p.enabled||!p.exact||p.closed||p.second_queued||p.second!=Some(*handle)||route.destination!=p.peer||!crate::qcom_tftp_server_check::is_transfer_port(route.source.port)||packet!=crate::qcom_tftp_server_check::MISSING_REPLY{return None;}
        let (slot,token)=p.first_tx?;let tx=self.transmit.get(slot)?;
        if tx.len==0||tx.sent!=tx.len||tx.glink_done||tx.remote_slot.is_none()||tx.reference_flow!=Some(token)
            || !matches!(tx.kind,TxKind::TftpReply{source,destination}if source==route.source&&destination==route.destination)
            || word(&tx.bytes,20)as usize!=packet.len()||tx.bytes.get(32..32+packet.len())!=Some(packet){return None;}
        self.efs_overlap.offers=self.efs_overlap.offers.saturating_add(1);Some(slot)
    }
    pub(super) fn bind_efs_overlap_queue(&mut self,handle:&TftpHandle,route:TftpReplyRoute,packet:&[u8],result:Result<usize,Error>,overlap:Option<usize>){
        let Ok(index)=result else{return;};let p=&mut self.efs_overlap;
        if !p.enabled||route.destination!=p.peer||!crate::qcom_tftp_server_check::is_transfer_port(route.source.port)||packet!=crate::qcom_tftp_server_check::MISSING_REPLY{return;}
        if p.first==Some(*handle)&&p.first_tx.is_none(){
            p.first_tx=self.transmit[index].reference_flow.map(|token|(index,token));
        }else if p.second==Some(*handle){
            p.second_queued=true;
            if overlap.is_some_and(|slot|p.first_tx.is_some_and(|(first,_)|first==slot)){p.admitted=p.admitted.saturating_add(1);}
        }
    }
}
