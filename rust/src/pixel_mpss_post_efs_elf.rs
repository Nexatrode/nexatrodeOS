//! Read-only projection of existing post-EFS/ELF writer and startup records.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_post_efs_elf(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}
        d.replace_line(1,"POST EFS ELF WRITER");
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let span=|t|self.time_from_us(Some(0),t);
        let row=|n,label,values:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in values.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..values.len()]);};
        let s=owner.service_registry_trace();let e=s.elf;
        let window=owner.elf_window_trace();let writer=window.map(|w|&w.writer);let p=writer.and_then(|w|w.publication);
        let reads=owner.startup_prerequisites();let first=reads.reads[2];let second=reads.reads[3];
        let pair=owner.efs_overlap_trace();let packed_pair=(pair.flags&255)|((pair.admitted&255)<<8)|((pair.offers&65535)<<16);
        row(2,"ERROR DETAIL CLOSE STAGE",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),owner.mcfg_close_trace().stage]);
        row(3,"ELF RX QUEUE PUB DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
        row(4,"EFS1 PUB DONE CLOSE FLAGS",&[age(first.and_then(|r|r.published)),age(first.and_then(|r|r.done)),age(first.and_then(|r|r.close)),first.map_or(0,|r|r.flags)]);
        row(5,"EFS2 PUB DONE CLOSE PAIR",&[age(second.and_then(|r|r.published)),age(second.and_then(|r|r.done)),age(second.and_then(|r|r.close)),packed_pair]);
        let ipa=owner.linux_ipa_report();let flags=ipa.and_then(|r|r.protocol.map(|p|u32::from(p.modem_ready)|(u32::from(p.qmi_uc_ready)<<1)|(u32::from(p.indication_requested)<<2)|(u32::from(p.indication_sent)<<3)|(u32::from(r.protocol_complete)<<4)|(u32::from(r.failed)<<5)|(u32::from(r.pending_record)<<6)|(u32::from(r.record_queued)<<7)|(u32::from(r.held_response)<<8)|(u32::from(p.initial_boot)<<9)));
        let exchange=s.elf_exchange;let credit=exchange.and_then(|e|e.host_credit);
        row(6,"BODY CREDIT IPA FLAGS PENDING",&[exchange.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),credit.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2)),flags.unwrap_or(u32::MAX),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from)]);
        let post=exchange.and_then(|e|e.first_tftp_request);
        row(7,"POST PATH OP PEER BYTES",&[post.map_or(u32::MAX,|p|match p.path_kind{"SERVER CHECK"=>1,"MCFG TMP"=>2,"OTA FIREWALL RULESET"=>3,"MODEM EFS"=>4,"WLANMDSP MBN"=>5,"OTHER PATH"=>6,_=>7}),post.map_or(u32::MAX,|p|u32::from(p.opcode)),post.map_or(u32::MAX,|p|p.source_port),post.map_or(u32::MAX,|p|p.body_bytes)]);
        let turn=owner.elf_queued_turn_trace();
        row(8,"QUEUED TURN ENTER END FLAGS TX",&[age(turn.entry),age(turn.end),turn.flags,turn.published]);
        row(9,"WRITER FIRST LAST PASSES OMIT",&[age(writer.and_then(|w|w.first)),age(writer.and_then(|w|w.last)),writer.map_or(0,|w|w.passes),writer.map_or(0,|w|w.omitted)]);
        row(10,"WRITER US GAP TIMED FLAGS",&[span(writer.and_then(|w|w.total)),span(writer.and_then(|w|w.max_gap)),writer.map_or(0,|w|w.timed),writer.map_or(0,|w|u32::from(w.finished)|(u32::from(w.active)<<1)|(u32::from(w.regressed)<<2))]);
        row(11,"PUB CALLS MASK TXN BYTES",&[p.map_or(0,|p|p.calls),p.map_or(0,|p|p.mask),p.map_or(u32::MAX,|p|u32::from(p.transaction)),p.map_or(u32::MAX,|p|u32::from(p.bytes))]);
        let turns=p.map_or([0;8],|p|p.turn_gates);row(12,"TURN GATES 0 1 2 3",&turns[..4]);row(13,"TURN GATES 4 5 6 7",&turns[4..]);
        let grants=p.map_or([0;12],|p|p.grant_gates);row(14,"GRANT GATES 0 1 2 3",&grants[..4]);row(15,"GRANT GATES 4 5 6 7",&grants[4..8]);row(16,"GRANT GATES 8 9 A B",&grants[8..]);
        for i in 0..8{let r=writer.and_then(|w|w.records[i]);row(17+i,"START END CMDKFTXN PARAM RESULT",&[age(r.and_then(|r|r.start)),age(r.and_then(|r|r.end)),r.map_or(u32::MAX,|r|(u32::from(r.command.min(255))<<24)|(u32::from(r.kind.min(15))<<20)|(u32::from(r.flags&15)<<16)|u32::from(r.transaction)),r.map_or(u32::MAX,|r|r.parameter),r.map_or(u32::MAX,|r|u32::from(r.result))]);}
        row(25,"ACK SENT BLOCKED DONE ADS AGE",&[p.map_or(0,|p|p.ack.sent),p.map_or(0,|p|p.ack.blocked),p.map_or(0,|p|p.ack.done_ahead),p.map_or(0,|p|p.ack.ads_ahead),span(p.and_then(|p|p.ack.max_age))]);
        let m=owner.post_elf_mcfg().and_then(|c|c.mcfg.as_ref());
        row(26,"MCFG PEER FLAGS FILE RESULT CLIENT CLOSE",&[m.map_or(u32::MAX,|m|m.peer.port),m.map_or(0,|m|m.flags),m.and_then(|m|m.file_before).unwrap_or(u32::MAX),m.and_then(|m|m.server_result).unwrap_or(u32::MAX),m.and_then(|m|m.last_client).unwrap_or(u32::MAX),age(m.and_then(|m|m.close_at))]);
        let q=self.wlan_query.snapshot(owner);row(27,"IPA PD STATE WLAN WLFW",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7]]);
        let inventory=owner.read_sequence();row(28,"ORIG ACK READ MASK LOC Q DONE",&[age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),diagnostic::packed_counts(inventory.total,inventory.kinds),s.queued_names,s.completed_names]);
    }
}
