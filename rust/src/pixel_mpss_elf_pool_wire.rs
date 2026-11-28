//! Read-only full-pool owners and bounded existing TX3 wire tail on one clock.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_elf_pool_wire(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"ELF POOL RX TAIL");
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
        let s=owner.service_registry_trace();let e=s.elf;let history=owner.registry_window_trace();let old=history.predecessor;let t=s.txn4;let pool=owner.elf_tx_slots();
        row(2,"ERROR DETAIL CLOSE STAGE TX3 CALLS MASK",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage,old.and_then(|p|p.publication).map_or(u32::MAX,|p|p.calls),old.and_then(|p|p.publication).map_or(u32::MAX,|p|p.mask)]);
        row(3,"TX3 Q PUB CORE TX4 Q PUB OWNER",&[age(old.and_then(|p|p.queued_at)),age(old.and_then(|p|p.published_at)),age(old.and_then(|p|p.done_at)),age(t.and_then(|t|t.queued)),age(t.and_then(|t|t.published)),age(t.and_then(|t|t.done))]);
        let elf_fifo=owner.elf_reply_fifo();
        row(4,"ELF RX Q PUB CORE OWNER TXCAP WAITS",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(elf_fifo.and_then(|f|f.core_done_at)),age(e.and_then(|e|e.done)),e.map_or(0,|e|e.waits_by_reason[2])]);
        for(n,label,f)in[(5,"TX3 TAIL ABS VIS AHEAD FIN EVICT",owner.registry_reply_fifo()),(6,"ELF TAIL ABS VIS AHEAD FIN EVICT",elf_fifo.map(|f|f.fifo))]{let r=f.and_then(|f|f.completion);row(n,label,&[age(f.and_then(|f|f.peer_tail_at)),age(r.and_then(|r|r.last_absent)),age(r.and_then(|r|r.first_visible)),r.and_then(|r|r.bytes_ahead).unwrap_or(u32::MAX),f.map_or(0,|f|u32::from(f.finished)),f.map_or(u32::MAX,|f|f.history_evictions)]);}
        let changed=pool.map_or(u32::MAX,|c|c.first.slots.iter().zip(c.latest.slots.iter()).enumerate().fold(0,|m,(i,(a,b))|m|(u32::from(a!=b)<<i)));
        row(7,"POOL FIRST LAST FAILS CHANGED ADMIT FIN",&[age(pool.and_then(|c|c.first.at)),age(pool.and_then(|c|c.latest.at)),pool.map_or(0,|c|c.failures),changed,age(pool.and_then(|c|c.admitted)),pool.map_or(0,|c|u32::from(c.finished))]);
        let free=pool.and_then(|c|c.first_free);let f=free.map(|f|f.original);
        row(8,"FREE US SLOTCAUSEMASK ADMIT SLOT EMPTY ACK",&[age(free.and_then(|f|f.at)),free.map_or(u32::MAX,|f|(u32::from(f.slot)<<16)|(u32::from(f.cause)<<8)|u32::from(f.empty_mask)),age(pool.and_then(|c|c.admitted)),pool.and_then(|c|c.admitted_slot).map_or(u32::MAX,u32::from),pool.map_or(u32::MAX,|c|u32::from(c.admitted_empty)),age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at))]);
        row(9,"FREED META SRC DST ID LENSENT TXNMSG",&[f.map_or(u32::MAX,|t|(u32::from(t.kind)<<24)|(u32::from(t.flags)<<16)|u32::from(t.op)),f.map_or(u32::MAX,|t|t.source.port),f.map_or(u32::MAX,|t|t.destination.port),f.map_or(u32::MAX,|t|t.remote),f.map_or(u32::MAX,|t|(u32::from(t.len)<<16)|u32::from(t.sent)),f.map_or(u32::MAX,|t|(u32::from(t.transaction)<<16)|u32::from(t.message))]);
        let wire=owner.registry_wire_headers();
        let count=wire.as_ref().map_or(0,|w|w.headers.iter().flatten().count());let opening=count.min(4);let tail_start=opening.max(count.saturating_sub(27));
        row(10,"WIRE RETAINED OMIT UNPARSED SKIP FIN DONE",&[count as u32,wire.as_ref().map_or(u32::MAX,|w|w.omitted),wire.as_ref().map_or(u32::MAX,|w|w.unparsed),(tail_start-opening)as u32,wire.as_ref().map_or(0,|w|u32::from(w.finished)),wire.as_ref().map_or(0,|w|u32::from(w.completion.is_some()))]);
        if pool.is_some(){
        for i in 0..8{let t=pool.map(|c|c.latest.slots[i]);row(11+i,"SLOT META SRC DST ID LENSENT TXNMSG",&[t.map_or(u32::MAX,|t|(u32::from(t.kind)<<24)|(u32::from(t.flags)<<16)|u32::from(t.op)),t.map_or(u32::MAX,|t|t.source.port),t.map_or(u32::MAX,|t|t.destination.port),t.map_or(u32::MAX,|t|t.remote),t.map_or(u32::MAX,|t|(u32::from(t.len)<<16)|u32::from(t.sent)),t.map_or(u32::MAX,|t|(u32::from(t.transaction)<<16)|u32::from(t.message))]);}
        }else{
            let p=old.and_then(|p|p.publication);let g=p.map_or([u32::MAX;8],|p|p.own_return_gates);row(11,"NO POOL OWN RETURN GATES 0 1 2 3",&g[..4]);row(12,"OWN RETURN GATES 4 5 6 7",&g[4..]);
            let t=p.map_or([u32::MAX;8],|p|p.turn_gates);let g=p.map_or([u32::MAX;12],|p|p.grant_gates);
            row(13,"TX3 PRIORITY 0 1 2 3 4 5",&t[..6]);row(14,"PRIORITY 6 7 GRANT 0 1 2 3",&[t[6],t[7],g[0],g[1],g[2],g[3]]);row(15,"GRANT 4 5 6 7 8 9",&g[4..10]);
            row(16,"GRANT A B ACK SENT BLOCK DONE ADS",&[g[10],g[11],p.map_or(u32::MAX,|p|p.ack.sent),p.map_or(u32::MAX,|p|p.ack.blocked),p.map_or(u32::MAX,|p|p.ack.done_ahead),p.map_or(u32::MAX,|p|p.ack.ads_ahead)]);
            let g=old.and_then(|p|p.grant);row(17,"FIRST CONVERT AD ACK BACKING",&[age(g.and_then(|g|g.converted_at)),age(g.and_then(|g|g.advertised_at)),age(g.and_then(|g|g.ack_at)),g.map_or(u32::MAX,|g|g.intent)]);
            row(18,"ELF WAITS NONE PEER TXCAP FLOW CREDIT CAP",&e.map_or([0;6],|e|e.waits_by_reason));
        }
        d.header_label(19,"US DIR CMD CH PARAM LEN QRTR SRC DST TAG A B");
        if let Some(w)=wire{for(index,h)in w.headers.iter().flatten().enumerate().filter(|(i,_)|*i<opening||*i>=tail_start).map(|(_,h)|h).chain(w.completion.as_ref()).enumerate(){
            let[kind,source,destination]=h.qrtr.unwrap_or([u32::MAX;3]);let(tag,a,b)=if let Some([a,b])=h.tftp{(0xA,a,b)}else if let Some([a,b])=h.del_client{(0xD,a,b)}else if let Some((flag,txn,msg))=h.qmi{(u32::from(flag),u32::from(txn),u32::from(msg))}else{(0xF,u32::MAX,u32::MAX)};
            d.wire_row(20+index/4,index%4,&[(age(h.at),8),(u32::from(h.tx),1),(u32::from(h.command),4),(u32::from(h.channel),4),(h.parameter,8),(u32::from(h.bytes),4),(kind,2),(source,8),(destination,8),(tag,1),(a,8),(b,8)]);
        }}
        let q=self.wlan_query.snapshot(owner);let ipa=owner.linux_ipa_report();
        row(28,"IPA PD WLAN WLFW PENDING ELF CREDIT",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),s.elf_exchange.and_then(|e|e.host_credit).map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2))]);
    }
}
