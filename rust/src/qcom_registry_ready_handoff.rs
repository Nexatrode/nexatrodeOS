//! Live protocol dependency, separate from all diagnostic captures.
use super::*;
impl Session {
    pub(crate) fn registry_unpublished_blocker(&self,waiting:&RequestHandle)->Option<RequestHandle>{
        let index=self.request_index(waiting).ok()?;
        let ticket=self.receive[index].ticket.as_ref()?;
        let route=self.router.reply_route(ticket).ok()?;
        if route.service!=Service::ServiceRegistry{return None;}
        let mut found=None;
        for tx in self.transmit.iter().filter(|t|t.len!=0 && t.sent<t.len){
            let TxKind::Reply{receive,route:older,..}=tx.kind else{continue;};
            if older.service!=Service::ServiceRegistry || older.source!=route.source || older.destination!=route.destination{continue;}
            let h=RequestHandle{owner:self.owner,generation:self.generation,intent:self.receive[receive].id};
            if h==*waiting{continue;}
            if found.is_some(){return None;}
            found=Some(h);
        }
        found
    }
}
