//! Both same-peer owners at exact ELF admission checks, with matched retirement.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self,d:&Display,owner:&ModemOwner){
        for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,if cfg!(feature="sunfish-elf-first-slot-turn"){"ELF FIRST SLOT TURN"}else{"ELF WINDOW RETIREMENT"});d.hex(2,"RUNTIME ERROR",self.error);
        let s=owner.service_registry_trace();let e=s.elf;let w=owner.elf_window_trace();let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);4];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
        row(3,"ELF RX QUEUE PUB DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
        let turn=owner.elf_queued_turn_trace();row(4,"TURN ARM ENTER END FLAGS",&[age(turn.armed_at),age(turn.entry),age(turn.end),turn.flags]);
        #[cfg(feature="sunfish-elf-first-slot-turn")]
        row(5,"TRIES RECEIPTS OMIT SLOT TURN",&[w.map_or(0,|w|w.attempts),w.map_or(0,|w|w.receipt_count),w.map_or(0,|w|w.receipt_omitted),u32::from(owner.elf_completion_turn_taken())]);
        #[cfg(not(feature="sunfish-elf-first-slot-turn"))]
        row(5,"ADMISSION TRIES RECEIPTS END",&[w.map_or(0,|w|w.attempts),w.map_or(0,|w|w.receipt_count),w.map_or(0,|w|w.receipt_omitted),w.map_or(0,|w|u32::from(w.finished))]);
        let prior=owner.registry_window_trace().predecessor;let fifo=owner.registry_reply_fifo();let rx=fifo.and_then(|f|f.completion);
        row(6,"TX3 QUEUE PUB TAIL DONE",&[age(prior.and_then(|p|p.queued_at)),age(prior.and_then(|p|p.published_at)),age(fifo.and_then(|f|f.peer_tail_at)),age(prior.and_then(|p|p.done_at))]);
        row(7,"TX3 ABS VIS AHEAD END LOST",&[age(rx.and_then(|r|r.last_absent)),age(rx.and_then(|r|r.first_visible)),rx.and_then(|r|r.bytes_ahead).unwrap_or(u32::MAX),fifo.map_or(u32::MAX,|f|(u32::from(f.finished)<<16)|f.history_evictions.min(0xffff))]);
        row(8,"TX4 RX QUEUE PUB DONE",&[age(s.txn4.and_then(|t|t.received)),age(s.txn4.and_then(|t|t.queued)),age(s.txn4.and_then(|t|t.published)),age(s.txn4.and_then(|t|t.done))]);
        row(9,"RX END REG ENTER SELECT",&[age(e.and_then(|e|e.dispatch.receive_pass_end)),age(e.and_then(|e|e.dispatch.registry_entered)),age(e.and_then(|e|e.dispatch.selected))]);
        let delta=|a,b|self.time_from_us(a,b);
        row(10,"PHASE US RX REG SELECT Q",&[delta(e.and_then(|e|e.received),e.and_then(|e|e.dispatch.receive_pass_end)),delta(e.and_then(|e|e.dispatch.receive_pass_end),e.and_then(|e|e.dispatch.registry_entered)),delta(e.and_then(|e|e.dispatch.registry_entered),e.and_then(|e|e.dispatch.selected)),delta(e.and_then(|e|e.dispatch.selected),e.and_then(|e|e.queued))]);
        for(n,label,a)in[(11,"FIRST FAIL US RS N GATES",w.and_then(|w|w.first_failure)),(14,"LAST FAIL US RS N GATES",w.and_then(|w|w.last_failure))]{
            row(n,label,&[age(a.and_then(|a|a.at)),a.map_or(u32::MAX,|a|(a.sequence.min(0xffffff)<<8)|u32::from(a.reason)),a.map_or(u32::MAX,|a|u32::from(a.count)),a.map_or(u32::MAX,|a|u32::from(a.gates))]);
            for i in 0..2{let v=a.and_then(|a|a.entries[i]);row(n+1+i,if i==0{"OWNER A TXN FLAGS ID KIND"}else{"OWNER B TXN FLAGS ID KIND"},&[v.map_or(u32::MAX,|v|u32::from(v.transaction)),v.map_or(u32::MAX,|v|u32::from(v.flags)),v.map_or(u32::MAX,|v|v.remote_intent),v.map_or(u32::MAX,|v|u32::from(v.kind))]);}
        }
        let admitted=w.and_then(|w|w.admitted);row(17,"ADMIT US N GATES LAST RX",&[age(admitted.and_then(|a|a.at)),admitted.map_or(u32::MAX,|a|u32::from(a.count)),admitted.map_or(u32::MAX,|a|u32::from(a.gates)),w.and_then(|w|w.last_before_admission).map_or(u32::MAX,u32::from)]);
        for i in 0..4{let r=w.and_then(|w|w.receipts[i]);row(18+i,"RETIRE US TXN ID STATE",&[age(r.and_then(|r|r.at)),r.map_or(u32::MAX,|r|u32::from(r.transaction)),r.map_or(u32::MAX,|r|r.remote_intent),r.map_or(u32::MAX,|r|(r.sequence.min(0xffff)<<16)|(u32::from(r.target)<<9)|(u32::from(r.matched_last_failure)<<8)|u32::from(r.remaining))]);}
        row(22,"RECEIPTS OMIT ADMIT SEQ FLAGS",&[w.map_or(0,|w|w.receipt_count),w.map_or(0,|w|w.receipt_omitted),admitted.map_or(u32::MAX,|a|a.sequence),w.map_or(0,|w|w.flags)]);
        let m=owner.post_elf_mcfg().and_then(|c|c.mcfg.as_ref());row(23,"MCFG PEER STAGE RESULT MASK",&[m.map_or(u32::MAX,|m|m.peer.port),m.map_or(0,|m|m.flags),m.and_then(|m|m.server_result).unwrap_or(u32::MAX),m.map_or(0,|m|m.options.present)]);
        let opts=m.map_or([u32::MAX;7],|m|m.options.values);row(24,"MCFG BLK TIME WIN TSIZE",&opts[..4]);row(25,"MCFG RSIZE SEEK UNLINK FLAGS",&[opts[4],opts[5],opts[6],m.map_or(0,|m|m.options.flags)]);
        row(26,"MCFG FILE OP ERR LEN",&[m.and_then(|m|m.file_before).unwrap_or(u32::MAX),m.map_or(u32::MAX,|m|m.reply_opcode),m.filter(|m|m.reply_opcode==5).map_or(u32::MAX,|m|m.reply_number),m.map_or(0,|m|m.reply_bytes)]);
        let q=self.wlan_query.snapshot(owner);row(27,"IPA PD STATE WLAN WLFW",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7]]);
        let(writes,bytes,reads)=self.server_check.mcfg_progress();row(28,"ORIG ACK MCFG W FILE READ",&[age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),writes,bytes.map_or(u32::MAX,|n|n as u32),reads]);
    }
}
