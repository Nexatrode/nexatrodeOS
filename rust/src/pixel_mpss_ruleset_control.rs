//! Read-only ruleset context, with immutable request/receipt joins.
use super::*;
impl RuntimeDomain {
 pub(super) fn show_ruleset_control_window(&self,d:&Display,owner:&ModemOwner){
  for n in 2..=28{d.replace_line(n,"");}d.replace_line(1,"RULESET CONTROL WINDOW");
  let age=|t|self.time_from_us(Some(self.runtime_started),t);
  let row=|n,label,v:&[u32]|{d.header_label(n,label);let mut f=[(0,8);6];for(i,v)in v.iter().enumerate(){f[i]=(*v,8);}d.header_row(n,true,&f[..v.len()]);};
  let t=owner.ruleset_control_trace();let early=owner.startup_prerequisites();
  row(2,"ERROR TOTAL OMIT RX TX FIN",&[self.error,t.total,t.omitted,t.rx,t.tx,u32::from(t.done_mask)]);
  for(n,index)in [1,6].iter().enumerate(){let r=early.reads[*index];let seq=r.and_then(|r|r.record_sequence());let q=t.requests[n].filter(|q|q.record.is_some()&&q.record==seq);let b=q.and_then(|q|q.bounds);let w=owner.ruleset_reply_fifo(n).filter(|w|Some(w.record)==seq);let f=w.map(|w|w.fifo);let c=f.and_then(|f|f.completion);
   row(3+n*2,if n==0{"R2 PEER SEQ LO HI INTENT REQ ABS VIS"}else{"R3 PEER SEQ LO HI INTENT REQ ABS VIS"},&[r.map_or(u32::MAX,|r|r.peer.port),seq.map_or(u32::MAX,|v|v as u32),seq.map_or(u32::MAX,|v|(v>>32)as u32),w.map_or(u32::MAX,|w|w.intent),age(b.and_then(|b|b.last_absent)),age(b.and_then(|b|b.first_visible))]);
   row(4+n*2,if n==0{"R2 ACCEPT PUB TAIL RXVIS DONE CLOSE"}else{"R3 ACCEPT PUB TAIL RXVIS DONE CLOSE"},&[age(r.and_then(|r|r.received)),age(r.and_then(|r|r.published)),age(f.and_then(|f|f.peer_tail_at)),age(c.and_then(|c|c.first_visible)),age(r.and_then(|r|r.done)),age(r.and_then(|r|r.close))]);
  }
  d.header_label(7,"US F CMD CH PARAM BYTES K A B TAG C D");
  for(index,h)in t.entries.iter().enumerate(){if let Some(h)=h{d.wire_row(index/4+8,index%4,&[(age(h.at),8),(u32::from(h.flags),1),(u32::from(h.command),4),(u32::from(h.channel),4),(h.parameter,8),(u32::from(h.bytes),4),(h.kind,2),(h.a,8),(h.b,8),(u32::from(h.tag),1),(h.c,8),(h.d,8)]);}}
  let clock=|index:usize,done:bool|age(early.reads[index].and_then(|r|if done{r.done}else{r.close}));
  row(24,"R1 DONE CLOSE EFS1 DONE CLOSE EFS2",&[clock(0,true),clock(0,false),clock(2,true),clock(2,false),clock(3,true),clock(3,false)]);
  let ipa=owner.linux_ipa_report();row(25,"IPA COMPLETE REG DRIVER PUB RESP PEND",&[u32::from(self.ipa_complete_seen),ipa.map_or(u32::MAX,|r|r.registration_requests),ipa.map_or(u32::MAX,|r|r.driver_requests),ipa.map_or(u32::MAX,|r|r.publications_committed),ipa.map_or(u32::MAX,|r|r.responses_accepted),ipa.and_then(|r|r.pending_message_id).map_or(u32::MAX,u32::from)]);
  let s=owner.service_registry_trace();let e=s.elf;let f=owner.elf_reply_fifo().map(|f|f.fifo);let c=f.and_then(|f|f.completion);let credit=s.elf_exchange.and_then(|e|e.host_credit);
  row(26,"ELF PUB TAIL VIS DONE MATCH CREDIT",&[age(e.and_then(|e|e.published)),age(f.and_then(|f|f.peer_tail_at)),age(c.and_then(|c|c.first_visible)),age(e.and_then(|e|e.done)),s.elf_exchange.map_or(u32::MAX,|e|u32::from(e.qmi_matches_reference)),credit.map_or(u32::MAX,|c|u32::from(c.requested)|(u32::from(c.remote_intent.is_some())<<1)|(u32::from(c.completed)<<2))]);
  let o=owner.original_tftp_write_trace();let(writes,file,_)=self.server_check.mcfg_progress();row(27,"ORIG ACK DONE CLOSE RULE N MCFG W FILE",&[age(o.and_then(|w|w.ack.done_at)),age(o.and_then(|w|w.timing.first_close_at)),early.total[0],writes,file.map_or(u32::MAX,|v|v as u32),t.unparsed]);
  let q=self.wlan_query.snapshot(owner);row(28,"PD WLAN WLFW RULE OMIT FIN UNPARSED",&[q[1],owner.tftp_wire_counts()[2],q[7],early.omitted[0],u32::from(t.done_mask),t.unparsed]);
 }
}
