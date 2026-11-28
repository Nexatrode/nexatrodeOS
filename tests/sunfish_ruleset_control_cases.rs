// Included after the actual FIFO/Owner ruleset fixtures, with parent and child features.
#[test]
fn ruleset_control_wire_and_complete_io_match_parent_with_grant_credit_and_close(){
 let(p,mut o,mut i,a,b)=rf_pending(false);
 let id=mc_offer(&p,&mut o,&mut i);
 p.push(&data(id,&owner_qrtr_packet(1,0x25,0x5000,true,STAT)));o.notified(8,&mut i).unwrap();let credit=p.take_tx();
 p.push(&control(7,2,108));o.notified(8,&mut i).unwrap();let grant=p.take_tx();
 mc_close(&p,&mut o,&mut i,0x21);let close=p.take_tx();
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&a);acknowledge_outgoing_glink(&p,&mut o,&mut i,&b);
 #[cfg(feature="sunfish-ruleset-control-window")]{let t=o.ruleset_control_trace();assert_eq!(t.done_mask,3);assert!(t.total>10&&t.rx>0&&t.tx>0);assert_eq!(t.unparsed,0);
  assert!(t.entries.iter().flatten().any(|e|e.flags&1==0&&e.command==7&&e.parameter==108));
  assert!(t.entries.iter().flatten().any(|e|e.flags&1!=0&&e.command==8&&e.parameter==1));
  assert!(t.entries.iter().flatten().any(|e|e.flags&1!=0&&e.command==9&&e.kind==7));
  assert!(t.entries.iter().flatten().any(|e|e.tag==0xd&&e.d==0x21));
  for(n,index)in [1,6].iter().enumerate(){assert_eq!(t.requests[n].unwrap().record,o.startup_prerequisites().reads[*index].unwrap().record_sequence());}
  let n=t.total;mc_close(&p,&mut o,&mut i,0x26);assert_eq!(o.ruleset_control_trace().total,n);
 }
 #[cfg(not(feature="sunfish-ruleset-control-window"))]mc_close(&p,&mut o,&mut i,0x26);
 assert!(o.error().is_none());print!("RULESET_CONTROL_WIRE ");for w in [&a,&b,&credit,&grant,&close]{for c in w{print!("{c:02x}");}}println!();println!("RULESET_CONTROL_IO {:?}",p.log());
}
#[cfg(feature="sunfish-ruleset-control-window")]
#[test]
fn ruleset_control_request_arrival_bounds_are_original_and_clockless_stays_unknown(){
 for clocked in [false,true]{let(p,mut o,mut i,a,b)=rf_pending(clocked);
  for(n,index)in [1,6].iter().enumerate(){let q=o.ruleset_control_trace().requests[n].unwrap();assert_eq!(q.record,o.startup_prerequisites().reads[*index].unwrap().record_sequence());if clocked{assert!(q.bounds.unwrap().first_visible.is_some());}else{assert!(q.bounds.is_none()||q.bounds.unwrap().first_visible.is_none());}}
  let q=o.ruleset_control_trace().requests;mc_close(&p,&mut o,&mut i,0x24);assert_eq!(o.ruleset_control_trace().requests,q);
  acknowledge_outgoing_glink(&p,&mut o,&mut i,&b);assert_eq!(o.ruleset_control_trace().done_mask,2);acknowledge_outgoing_glink(&p,&mut o,&mut i,&a);assert_eq!(o.ruleset_control_trace().done_mask,3);
 }
}
#[cfg(feature="sunfish-ruleset-control-window")]
#[test]
fn ruleset_control_fault_cannot_append_accepted_or_committed_records(){
 let(p,mut o,mut i,_,_)=rf_pending(true);let t=o.ruleset_control_trace();let before=(t.total,t.rx,t.tx,t.done_mask,t.requests);
 p.0.borrow_mut().observation.ready=false;p.push(&control(7,2,108));assert!(o.notified(8,&mut i).is_err());let t=o.ruleset_control_trace();assert_eq!((t.total,t.rx,t.tx,t.done_mask,t.requests),before);
}
#[cfg(feature="sunfish-ruleset-control-window")]
#[test]
fn ruleset_control_opens_on_original_wrq_before_early_read_path(){
 let(p,mut o,mut i)=rf_owner(true);assert_eq!(o.ruleset_control_trace().total,0);
 mc_input(&p,&mut o,&mut i,0x1f,0x5000,qcom_tftp_server_check::REQUEST);
 let e=o.ruleset_control_trace().entries[0].unwrap();assert_eq!((e.flags,e.command,e.tag,e.c>>16,e.d>>24),(0,9,0xa,2,1));assert!(o.startup_prerequisites().reads.iter().all(Option::is_none));assert_eq!(o.ruleset_control_trace().done_mask,0);
}
#[cfg(feature="sunfish-ruleset-control-window")]
#[test]
fn ruleset_control_efs_first_opens_context_before_ruleset_and_keeps_original_bounds(){
 let(p,mut o,mut i)=rf_owner(true);let mut s=qcom_tftp_server_check::ServerCheck::new();
 mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0timeoutms\x001000\0");
 let e=o.ruleset_control_trace().entries[0].unwrap();assert_eq!((e.flags,e.command,e.tag,e.d>>24),(0,9,0xa,4));
 o.service_tftp_server_check_step(&mut s,&mut i).unwrap();o.notified(8,&mut i).unwrap();let efs=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&efs);
 let first=rf_send(&p,&mut o,&mut i,&mut s,0x21);acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);let a=rf_send(&p,&mut o,&mut i,&mut s,0x21);let b=rf_send(&p,&mut o,&mut i,&mut s,0x23);
 for q in o.ruleset_control_trace().requests{assert!(q.unwrap().bounds.unwrap().first_visible.is_some());}
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&a);acknowledge_outgoing_glink(&p,&mut o,&mut i,&b);assert_eq!(o.ruleset_control_trace().done_mask,3);
}
#[cfg(feature="sunfish-ruleset-control-window")]
#[test]
fn ruleset_control_uncertain_publication_is_not_a_successful_write_entry(){
 let(p,mut o,mut i)=rf_owner(true);let mut s=qcom_tftp_server_check::ServerCheck::new();let first=rf_send(&p,&mut o,&mut i,&mut s,0x21);acknowledge_outgoing_glink(&p,&mut o,&mut i,&first);
 mc_input(&p,&mut o,&mut i,0x21,0x5000,RF_RULE);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();let tx=o.ruleset_control_trace().tx;
 p.0.borrow_mut().fail_after_store=true;assert!(o.notified(8,&mut i).is_err());assert_eq!(o.ruleset_control_trace().tx,tx);assert!(o.ruleset_reply_fifo(0).is_none());assert!(o.error().is_some());
}
