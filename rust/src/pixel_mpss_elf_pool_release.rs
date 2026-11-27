//! Existing first/latest full-pool records; no new transport observations.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_elf_pool_release(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"ELF POOL FIRST RELEASE");
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
        let s=owner.service_registry_trace();let e=s.elf;let c=owner.elf_tx_slots();let w=owner.registry_window_trace();let p=w.predecessor;let t=s.txn4;
        row(2,"ERROR DETAIL IPA PENDING CLOSE STAGE",&[self.error,self.detail,u32::from(self.ipa_complete_seen),owner.linux_ipa_report().and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage]);
        row(3,"TX3 Q PUB CORE TX4 RX Q PUB",&[age(p.and_then(|p|p.queued_at)),age(p.and_then(|p|p.published_at)),age(p.and_then(|p|p.done_at)),age(t.and_then(|t|t.received)),age(t.and_then(|t|t.queued)),age(t.and_then(|t|t.published))]);
        row(4,"ELF RX QUEUE PUB OWNER DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
        let changed=c.map_or(u32::MAX,|c|c.first.slots.iter().zip(c.latest.slots.iter()).enumerate().fold(0,|m,(i,(a,b))|m|(u32::from(a!=b)<<i)));
        row(5,"POOL FIRST LAST FAILS CHANGED ADMIT END",&[age(c.and_then(|c|c.first.at)),age(c.and_then(|c|c.latest.at)),c.map_or(0,|c|c.failures),changed,age(c.and_then(|c|c.admitted)),c.map_or(0,|c|u32::from(c.finished))]);
        let free=c.and_then(|c|c.first_free);let cap=owner.elf_capacity_turn_trace();
        row(6,"FREE US SLOTCAUSEMASK ADMIT EMPTY USED CAP",&[age(free.and_then(|f|f.at)),free.map_or(u32::MAX,|f|(u32::from(f.slot)<<16)|(u32::from(f.cause)<<8)|u32::from(f.empty_mask)),c.and_then(|c|c.admitted_slot).map_or(u32::MAX,u32::from),c.map_or(u32::MAX,|c|u32::from(c.admitted_empty)),u32::from(owner.elf_completion_turn_taken()),cap.flags]);
        for i in 0..8{for latest in [false,true]{let t=c.map(|c|if latest{c.latest.slots[i]}else{c.first.slots[i]});row(7+i*2+usize::from(latest),if latest{"LAST SLOT META SRC DST ID LENSENT TXNMSG"}else{"FIRST SLOT META SRC DST ID LENSENT TXNMSG"},&[t.map_or(u32::MAX,|t|(u32::from(t.kind)<<24)|(u32::from(t.flags)<<16)|u32::from(t.op)),t.map_or(u32::MAX,|t|t.source.port),t.map_or(u32::MAX,|t|t.destination.port),t.map_or(u32::MAX,|t|t.remote),t.map_or(u32::MAX,|t|(u32::from(t.len)<<16)|u32::from(t.sent)),t.map_or(u32::MAX,|t|(u32::from(t.transaction)<<16)|u32::from(t.message))]);}}
        let f=free.map(|f|f.original);
        row(23,"FREE ORIGINAL META SRC DST ID LENSENT QMI",&[f.map_or(u32::MAX,|t|(u32::from(t.kind)<<24)|(u32::from(t.flags)<<16)|u32::from(t.op)),f.map_or(u32::MAX,|t|t.source.port),f.map_or(u32::MAX,|t|t.destination.port),f.map_or(u32::MAX,|t|t.remote),f.map_or(u32::MAX,|t|(u32::from(t.len)<<16)|u32::from(t.sent)),f.map_or(u32::MAX,|t|(u32::from(t.transaction)<<16)|u32::from(t.message))]);
        row(24,"FREE NODES TYPE TX4 OWNER DONE CAP SLOT",&[f.map_or(u32::MAX,|t|t.source.node),f.map_or(u32::MAX,|t|t.destination.node),f.map_or(u32::MAX,|t|t.qrtr_type),age(t.and_then(|t|t.done)),cap.free_slot.map_or(u32::MAX,u32::from)]);
        row(25,"CAP ARM ENTER END RESULT ARMS FULL",&[age(cap.armed),age(cap.entered),age(cap.ended),u32::from(cap.result),cap.arms,cap.full_skips]);
        row(26,"WAITS NONE PEER TXCAP FLOW CREDIT CAP",&e.map_or([0;6],|e|e.waits_by_reason));
        let a=p.and_then(|p|p.publication);let b=w.target.and_then(|p|p.publication);
        row(27,"TX3 SNAP CALLS MASK TX4 SNAP CALLS MASK",&[p.map_or(0,|p|if p.publication_frozen{2}else{1}),a.map_or(u32::MAX,|p|p.calls),a.map_or(u32::MAX,|p|p.mask),w.target.map_or(0,|p|if p.publication_frozen{2}else{1}),b.map_or(u32::MAX,|p|p.calls),b.map_or(u32::MAX,|p|p.mask)]);
        let q=self.wlan_query.snapshot(owner);let pair=owner.efs_overlap_trace();
        row(28,"IPA PD WLAN WLFW ACK PAIR",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),(pair.flags&255)|((pair.admitted&255)<<8)|((pair.offers&65535)<<16)]);
/* STARTUP_PRESSURE_PANEL_BEGIN */
#[cfg(feature="sunfish-startup-pressure")]
        d.replace_line(1,"STARTUP POOL PRESSURE");
/* STARTUP_PRESSURE_PANEL_END */
/* READY_HANDOFF_BEGIN */
#[cfg(feature="sunfish-registry-ready-handoffs")]
        {let h=owner.ready_handoff_trace();row(26,"HANDOFF FLAGS ARMS RECEIPTS SERVICES RESULT",&[h.flags,h.arms,h.receipts,h.services,h.result]);d.replace_line(1,"READY HANDOFF POOL");}
/* READY_HANDOFF_END */
    }
}
