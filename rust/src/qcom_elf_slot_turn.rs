//! Scheduling eligibility only. Normal queue, FIFO and writer checks remain authoritative.
use super::*;
impl Session {
    /// Consume the exact validated receipt from the current ingest call. No
    /// diagnostic history, transaction-number heuristic or clock is authority.
    pub(crate) fn take_elf_slot_turn(&mut self,completed:&RequestHandle,waiting:&RequestHandle)->bool {
        let Some((actual,prior))=self.elf_slot_receipt.take() else{return false;};
        if actual!=*completed || completed==waiting {return false;}
        let Ok(index)=self.request_index(waiting) else{return false;};
        let Some(ticket)=self.receive[index].ticket.as_ref() else{return false;};
        let Ok(route)=self.router.reply_route(ticket) else{return false;};
        if route.service!=Service::ServiceRegistry || prior.service!=Service::ServiceRegistry
            || route.source!=prior.source || route.destination!=prior.destination {return false;}
        let mut peers=self.transmit.iter().filter(|tx|tx.len!=0
            && tx.kind.qrtr_addresses().is_some_and(|(_,peer)|peer==route.destination));
        let Some(older)=peers.next() else{return true;};
        // Conservative live mirror of the existing second-reply eligibility.
        // This does not reserve credit or admit a reply; queue_reply repeats all
        // normal checks, including credit/capacity and actual response validation.
        peers.next().is_none() && matches!(older.kind,TxKind::Reply{route:old,..}
            if old.service==Service::ServiceRegistry && old.source==route.source)
            && older.sent==older.len && older.remote_slot.is_some()
            && !older.glink_done && older.qrtr_done && older.reference_flow.is_some()
    }
}
