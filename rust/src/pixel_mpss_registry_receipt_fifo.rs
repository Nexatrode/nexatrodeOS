//! Exact RX_DONE visibility for the existing TX3 watch and first ELF reply.
/* RULESET_FIFO_BEGIN */
#[cfg(feature="sunfish-ruleset-reply-fifo")]
#[path="pixel_mpss_ruleset_reply_fifo.rs"]
mod ruleset_reply_fifo;
/* RULESET_FIFO_END */
/* STARTUP_COVERAGE_BEGIN */
#[cfg(feature="sunfish-startup-reply-coverage")]
#[path="pixel_mpss_startup_reply_coverage.rs"]
mod startup_reply_coverage;
/* STARTUP_COVERAGE_END */
use super::*;
/* FIRST_MCFG_BEGIN */
#[cfg(feature="sunfish-first-mcfg-response")]
#[path="pixel_mpss_first_mcfg_response.rs"]
mod first_mcfg_response;
/* FIRST_MCFG_END */
/* POOL_WIRE_PANEL_BEGIN */
#[cfg(feature="sunfish-elf-pool-wire-panel")]
#[path="pixel_mpss_elf_pool_wire.rs"]
mod elf_pool_wire;
/* POOL_WIRE_PANEL_END */
impl RuntimeDomain {
    pub(super) fn show_registry_receipt_fifo(&self,d:&Display,owner:&ModemOwner){
/* RULESET_FIFO_BEGIN */
        #[cfg(feature="sunfish-ruleset-reply-fifo")]
        {self.show_ruleset_reply_fifo(d,owner);return;}
/* RULESET_FIFO_END */
/* FIRST_MCFG_BEGIN */
/* STARTUP_COVERAGE_BEGIN */
        #[cfg(feature="sunfish-startup-reply-coverage")]
        {self.show_startup_reply_coverage(d,owner);return;}
/* STARTUP_COVERAGE_END */
        #[cfg(feature="sunfish-first-mcfg-response")]
        {self.show_first_mcfg_response(d,owner);return;}
/* FIRST_MCFG_END */
/* POOL_WIRE_PANEL_BEGIN */
        #[cfg(feature="sunfish-elf-pool-wire-panel")]
        {self.show_elf_pool_wire(d,owner);return;}
/* POOL_WIRE_PANEL_END */
        for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"REGISTRY RECEIPT FIFO");
/* TX3_RETURN_BEGIN */
        #[cfg(feature="sunfish-tx3-own-return-turn")]
        d.replace_line(1,"TX3 OWN RETURN TURN");
/* TX3_CREDIT_BEGIN */
        #[cfg(feature="sunfish-tx3-credit-return-turn")]
        d.replace_line(1,"TX3 CREDIT RETURN");
/* TX3_CREDIT_END */
/* TX3_RETURN_END */
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let span=|t|self.time_from_us(Some(0),t);
        let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
        let s=owner.service_registry_trace();let e=s.elf;let history=owner.registry_window_trace();let old=history.predecessor;let tx3=old.and_then(|p|p.publication);let tx4=s.txn4;
        let window=owner.elf_window_trace();let writer=window.map(|w|&w.writer);let elf=writer.and_then(|w|w.publication);
        row(2,"ERROR DETAIL CLOSE STAGE",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage]);
        row(3,"TX3 Q PUB CORE TX4 Q PUB OWNER",&[age(old.and_then(|p|p.queued_at)),age(old.and_then(|p|p.published_at)),age(old.and_then(|p|p.done_at)),age(tx4.and_then(|p|p.queued)),age(tx4.and_then(|p|p.published)),age(tx4.and_then(|p|p.done))]);
        row(4,"ELF RX Q PUB OWNER LOC Q DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done)),s.queued_names,s.completed_names]);
        row(5,"TX3 SNAP CALLS MASK ELF CALLS MASK FLAGS",&[old.map_or(0,|p|if p.publication_frozen{2}else{1}),tx3.map_or(u32::MAX,|p|p.calls),tx3.map_or(u32::MAX,|p|p.mask),elf.map_or(u32::MAX,|p|p.calls),elf.map_or(u32::MAX,|p|p.mask),writer.map_or(0,|w|u32::from(w.finished)|(u32::from(w.active)<<1)|(u32::from(w.regressed)<<2))]);
        let elf_fifo=owner.elf_reply_fifo();
        for (n,label,f) in [(6,"TX3 TAIL ABS VIS AHEAD FINISHED EVICT",owner.registry_reply_fifo()),(7,"ELF TAIL ABS VIS AHEAD FINISHED EVICT",elf_fifo.map(|w|w.fifo))] {
            let receipt=f.and_then(|f|f.completion);
            row(n,label,&[age(f.and_then(|f|f.peer_tail_at)),age(receipt.and_then(|r|r.last_absent)),age(receipt.and_then(|r|r.first_visible)),receipt.and_then(|r|r.bytes_ahead).unwrap_or(u32::MAX),f.map_or(0,|f|u32::from(f.finished)),f.map_or(u32::MAX,|f|f.history_evictions)]);
        }
        row(8,"ELF ARM UNREAD INTENT CORE OWNER",&[age(elf_fifo.and_then(|w|w.armed_at)),elf_fifo.map_or(u32::MAX,|w|w.unread_at_arm),elf_fifo.map_or(u32::MAX,|w|w.remote_intent),age(elf_fifo.and_then(|w|w.core_done_at)),age(e.and_then(|e|e.done))]);
        let grant=old.and_then(|p|p.grant);
/* TX3_RETURN_BEGIN */
        #[cfg(feature="sunfish-tx3-own-return-turn")]
        {let g=tx3.map_or([u32::MAX;8],|p|p.own_return_gates);row(9,"OWN RETURN GATES 0 1 2 3",&g[..4]);row(25,"OWN RETURN GATES 4 5 6 7",&g[4..]);}
/* TX3_RETURN_END */
/* TX3_RETURN_BEGIN */
        #[cfg(not(feature="sunfish-tx3-own-return-turn"))]
/* TX3_RETURN_END */
        row(9,"TX3 FIRST CONVERT AD ACK INTENT",&[age(grant.and_then(|g|g.converted_at)),age(grant.and_then(|g|g.advertised_at)),age(grant.and_then(|g|g.ack_at)),grant.map_or(u32::MAX,|g|g.intent)]);
        for (base,label,p) in [(10,"TX3",tx3),(14,"ELF",elf)] {
            let t=p.map_or([u32::MAX;8],|p|p.turn_gates);let g=p.map_or([u32::MAX;12],|p|p.grant_gates);
            row(base,if label=="TX3"{"TX3 PRIORITY 0 1 2 3 4 5"}else{"ELF PRIORITY 0 1 2 3 4 5"},&t[..6]);
            row(base+1,if label=="TX3"{"TX3 PRIORITY 6 7 GRANT 0 1 2 3"}else{"ELF PRIORITY 6 7 GRANT 0 1 2 3"},&[t[6],t[7],g[0],g[1],g[2],g[3]]);
            row(base+2,if label=="TX3"{"TX3 GRANT 4 5 6 7 8 9"}else{"ELF GRANT 4 5 6 7 8 9"},&g[4..10]);
            row(base+3,if label=="TX3"{"TX3 GRANT A B ACK SENT BLOCK DONE ADS"}else{"ELF GRANT A B ACK SENT BLOCK DONE ADS"},&[g[10],g[11],p.map_or(u32::MAX,|p|p.ack.sent),p.map_or(u32::MAX,|p|p.ack.blocked),p.map_or(u32::MAX,|p|p.ack.done_ahead),p.map_or(u32::MAX,|p|p.ack.ads_ahead)]);
        }
        row(18,"ELF WAITS NONE PEER TXCAP FLOW CREDIT CAP",&e.map_or([0;6],|e|e.waits_by_reason));
        row(19,"WRITER FIRST LAST PASSES OMIT US MAXGAP",&[age(writer.and_then(|w|w.first)),age(writer.and_then(|w|w.last)),writer.map_or(0,|w|w.passes),writer.map_or(0,|w|w.omitted),span(writer.and_then(|w|w.total)),span(writer.and_then(|w|w.max_gap))]);
        row(20,"RX END REG ENTRY SELECT Q PRECALLS OMIT",&[age(e.and_then(|e|e.dispatch.receive_pass_end)),age(e.and_then(|e|e.dispatch.registry_entered)),age(e.and_then(|e|e.dispatch.selected)),age(e.and_then(|e|e.queued)),e.map_or(0,|e|e.dispatch.turns.calls),e.map_or(0,|e|e.dispatch.turns.omitted)]);
        let turn=owner.elf_queued_turn_trace();let free=owner.elf_tx_slots().and_then(|s|s.first_free);
        row(21,"QUEUED TURN ENTRY END FLAGS TX ACK FREE",&[age(turn.entry),age(turn.end),turn.flags,turn.published,age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),age(free.and_then(|f|f.at))]);
        let early=owner.startup_prerequisites();
        for i in 0..2{let r=early.reads[4+i];
            row(22+i,"DIGEST PEER FLAGS PUB DONE CLOSE PORTLEN",&[r.map_or(u32::MAX,|r|r.peer.port),r.map_or(0,|r|r.flags),age(r.and_then(|r|r.published)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close)),r.map_or(u32::MAX,|r|(r.local_port.min(0xffff)<<16)|r.reply_bytes.min(0xffff))]);
        }
        let seq=|i:usize|early.reads[i].and_then(|r|r.record_sequence());
        row(24,"DIGEST RECORD1 LO HI RECORD2 LO HI N OMIT",&[seq(4).map_or(u32::MAX,|s|s as u32),seq(4).map_or(u32::MAX,|s|(s>>32)as u32),seq(5).map_or(u32::MAX,|s|s as u32),seq(5).map_or(u32::MAX,|s|(s>>32)as u32),early.digest_total,early.digest_omitted]);
        let h=owner.ready_handoff_trace();let close=owner.mcfg_close_trace();
/* TX3_RETURN_BEGIN */
        #[cfg(not(feature="sunfish-tx3-own-return-turn"))]
/* TX3_RETURN_END */
        row(25,"HANDOFF FLAGS ARMS RX RESULT CLOSE OBS COMMIT",&[h.flags,h.arms,h.receipts,h.result,close.observed,close.committed]);
        let ipa=owner.linux_ipa_report();let flags=ipa.and_then(|r|r.protocol.map(|p|u32::from(p.modem_ready)|(u32::from(p.qmi_uc_ready)<<1)|(u32::from(p.indication_requested)<<2)|(u32::from(p.indication_sent)<<3)|(u32::from(r.protocol_complete)<<4)|(u32::from(r.failed)<<5)|(u32::from(r.pending_record)<<6)|(u32::from(r.record_queued)<<7)|(u32::from(r.held_response)<<8)|(u32::from(p.initial_boot)<<9)));
        let credit=s.elf_exchange.and_then(|e|e.host_credit);let reads=owner.read_sequence();
        row(26,"LIVE IPA PENDING CREDIT BODY ACK READ MASK",&[flags.unwrap_or(u32::MAX),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),credit.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2)),s.elf_exchange.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),diagnostic::packed_counts(reads.total,reads.kinds)]);
        let fail=window.and_then(|w|w.last_failure);let a=fail.and_then(|f|f.entries[0]);let b=fail.and_then(|f|f.entries[1]);
        row(27,"LAST FAIL US SEQREASON N GATES A B",&[age(fail.and_then(|f|f.at)),fail.map_or(u32::MAX,|f|(f.sequence.min(0xffffff)<<8)|u32::from(f.reason)),fail.map_or(u32::MAX,|f|u32::from(f.count)),fail.map_or(u32::MAX,|f|u32::from(f.gates)),a.map_or(u32::MAX,|a|(u32::from(a.transaction)<<16)|(u32::from(a.flags)<<8)|u32::from(a.kind)),b.map_or(u32::MAX,|b|(u32::from(b.transaction)<<16)|(u32::from(b.flags)<<8)|u32::from(b.kind))]);
        let(_,bytes,done)=self.server_check.mcfg_progress();let q=self.wlan_query.snapshot(owner);
        row(28,"HIST IPA PD WLAN WLFW MCFG FILE READS",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],bytes.map_or(u32::MAX,|n|n as u32),done]);
    }
}
