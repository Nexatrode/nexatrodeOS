//! Four ruleset and two EFS lifecycles, including previously omitted ruleset reads.
use super::*;
impl RuntimeDomain {
 pub(super) fn show_startup_reply_coverage(&self,d:&Display,owner:&ModemOwner){
  for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"EARLY REPLY COVERAGE");
  let age=|t|self.time_from_us(Some(self.runtime_started),t);
  let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
  let s=owner.service_registry_trace();let e=s.elf;let a=owner.registry_window_trace().predecessor;let p=a.and_then(|a|a.publication);let b=s.txn4;
  let close=owner.mcfg_close_trace();row(2,"ERROR DETAIL CLOSE STAGE TX3 CALLS MASK",&[self.error,self.detail,owner.pending_mcfg_close().map_or(u32::MAX,|p|p.port),close.stage,p.map_or(u32::MAX,|p|p.calls),p.map_or(u32::MAX,|p|p.mask)]);
  let w=owner.original_tftp_write_trace();row(3,"ORIG PEER WRQ OACK DATA ACK PUB DONE",&[w.map_or(u32::MAX,|w|w.peer.port),age(w.and_then(|w|w.received_at)),age(w.and_then(|w|w.oack.published_at)),age(w.and_then(|w|w.data).and_then(|d|d.received_at)),age(w.and_then(|w|w.ack.published_at)),age(w.and_then(|w|w.ack.done_at))]);
  let ex=s.elf_exchange;let c=ex.and_then(|e|e.host_credit);row(4,"ORIG CLOSE ELF RX Q PUB OWNER CREDIT",&[age(w.and_then(|w|w.timing.first_close_at)),age(e.and_then(|e|e.received)),age(e.and_then(|e|e.queued)),age(e.and_then(|e|e.published)),age(e.and_then(|e|e.done)),c.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2))]);
  row(5,"TX3 Q PUB CORE TX4 Q PUB OWNER",&[age(a.and_then(|a|a.queued_at)),age(a.and_then(|a|a.published_at)),age(a.and_then(|a|a.done_at)),age(b.and_then(|b|b.queued)),age(b.and_then(|b|b.published)),age(b.and_then(|b|b.done))]);
  let early=owner.startup_prerequisites();row(6,"RULE N OMIT EFS N OMIT DIGEST N OMIT",&[early.total[0],early.omitted[0],early.total[1],early.omitted[1],early.digest_total,early.digest_omitted]);
  for (i,index) in [0,1,6,7,2,3].iter().enumerate(){let r=early.reads[*index];let seq=r.and_then(|r|r.record_sequence());
   let label=match i{0=>"RULE1 PEER FLAGS SEQ LO HI TMO PORT",1=>"RULE2 PEER FLAGS SEQ LO HI TMO PORT",2=>"RULE3 PEER FLAGS SEQ LO HI TMO PORT",3=>"RULE4 PEER FLAGS SEQ LO HI TMO PORT",4=>"EFS1 PEER FLAGS SEQ LO HI TMO PORT",_=>"EFS2 PEER FLAGS SEQ LO HI TMO PORT"};
   row(7+i*3,label,&[r.map_or(u32::MAX,|r|r.peer.port),r.map_or(0,|r|r.flags),seq.map_or(u32::MAX,|s|s as u32),seq.map_or(u32::MAX,|s|(s>>32)as u32),r.map_or(u32::MAX,|r|r.timeout_ms),r.map_or(u32::MAX,|r|r.local_port)]);
   row(8+i*3,"RX DISPATCH QUEUE PUB DONE CLOSE",&[age(r.and_then(|r|r.received)),age(r.and_then(|r|r.dispatched)),age(r.and_then(|r|r.queued)),age(r.and_then(|r|r.published)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close))]);
   row(9+i*3,"BYTES OPT FLAGS RSIZE OP CODE TRIES WAIT",&[r.map_or(u32::MAX,|r|r.request_bytes),r.map_or(0,|r|r.options_present|(r.options_flags<<16)),r.map_or(u32::MAX,|r|r.rsize),r.map_or(u32::MAX,|r|(r.opcode.min(0xffff)<<16)|r.number.min(0xffff)),r.map_or(0,|r|r.attempts),r.map_or(0,|r|diagnostic::packed_counts(r.waits[0],r.waits[1]))]);
  }
  let f=owner.elf_reply_fifo();let fifo=f.map(|f|f.fifo);let receipt=fifo.and_then(|f|f.completion);row(25,"ELF TAIL ABS VIS AHEAD CORE OWNER",&[age(fifo.and_then(|f|f.peer_tail_at)),age(receipt.and_then(|r|r.last_absent)),age(receipt.and_then(|r|r.first_visible)),receipt.and_then(|r|r.bytes_ahead).unwrap_or(u32::MAX),age(f.and_then(|f|f.core_done_at)),age(e.and_then(|e|e.done))]);
  let ipa=owner.linux_ipa_report();let flags=ipa.and_then(|r|r.protocol.map(|p|u32::from(p.modem_ready)|(u32::from(p.qmi_uc_ready)<<1)|(u32::from(p.indication_requested)<<2)|(u32::from(p.indication_sent)<<3)|(u32::from(r.protocol_complete)<<4)|(u32::from(r.failed)<<5)|(u32::from(r.pending_record)<<6)|(u32::from(r.record_queued)<<7)|(u32::from(r.held_response)<<8)|(u32::from(p.initial_boot)<<9)));
  row(26,"LIVE IPA REG DRIVER COMMITS RESP PENDING",&[flags.unwrap_or(u32::MAX),ipa.map_or(u32::MAX,|r|r.registration_requests),ipa.map_or(u32::MAX,|r|r.driver_requests),ipa.map_or(u32::MAX,|r|r.publications_committed),ipa.map_or(u32::MAX,|r|r.responses_accepted),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from)]);
  let(writes,file,reads)=self.server_check.mcfg_progress();let cap=owner.post_elf_mcfg();row(27,"MCFG WRITES FILE READS CLOSE OBS COMMIT OTHER",&[writes,file.map_or(u32::MAX,|n|n as u32),reads,close.observed,close.committed,cap.and_then(|c|c.other).map_or(u32::MAX,|o|o[0])]);
  let q=self.wlan_query.snapshot(owner);row(28,"HIST IPA PD WLAN WLFW ELF MATCH DONE",&[u32::from(self.ipa_complete_seen),q[1],owner.tftp_wire_counts()[2],q[7],ex.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),ex.map_or(u32::MAX,|e|u32::from(e.completed))]);
 }
}
