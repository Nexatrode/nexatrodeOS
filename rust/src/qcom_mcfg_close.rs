//! Exact validated MCFG client-close transaction. Published ownership stays live.
use super::*;
const _:()={assert!(REQUEST_SLOTS<=16);assert!(TX_STORAGE_SLOTS<=16);assert!(MAX_REQUESTS<=8);};
#[derive(Clone,Copy,Debug,Default,Eq,PartialEq)]
pub(crate) struct McfgCloseOutcome {
    pub request_mask:u8, pub receives:u8, pub unsent:u8, pub published:u8, pub credits:u8,
}
fn is_request(rx:&Receive)->bool {
    if rx.used<34 || word(&rx.bytes,4)!=1 || word(&rx.bytes,28)!=crate::qcom_tftp_server_check::SERVICE_PORT {return false;}
    let Some(end)=32usize.checked_add(word(&rx.bytes,20)as usize).filter(|n|*n<=rx.used) else{return false;};
    let b=&rx.bytes[32..end];b.starts_with(b"\0\x01/readwrite/mcfg.tmp\0")||b.starts_with(b"\0\x02/readwrite/mcfg.tmp\0")
}
impl Session {
    pub(crate) fn set_mcfg_close_transfers(&mut self,g:u64,transfers:[Option<(Address,u32)>;crate::qcom_tftp_server_check::TRANSFER_COUNT])->Result<(),Error>{
        self.check(g)?;self.mcfg_close_transfers=transfers;Ok(())
    }
    pub(super) fn hold_mcfg_close(&mut self,peer:Address,index:usize)->bool {
        let candidate=self.mcfg_close_transfers.iter().flatten().any(|(p,_)|*p==peer)
            || self.receive.iter().any(|r| matches!(r.state,RxState::Tftp|RxState::Deferred) && r.used>=32
                && word(&r.bytes,8)==peer.node && word(&r.bytes,12)==peer.port && is_request(r));
        if candidate {self.pending_mcfg_close=Some((peer,index));}candidate
    }
    pub(crate) fn commit_mcfg_close(&mut self,g:u64,peer:Address,port:Option<u32>,handles:&[Option<TftpHandle>;MAX_REQUESTS])->Result<McfgCloseOutcome,Error>{
        if g!=self.generation{return Err(Error::StaleGeneration);}
        if self.quarantined{return Err(Error::Quarantined);}
        if peer.node!=self.config.peer_node || self.pending_mcfg_close.map(|p|p.0)!=Some(peer){return Err(Error::InvalidHandle);}
        let result=(||{
            let (_,closing)=self.pending_mcfg_close.ok_or(Error::InvalidHandle)?;
            let close=&self.receive[closing];
            if close.state!=RxState::Held || close.used<52{return Err(Error::InvalidHandle);}
            let end=32usize.checked_add(word(&close.bytes,20)as usize).filter(|n|*n<=close.used).ok_or(Error::InvalidHandle)?;
            if self.control.decode(&close.bytes[..end])?!=control::Command::DelClient(peer.port){return Err(Error::InvalidHandle);}
            let mut known=self.mcfg_close_transfers.iter().flatten().filter(|(p,_)|*p==peer);
            let cached=known.next().map(|(_,p)|*p);
            // At most one RAM transfer per peer; the exclusive server rechecks it.
            if known.next().is_some() || cached!=port{return Err(Error::InvalidHandle);}
            if self.retired_rmtfs_peers.contains(&Some(peer)){return Err(Error::FlowBusy);}
            let tombstone=self.retired_rmtfs_peers.iter().position(Option::is_none).ok_or(Error::NoCapacity)?;
            let mut owned=0u16;let mut mask=0u8;
            for (slot,h) in handles.iter().enumerate(){if let Some(h)=h{
                if h.owner!=self.owner || h.generation!=g{return Err(Error::InvalidHandle);}
                let i=self.receive.iter().position(|r|r.state==RxState::Tftp && r.tftp_datagram_id()==h.intent).ok_or(Error::InvalidHandle)?;
                if owned&(1<<i)!=0{return Err(Error::InvalidHandle);}owned|=1<<i;
                if word(&self.receive[i].bytes,8)==peer.node && word(&self.receive[i].bytes,12)==peer.port{mask|=1<<slot;}
            }}
            let mut rx_mask=0u16;
            for (i,r) in self.receive.iter().enumerate(){
                if i==closing || r.used<32 || word(&r.bytes,8)!=peer.node || word(&r.bytes,12)!=peer.port{continue;}
                if matches!(r.state,RxState::Free|RxState::Available|RxState::Advertising){continue;}
                if r.state==RxState::Control && r.context.is_none() && r.ticket.is_none(){continue;}
                let held=r.state==RxState::Tftp && owned&(1<<i)!=0;
                let deferred=r.state==RxState::Deferred;
                let scoped=is_request(r) || port.is_some_and(|p|word(&r.bytes,28)==p && word(&r.bytes,4)==1);
                if (!held&&!deferred)||!scoped||r.context.is_some()||r.ticket.is_some(){return Err(Error::FlowBusy);}
                rx_mask|=1<<i;
            }
            let mut tx_mask=0u16;let mut out=McfgCloseOutcome{request_mask:mask,receives:rx_mask.count_ones()as u8,..McfgCloseOutcome::default()};
            for (i,t) in self.transmit.iter().enumerate(){
                if t.len<32 || word(&t.bytes,24)!=peer.node || word(&t.bytes,28)!=peer.port{continue;}
                if matches!(t.kind,TxKind::Resume) && word(&t.bytes,4)==7 && word(&t.bytes,8)==self.config.local_node
                    && (word(&t.bytes,12)==crate::qcom_tftp_server_check::SERVICE_PORT || port==Some(word(&t.bytes,12))){out.credits+=1;continue;}
                if !matches!(t.kind,TxKind::TftpReply{source,destination} if destination==peer && source.node==self.config.local_node && Some(source.port)==port){return Err(Error::FlowBusy);}
                if t.sent==0 && t.remote_slot.is_none(){
                    if t.reference_flow.is_none(){return Err(Error::InvalidHandle);}tx_mask|=1<<i;out.unsent+=1;
                }else if t.sent>0 && t.sent<=t.len && t.remote_slot.is_some(){out.published+=1;}
                else{return Err(Error::FlowBusy);}
            }
            // Only internal state and optional diagnostic clocks change after preflight.
            for i in 0..self.transmit.len(){if tx_mask&(1<<i)!=0{
                let t=&self.transmit[i];let (from,to)=t.kind.qrtr_addresses().ok_or(Error::InvalidHandle)?;
                self.rmtfs_reference_flows.fail_unpublished(t.reference_flow.as_ref().ok_or(Error::InvalidHandle)?).map_err(|_|Error::FlowBusy)?;
                #[cfg(feature="sunfish-mcfg-record-identity")]
                self.observe_post_mcfg_cancel(i);
                self.transmit[i]=Transmit::empty();self.observe_elf_tx_freed(i,5);self.forget_qrtr_flow_if_idle(from,to);
            }}
            let key=crate::qcom_qrtr_flow::Destination{node:peer.node,port:peer.port};
            if out.published==0 && self.rmtfs_reference_flows.contains(key){self.rmtfs_reference_flows.close(key).map_err(|_|Error::FlowBusy)?;}
            let event=self.control.receive(&self.receive[closing].bytes[..end])?;
            if event!=control::Event::ClientRemoved(peer.port){return Err(Error::InvalidHandle);}self.observe_client_control(event)?;
            for i in 0..self.receive.len(){if i==closing||rx_mask&(1<<i)!=0{self.receive[i].state=RxState::Control;self.receive[i].done_pending=true;}}
            self.retired_rmtfs_peers[tombstone]=Some(peer);self.pending_mcfg_close=None;Ok(out)
        })();
        if result.is_err(){self.quarantined=true;}result
    }
}
