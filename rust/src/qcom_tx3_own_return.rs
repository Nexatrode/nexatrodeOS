//! One normal checked return of the original queued TX3 request before a fresh AD.
use super::*;
impl Session {
    fn tx3_return_scope(tx:&Transmit)->Option<usize> {
        if tx.len==0 || tx.sent!=0{return None;}
        let TxKind::Reply{receive,route,..}=tx.kind else{return None;};
        (route.service==Service::ServiceRegistry && route.transaction==3
            && route.source.port==crate::qcom_servreg::LOCAL_PORT
            && route.destination==(Address{node:0,port:9})).then_some(receive)
    }
    pub(super) fn tx3_own_return_candidate(&self,ordinary:bool,protected:bool)->(Option<(usize,usize)>,usize) {
        if !ordinary{return (None,0);}
        if protected || self.registry_turn_used || self.registry_converted_grant.is_some(){return (None,1);}
        if self.intent_request_ack!=Some(true){return (None,2);}
        if !self.intent_ack_backing.is_some_and(|id|self.receive[..RX_SLOTS].iter()
            .any(|r|r.id==id&&r.state==RxState::Advertising)){return (None,3);}
        let Some((slot,rx))=self.transmit.iter().enumerate().find_map(|(i,tx)|{
            let rx=Self::tx3_return_scope(tx)?;
            (tx.registry_return_turn_due && !tx.registry_own_return_used
                && !tx.registry_grant_turn_used && !tx.registry_credit_turn_used
                && rx<RX_SLOTS && self.receive[rx].state==RxState::ReplyQueued && self.receive[rx].done_pending)
                .then_some((i,rx))
        })else{return (None,4);};
        // Preserve the normal unprotected DATA order, fitting-intent, outbound
        // intent-wait and flow-predecessor checks. Only its own return moves.
        if self.next_transmit_in_order([0]).is_none_or(|(i,_)|i!=slot){return (None,5);}
        if self.next_transmit_in_order([1]).is_some(){return (None,6);}
        (Some((slot,rx)),7)
    }
    pub(super) fn observe_tx3_own_return_gate(&mut self,gate:usize) {
        for tx in &mut self.transmit {
            if Self::tx3_return_scope(tx).is_none(){continue;}
            let n=&mut tx.registry_publication.own_return_gates[gate];*n=n.saturating_add(1);
        }
    }
}
