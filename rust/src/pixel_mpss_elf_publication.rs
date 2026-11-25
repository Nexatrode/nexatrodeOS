//! Frozen exact-ELF writer choices, per-call costs and gaps between calls.
use super::*;
impl RuntimeDomain {
 pub(super) fn show_tftp_completion(&self,d:&Display,owner:&ModemOwner){
  for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,if cfg!(feature="sunfish-elf-third-reply"){"ELF THIRD REPLY WINDOW"}else{"ELF WRITER PUBLICATION"});d.hex(2,"RUNTIME ERROR",self.error);
  let s=owner.service_registry_trace();let e=s.elf;let w=owner.elf_window_trace();let p=w.map(|w|&w.writer);let a=p.and_then(|p|p.publication);
  let age=|t|self.time_from_us(Some(self.runtime_started),t);let span=|t|self.time_from_us(Some(0),t);
  let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);4];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
  row(3,"ELF RX QUEUE PUB DONE",&[age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done))]);
  row(4,if cfg!(feature="sunfish-elf-third-reply"){"LIMIT ADMIT SEQ N RX"}else{"SLOT TURN ADMIT SEQ N RX"},&[if cfg!(feature="sunfish-elf-third-reply"){3}else{u32::from(owner.elf_completion_turn_taken())},w.and_then(|w|w.admitted).map_or(u32::MAX,|a|a.sequence),w.and_then(|w|w.admitted).map_or(u32::MAX,|a|u32::from(a.count)),w.and_then(|w|w.last_before_admission).map_or(u32::MAX,u32::from)]);
  let t=owner.elf_queued_turn_trace();row(5,"QUEUED TURN ENTER END FLAGS",&[age(t.entry),age(t.end),t.flags]);
  row(6,"WRITER FIRST LAST FINISHED",&[age(p.and_then(|p|p.first)),age(p.and_then(|p|p.last)),p.map_or(0,|p|u32::from(p.finished))]);
  row(7,"PASSES TIMED OMIT ACTIVE",&[p.map_or(0,|p|p.passes),p.map_or(0,|p|p.timed),p.map_or(0,|p|p.omitted),p.map_or(0,|p|u32::from(p.active))]);
  row(8,"PASS US MAX GAP US BAD CLOCK",&[span(p.and_then(|p|p.total)),span(p.and_then(|p|p.max_gap)),p.map_or(0,|p|u32::from(p.regressed))]);
  row(9,"PUB CALLS MASK TXN BYTES",&[a.map_or(0,|a|a.calls),a.map_or(0,|a|a.mask),a.map_or(u32::MAX,|a|u32::from(a.transaction)),a.map_or(u32::MAX,|a|u32::from(a.bytes))]);
  let turns=a.map_or([0;8],|a|a.turn_gates);row(10,"TURN GATES 0 1 2 3",&turns[..4]);row(11,"TURN GATES 4 5 6 7",&turns[4..]);
  let grants=a.map_or([0;12],|a|a.grant_gates);row(12,"GRANT GATES 0 1 2 3",&grants[..4]);row(13,"GRANT GATES 4 5 6 7",&grants[4..8]);row(14,"GRANT GATES 8 9 A B",&grants[8..]);
  row(15,"ACK SENT BLOCKED DONE ADS",&a.map_or([0;4],|a|[a.ack.sent,a.ack.blocked,a.ack.done_ahead,a.ack.ads_ahead]));
  row(16,"ACK AGE FIRST GAP LAST GAP",&[span(a.and_then(|a|a.ack.max_age)),self.time_from_us(e.and_then(|e|e.queued),p.and_then(|p|p.first)),self.time_from_us(p.and_then(|p|p.last),e.and_then(|e|e.published))]);
  for i in 0..8{let r=p.and_then(|p|p.records[i]);row(17+i,"START END CMDKFTXN PARAM",&[age(r.and_then(|r|r.start)),age(r.and_then(|r|r.end)),r.map_or(u32::MAX,|r|(u32::from(r.command.min(255))<<24)|(u32::from(r.kind.min(15))<<20)|(u32::from(r.flags&15)<<16)|u32::from(r.transaction)),r.map_or(u32::MAX,|r|r.parameter)]);}
  row(25,"RECEIPT3 ADMIT RECEIPT4",&[age(w.and_then(|w|w.receipts.iter().flatten().find(|r|r.transaction==3)).and_then(|r|r.at)),age(w.and_then(|w|w.admitted).and_then(|a|a.at)),age(w.and_then(|w|w.receipts.iter().flatten().find(|r|r.transaction==4)).and_then(|r|r.at))]);
  let m=owner.post_elf_mcfg().and_then(|c|c.mcfg.as_ref());row(26,"MCFG PEER STAGE RESULT OP",&[m.map_or(u32::MAX,|m|m.peer.port),m.map_or(0,|m|m.flags),m.and_then(|m|m.server_result).unwrap_or(u32::MAX),m.map_or(u32::MAX,|m|m.reply_opcode)]);
  let q=self.wlan_query.snapshot(owner);row(27,"IPA PD STATE WLAN WLFW",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7]]);
  let(writes,bytes,reads)=self.server_check.mcfg_progress();row(28,"ORIG ACK MCFG W FILE READ",&[age(owner.original_tftp_write_trace().and_then(|w|w.ack.done_at)),writes,bytes.map_or(u32::MAX,|n|n as u32),reads]);
 }
}
