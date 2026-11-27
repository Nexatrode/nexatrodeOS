//! Both delayed phases on one page, using existing frozen/retained observations.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_registry_elf_pressure(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"REGISTRY ELF PRESSURE");
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let span=|t|self.time_from_us(Some(0),t);
        let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
        let s=owner.service_registry_trace();let e=s.elf;let history=owner.registry_window_trace();let old=history.predecessor;let tx3=old.and_then(|p|p.publication);let tx4=s.txn4;
        let window=owner.elf_window_trace();let writer=window.map(|w|&w.writer);let elf=writer.and_then(|w|w.publication);
        row(2,"ERROR DETAIL CLOSE STAGE",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage]);
        row(3,"TX3 Q PUB CORE TX4 Q PUB OWNER",&[age(old.and_then(|p|p.queued_at)),age(old.and_then(|p|p.published_at)),age(old.and_then(|p|p.done_at)),age(tx4.and_then(|p|p.queued)),age(tx4.and_then(|p|p.published)),age(tx4.and_then(|p|p.done))]);
        row(4,"ELF RX Q PUB OWNER LOC Q DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done)),s.queued_names,s.completed_names]);
        row(5,"TX3 SNAP CALLS MASK ELF CALLS MASK FLAGS",&[old.map_or(0,|p|if p.publication_frozen{2}else{1}),tx3.map_or(u32::MAX,|p|p.calls),tx3.map_or(u32::MAX,|p|p.mask),elf.map_or(u32::MAX,|p|p.calls),elf.map_or(u32::MAX,|p|p.mask),writer.map_or(0,|w|u32::from(w.finished)|(u32::from(w.active)<<1)|(u32::from(w.regressed)<<2))]);
        for (base,label,p) in [(6,"TX3",tx3),(10,"ELF",elf)] {
            let t=p.map_or([u32::MAX;8],|p|p.turn_gates);let g=p.map_or([u32::MAX;12],|p|p.grant_gates);
            row(base,if label=="TX3"{"TX3 PRIORITY 0 1 2 3 4 5"}else{"ELF PRIORITY 0 1 2 3 4 5"},&t[..6]);
            row(base+1,if label=="TX3"{"TX3 PRIORITY 6 7 GRANT 0 1 2 3"}else{"ELF PRIORITY 6 7 GRANT 0 1 2 3"},&[t[6],t[7],g[0],g[1],g[2],g[3]]);
            row(base+2,if label=="TX3"{"TX3 GRANT 4 5 6 7 8 9"}else{"ELF GRANT 4 5 6 7 8 9"},&g[4..10]);
            row(base+3,if label=="TX3"{"TX3 GRANT A B ACK SENT BLOCK DONE ADS"}else{"ELF GRANT A B ACK SENT BLOCK DONE ADS"},&[g[10],g[11],p.map_or(u32::MAX,|p|p.ack.sent),p.map_or(u32::MAX,|p|p.ack.blocked),p.map_or(u32::MAX,|p|p.ack.done_ahead),p.map_or(u32::MAX,|p|p.ack.ads_ahead)]);
        }
        row(14,"ELF WAITS NONE PEER TXCAP FLOW CREDIT CAP",&e.map_or([0;6],|e|e.waits_by_reason));
        row(15,"RX END REG ENTRY SELECT Q PRECALLS OMIT",&[age(e.and_then(|e|e.dispatch.receive_pass_end)),age(e.and_then(|e|e.dispatch.registry_entered)),age(e.and_then(|e|e.dispatch.selected)),age(e.and_then(|e|e.queued)),e.map_or(0,|e|e.dispatch.turns.calls),e.map_or(0,|e|e.dispatch.turns.omitted)]);
        let fail=window.and_then(|w|w.last_failure);let a=fail.and_then(|f|f.entries[0]);let b=fail.and_then(|f|f.entries[1]);
        row(16,"LAST FAIL US SEQREASON N GATES A B",&[age(fail.and_then(|f|f.at)),fail.map_or(u32::MAX,|f|(f.sequence.min(0xffffff)<<8)|u32::from(f.reason)),fail.map_or(u32::MAX,|f|u32::from(f.count)),fail.map_or(u32::MAX,|f|u32::from(f.gates)),a.map_or(u32::MAX,|a|(u32::from(a.transaction)<<16)|(u32::from(a.flags)<<8)|u32::from(a.kind)),b.map_or(u32::MAX,|b|(u32::from(b.transaction)<<16)|(u32::from(b.flags)<<8)|u32::from(b.kind))]);
        row(17,"WRITER FIRST LAST PASSES OMIT US MAXGAP",&[age(writer.and_then(|w|w.first)),age(writer.and_then(|w|w.last)),writer.map_or(0,|w|w.passes),writer.map_or(0,|w|w.omitted),span(writer.and_then(|w|w.total)),span(writer.and_then(|w|w.max_gap))]);
        let turn=owner.elf_queued_turn_trace();let slots=owner.elf_tx_slots();let free=slots.and_then(|s|s.first_free);
        row(18,"QUEUED TURN ENTRY END FLAGS TX ACK FREE",&[age(turn.entry),age(turn.end),turn.flags,turn.published,age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),age(free.and_then(|f|f.at))]);
        let early=owner.startup_prerequisites();
        for i in 0..2{let r=early.reads[4+i];let seq=r.and_then(|r|r.record_sequence());
            row(19+i*2,"DIGEST PEER FLAGS RECORD LO HI OPCODE RESULT",&[r.map_or(u32::MAX,|r|r.peer.port),r.map_or(0,|r|r.flags),seq.map_or(u32::MAX,|s|s as u32),seq.map_or(u32::MAX,|s|(s>>32)as u32),r.map_or(u32::MAX,|r|(r.opcode.min(0xffff)<<16)|r.number.min(0xffff)),r.map_or(u32::MAX,|r|r.result)]);
            row(20+i*2,"DIGEST RX Q PUB DONE CLOSE PORTLEN",&[age(r.and_then(|r|r.received)),age(r.and_then(|r|r.queued)),age(r.and_then(|r|r.published)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close)),r.map_or(u32::MAX,|r|(r.local_port.min(0xffff)<<16)|r.reply_bytes.min(0xffff))]);
        }
        let handoff=owner.ready_handoff_trace();row(23,"HANDOFF FLAGS ARMS RX SERVICE RESULT SLOT",&[handoff.flags,handoff.arms,handoff.receipts,handoff.services,handoff.result,u32::from(owner.elf_completion_turn_taken())]);
        let retry=owner.elf_retry_trace();row(24,"RETRY RX RETURN REG ENTRY SELECT FLAGS BLOCKER",&[age(retry.receipt_at),age(retry.work_returned_at),age(retry.registry_entry_at),age(retry.registry_selected_at),retry.flags,retry.blocker_transaction.map_or(u32::MAX,u32::from)]);
        let cap=owner.elf_capacity_turn_trace();row(25,"FREE SLOTCAUSEMASK ADMIT SLOT US CAP FLAGS RESULT",&[free.map_or(u32::MAX,|f|(u32::from(f.slot)<<16)|(u32::from(f.cause)<<8)|u32::from(f.empty_mask)),slots.and_then(|s|s.admitted_slot).map_or(u32::MAX,u32::from),age(slots.and_then(|s|s.admitted)),cap.flags,u32::from(cap.result)]);
        let ipa=owner.linux_ipa_report();let flags=ipa.and_then(|r|r.protocol.map(|p|u32::from(p.modem_ready)|(u32::from(p.qmi_uc_ready)<<1)|(u32::from(p.indication_requested)<<2)|(u32::from(p.indication_sent)<<3)|(u32::from(r.protocol_complete)<<4)|(u32::from(r.failed)<<5)|(u32::from(r.pending_record)<<6)|(u32::from(r.record_queued)<<7)|(u32::from(r.held_response)<<8)|(u32::from(p.initial_boot)<<9)));
        let credit=s.elf_exchange.and_then(|e|e.host_credit);let reads=owner.read_sequence();row(26,"LIVE IPA PENDING CREDIT BODY ACK READ MASK",&[flags.unwrap_or(u32::MAX),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from),credit.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2)),s.elf_exchange.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),diagnostic::packed_counts(reads.total,reads.kinds)]);
        let close=owner.mcfg_close_trace();let pair=owner.efs_overlap_trace();let(writes,bytes,done)=self.server_check.mcfg_progress();row(27,"DIGEST N OMIT CLOSE OBS COMMIT EFS PAIR WRITES",&[early.digest_total,early.digest_omitted,close.observed,close.committed,(pair.flags&255)|((pair.admitted&255)<<8)|((pair.offers&65535)<<16),writes]);
        let q=self.wlan_query.snapshot(owner);row(28,"HIST IPA PD WLAN WLFW MCFG FILE READS",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],bytes.map_or(u32::MAX,|n|n as u32),done]);
    }
}
