//! Exclusive Owner/RAM-server completion of a paused MCFG client close.
use super::*;
use crate::qcom_qmi_router::Address;
#[derive(Clone,Copy,Debug,Default)]
pub struct McfgCloseTrace {
    pub observed:u32,pub committed:u32,pub held:u32,pub retained:u32,pub unsent:u32,pub published:u32,pub credits:u32,
    pub peer:Option<Address>,pub at:Option<u64>,
}
impl<I:Io> Owner<I>{
    pub fn pending_mcfg_close(&self)->Option<Address>{self.pending_mcfg_close}
    pub fn mcfg_close_trace(&self)->McfgCloseTrace{self.mcfg_close_trace}
    pub(super) fn refresh_mcfg_transfers(&mut self,server:&crate::qcom_tftp_server_check::ServerCheck){
        self.mcfg_close_transfers=server.mcfg_close_transfers().map(|entry|entry.map(|(p,port)|(Address{node:p.node,port:p.port},port)));
    }
    pub fn resolve_mcfg_close(&mut self,server:&mut crate::qcom_tftp_server_check::ServerCheck)->Result<(),Error>{
        if self.stopped{return Err(Error::Stopped);}
        let peer=self.pending_mcfg_close.ok_or(Error::Stopped)?;
        let result=(||{
            let key=crate::qcom_tftp_server_check::Address{node:peer.node,port:peer.port};
            let port=server.preflight_mcfg_close(key).map_err(|_|Error::Stopped)?;
            if self.pending_tftp_reply.is_some_and(|r|r.route.destination==peer){return Err(Error::Stopped);}
            let mut retained=0u8;
            for (i,r) in self.peer_tftp_replies.iter().enumerate(){if let Some(r)=r.as_ref().filter(|r|r.meta.route.destination==peer){
                if r.meta.route.source.node!=1 || Some(r.meta.route.source.port)!=port{return Err(Error::Stopped);}retained|=1<<i;
            }}
            // Hold the Owner terminal across every potentially failing commit.
            self.stopped=true;
            let out=self.adapter.commit_mcfg_close(self.generation,peer,port,&self.tftp_requests)?;
            server.close_mcfg_peer(key).map_err(|_|Error::Stopped)?;
            for i in 0..MAX_REQUESTS{
                if out.request_mask&(1<<i)!=0{self.tftp_requests[i]=None;}
                if retained&(1<<i)!=0{self.peer_tftp_replies[i]=None;}
            }
            self.refresh_mcfg_transfers(server);
            self.pending_mcfg_close=None;
            let t=&mut self.mcfg_close_trace;t.committed=t.committed.saturating_add(1);t.held=t.held.saturating_add(u32::from(out.receives));t.retained=t.retained.saturating_add(retained.count_ones());t.unsent=t.unsent.saturating_add(u32::from(out.unsent));t.published=t.published.saturating_add(u32::from(out.published));t.credits=t.credits.saturating_add(u32::from(out.credits));t.peer=Some(peer);t.at=self.rmtfs_event_clock.map(|c|c());
            self.stopped=false;Ok(())
        })();
        result.map_err(|e|self.stop(e))
    }
}
