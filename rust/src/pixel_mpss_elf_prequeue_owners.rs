//! Read-only projection of existing first-ELF phases, admission owners and pool history.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_elf_prequeue_owners(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}
        d.replace_line(1,"ELF PREQUEUE OWNERS");
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let row=|n,label,values:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in values.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..values.len()]);};
        let s=owner.service_registry_trace();let e=s.elf;let w=owner.elf_window_trace();let pool=owner.elf_tx_slots();
        let ipa=owner.linux_ipa_report();let flags=ipa.and_then(|r|r.protocol.map(|p|u32::from(p.modem_ready)|(u32::from(p.qmi_uc_ready)<<1)|(u32::from(p.indication_requested)<<2)|(u32::from(p.indication_sent)<<3)|(u32::from(r.protocol_complete)<<4)|(u32::from(r.failed)<<5)|(u32::from(r.pending_record)<<6)|(u32::from(r.record_queued)<<7)|(u32::from(r.held_response)<<8)|(u32::from(p.initial_boot)<<9)));
        row(2,"ERROR DETAIL IPA PENDING CLOSE STAGE",&[self.error,self.detail,flags.unwrap_or(u32::MAX),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage]);
        row(3,"ELF RX QUEUE PUB DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
        row(4,"RX END REG ENTER SELECT PASSES",&[age(e.and_then(|e|e.dispatch.receive_pass_end)),age(e.and_then(|e|e.dispatch.registry_entered)),age(e.and_then(|e|e.dispatch.selected)),w.map_or(0,|w|w.writer.passes)]);
        let prior=owner.registry_window_trace().predecessor;let fifo=owner.registry_reply_fifo();let rx=fifo.and_then(|f|f.completion);
        row(5,"TX3 Q PUB TAIL DONE RXVIS AHEAD",&[age(prior.and_then(|p|p.queued_at)),age(prior.and_then(|p|p.published_at)),age(fifo.and_then(|f|f.peer_tail_at)),age(prior.and_then(|p|p.done_at)),age(rx.and_then(|r|r.first_visible)),rx.and_then(|r|r.bytes_ahead).unwrap_or(u32::MAX)]);
        row(6,"TX4 RX QUEUE PUB DONE",&[age(s.txn4.and_then(|t|t.received)),age(s.txn4.and_then(|t|t.queued)),age(s.txn4.and_then(|t|t.published)),age(s.txn4.and_then(|t|t.done))]);
        let waits=e.map_or([0;6],|e|e.waits_by_reason);row(7,"WAITS NONE PEER TXCAP FLOW CREDIT CAP",&waits);
        let turns=e.map(|e|e.dispatch.turns);let runs=turns.map_or([0;4],|t|t.runs);
        row(8,"PRESELECT CALLS OMIT RUN0 RUN1 RUN2 RUN3",&[turns.map_or(0,|t|t.calls),turns.map_or(0,|t|t.omitted),runs[0],runs[1],runs[2],runs[3]]);
        for(n,label,a)in[(9,"FIRST FAIL US SEQREASON N GATES",w.and_then(|w|w.first_failure)),(11,"LAST FAIL US SEQREASON N GATES",w.and_then(|w|w.last_failure))]{
            row(n,label,&[age(a.and_then(|a|a.at)),a.map_or(u32::MAX,|a|(a.sequence.min(0xffffff)<<8)|u32::from(a.reason)),a.map_or(u32::MAX,|a|u32::from(a.count)),a.map_or(u32::MAX,|a|u32::from(a.gates))]);
            let a0=a.and_then(|a|a.entries[0]);let a1=a.and_then(|a|a.entries[1]);
            row(n+1,"OWNERS A TXNFLAGS ID B TXNFLAGS ID",&[a0.map_or(u32::MAX,|v|(u32::from(v.transaction)<<16)|(u32::from(v.flags)<<8)|u32::from(v.kind)),a0.map_or(u32::MAX,|v|v.remote_intent),a1.map_or(u32::MAX,|v|(u32::from(v.transaction)<<16)|(u32::from(v.flags)<<8)|u32::from(v.kind)),a1.map_or(u32::MAX,|v|v.remote_intent)]);
        }
        let admitted=w.and_then(|w|w.admitted);
        row(13,"ADMIT US SEQ N GATES LASTRX TRIES",&[age(admitted.and_then(|a|a.at)),admitted.map_or(u32::MAX,|a|a.sequence),admitted.map_or(u32::MAX,|a|u32::from(a.count)),admitted.map_or(u32::MAX,|a|u32::from(a.gates)),w.and_then(|w|w.last_before_admission).map_or(u32::MAX,u32::from),w.map_or(0,|w|w.attempts)]);
        for i in 0..2{let a=w.and_then(|w|w.receipts[i*2]);let b=w.and_then(|w|w.receipts[i*2+1]);row(14+i,if i==0{"RX0 RX1 US TXNSTATE INTENT"}else{"RX2 RX3 US TXNSTATE INTENT"},&[age(a.and_then(|r|r.at)),a.map_or(u32::MAX,|r|(u32::from(r.transaction)<<16)|(u32::from(r.remaining)<<8)|(u32::from(r.target)<<1)|u32::from(r.matched_last_failure)),a.map_or(u32::MAX,|r|r.remote_intent),age(b.and_then(|r|r.at)),b.map_or(u32::MAX,|r|(u32::from(r.transaction)<<16)|(u32::from(r.remaining)<<8)|(u32::from(r.target)<<1)|u32::from(r.matched_last_failure)),b.map_or(u32::MAX,|r|r.remote_intent)]);}
        row(16,"RECEIPTS OMIT SEQ0 SEQ1 SEQ2 SEQ3",&[w.map_or(0,|w|w.receipt_count),w.map_or(0,|w|w.receipt_omitted),w.and_then(|w|w.receipts[0]).map_or(u32::MAX,|r|r.sequence),w.and_then(|w|w.receipts[1]).map_or(u32::MAX,|r|r.sequence),w.and_then(|w|w.receipts[2]).map_or(u32::MAX,|r|r.sequence),w.and_then(|w|w.receipts[3]).map_or(u32::MAX,|r|r.sequence)]);
        let changed=pool.map_or(u32::MAX,|c|c.first.slots.iter().zip(c.latest.slots.iter()).enumerate().fold(0,|m,(i,(a,b))|m|(u32::from(a!=b)<<i)));
        row(17,"POOL FIRST LAST FAILS CHANGED ADMIT END",&[age(pool.and_then(|c|c.first.at)),age(pool.and_then(|c|c.latest.at)),pool.map_or(0,|c|c.failures),changed,age(pool.and_then(|c|c.admitted)),pool.map_or(0,|c|u32::from(c.finished))]);
        for i in 0..8{let t=pool.map(|c|c.latest.slots[i]);row(18+i,"SLOT META SRC DST ID LENSENT TXNMSG",&[t.map_or(u32::MAX,|t|(u32::from(t.kind)<<24)|(u32::from(t.flags)<<16)|u32::from(t.op)),t.map_or(u32::MAX,|t|t.source.port),t.map_or(u32::MAX,|t|t.destination.port),t.map_or(u32::MAX,|t|t.remote),t.map_or(u32::MAX,|t|(u32::from(t.len)<<16)|u32::from(t.sent)),t.map_or(u32::MAX,|t|(u32::from(t.transaction)<<16)|u32::from(t.message))]);}
        let free=pool.and_then(|c|c.first_free);let t=free.map(|f|f.original);
        row(26,"FREE US SLOTCAUSEMASK META SRC DST ID",&[age(free.and_then(|f|f.at)),free.map_or(u32::MAX,|f|(u32::from(f.slot)<<16)|(u32::from(f.cause)<<8)|u32::from(f.empty_mask)),t.map_or(u32::MAX,|t|(u32::from(t.kind)<<24)|(u32::from(t.flags)<<16)|u32::from(t.op)),t.map_or(u32::MAX,|t|t.source.port),t.map_or(u32::MAX,|t|t.destination.port),t.map_or(u32::MAX,|t|t.remote)]);
        row(27,"FREE LEN QMI NODES ADMIT SLOT EMPTY",&[t.map_or(u32::MAX,|t|(u32::from(t.len)<<16)|u32::from(t.sent)),t.map_or(u32::MAX,|t|(u32::from(t.transaction)<<16)|u32::from(t.message)),t.map_or(u32::MAX,|t|t.source.node),t.map_or(u32::MAX,|t|t.destination.node),pool.and_then(|c|c.admitted_slot).map_or(u32::MAX,u32::from),pool.map_or(u32::MAX,|c|u32::from(c.admitted_empty))]);
        let q=self.wlan_query.snapshot(owner);let pair=owner.efs_overlap_trace();
        row(28,"IPA PD WLAN WLFW ACK PAIR",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),(pair.flags&255)|((pair.admitted&255)<<8)|((pair.offers&65535)<<16)]);
    }
}
