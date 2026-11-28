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
                && !tx.registry_grant_turn_used && (!tx.registry_credit_turn_used
/* TX3_CREDIT_BEGIN */
                    || {#[cfg(feature="sunfish-tx3-credit-return-turn")] {self.tx3_own_credit_can_continue(i)}
                        #[cfg(not(feature="sunfish-tx3-credit-return-turn"))] {false}}
/* TX3_CREDIT_END */
                )
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
/* TX3_CREDIT_BEGIN */
#[cfg(feature="sunfish-tx3-credit-return-turn")]
impl Session {
    pub(super) fn tx3_own_credit_can_continue(&self,index:usize)->bool {
        let tx=&self.transmit[index];
        tx.tx3_own_credit_committed && tx.registry_credit_turn_used
            && tx.registry_return_turn_due && !tx.registry_grant_turn_used
            && Self::tx3_return_scope(tx).is_some() && self.registry_converted_grant.is_none()
    }
    pub(super) fn tx3_own_credit_candidate(&self,ordinary:bool,protected:bool)
        ->Option<(usize,usize,usize)> {
        if !ordinary || protected || self.registry_turn_used || self.registry_converted_grant.is_some()
            || self.intent_request_ack!=Some(true)
            || !self.intent_ack_backing.is_some_and(|id|self.receive[..RX_SLOTS].iter()
                .any(|r|r.id==id&&r.state==RxState::Advertising)){return None;}
        let target=self.transmit.iter().enumerate().find_map(|(i,tx)|{
            let rx=Self::tx3_return_scope(tx)?;
            (tx.registry_return_turn_due && !tx.registry_own_return_used
                && !tx.registry_grant_turn_used && !tx.registry_credit_turn_used
                && !tx.tx3_own_credit_committed && rx<RX_SLOTS
                && self.receive[rx].state==RxState::ReplyQueued && self.receive[rx].done_pending)
                .then_some(i)
        })?;
        if self.next_transmit_in_order([0]).is_none_or(|(i,_)|i!=target){return None;}
        let(credit,remote)=self.next_transmit_in_order([1])?;
        let c=&self.transmit[credit];
        (matches!(c.kind,TxKind::Resume)&&c.sent==0&&c.len==52).then_some((target,credit,remote))
    }
}
/* TX3_CREDIT_END */
