// Exact absent-MCFG response states through the actual serialized Owner/FIFO/server.
fn missing_setup()->(Fake,Owner<Fake>,Irqs,qcom_tftp_server_check::ServerCheck,u32){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x49,0x5000,STAT);
 let close=mc_offer(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();assert_eq!(server.mcfg_progress(),(0,None,0));(p,o,i,server,close)
}
fn missing_close(p:&Fake,o:&mut Owner<Fake>,i:&mut Irqs,close:u32){p.push(&data(close,&owner_control_frame(6,[0,0x49,0,0])));o.notified(1,i).unwrap();}
#[test]
fn mcfg_missing_close_cancels_unstarted_error_without_creating_file(){
 let(p,mut o,mut i,mut server,close)=missing_setup();missing_close(&p,&mut o,&mut i,close);
 assert!(o.pending_mcfg_close().is_some(),"known missing-MCFG transfer must pause even with no held retry");o.resolve_mcfg_close(&mut server).unwrap();
 let t=o.mcfg_close_trace();assert_eq!((t.committed,t.unsent,t.published),(1,1,0));assert_eq!(t.kind,3);assert_eq!(server.mcfg_progress(),(0,None,0));o.notified(8,&mut i).unwrap();assert!(take_owner_datagrams(&p,&mut o,&mut i).is_empty());
}
#[test]
fn mcfg_missing_close_published_error_waits_for_real_receipt(){
 let(p,mut o,mut i,mut server,close)=missing_setup();o.notified(8,&mut i).unwrap();let wire=p.take_tx();let packets=owner_datagrams(&wire);assert_eq!(packets.len(),1);assert_eq!(&packets[0][32..51],qcom_tftp_server_check::MISSING_REPLY);
 missing_close(&p,&mut o,&mut i,close);o.resolve_mcfg_close(&mut server).unwrap();let t=o.mcfg_close_trace();assert_eq!((t.committed,t.unsent,t.published),(1,0,1));assert_eq!(t.kind,3);assert_eq!(o.tftp_tx_progress()[2],0);
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.tftp_tx_progress()[2],1);assert_eq!(server.mcfg_progress(),(0,None,0));
}
#[test]
fn mcfg_missing_close_drops_retained_duplicate_but_keeps_original_completion(){
 let(p,mut o,mut i,mut server,close)=missing_setup();o.notified(8,&mut i).unwrap();let wire=p.take_tx();mc_input(&p,&mut o,&mut i,0x49,0x5000,STAT);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();assert!(o.tftp_queue_snapshot().1);
 missing_close(&p,&mut o,&mut i,close);o.resolve_mcfg_close(&mut server).unwrap();let t=o.mcfg_close_trace();assert_eq!((t.committed,t.retained,t.published),(1,1,1));assert!(!o.tftp_queue_snapshot().1);assert_eq!(o.tftp_tx_progress()[2],0);
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);o.notified(8,&mut i).unwrap();assert_eq!(o.tftp_tx_progress()[2],1);assert!(take_owner_datagrams(&p,&mut o,&mut i).is_empty());
}
#[test]
fn mcfg_missing_close_held_retry_does_not_fail_at_server_preflight(){
 let(p,mut o,mut i,mut server,close)=missing_setup();o.notified(8,&mut i).unwrap();let wire=p.take_tx();mc_input(&p,&mut o,&mut i,0x49,0x5000,STAT);
 missing_close(&p,&mut o,&mut i,close);o.resolve_mcfg_close(&mut server).unwrap();assert_eq!((o.mcfg_close_trace().committed,o.mcfg_close_trace().held),(1,1));assert!(o.pending_tftp_request().is_none());acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.tftp_tx_progress()[2],1);
}
#[test]
fn mcfg_missing_close_rejects_foreign_server_check_ruleset_and_efs_transfers(){
 for request in [qcom_tftp_server_check::REQUEST,b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0",b"\0\x01/readwrite/modem_efs\0octet\0"]{
  let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();let route=qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x49},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}};
  server.receive(route,request,&mut[0;128]).unwrap();o.service_tftp_server_check_step(&mut server,&mut i).unwrap();mc_input(&p,&mut o,&mut i,0x49,0x5000,STAT);mc_close(&p,&mut o,&mut i,0x49);
  assert!(o.resolve_mcfg_close(&mut server).is_err());assert_eq!(o.mcfg_close_trace().stage,1);assert_eq!(o.mcfg_close_trace().committed,0);
 }
}
#[test]
fn mcfg_missing_close_does_not_cancel_later_committed_file(){
 let(p,mut o,mut i,mut server,close)=missing_setup();o.notified(8,&mut i).unwrap();let wire=p.take_tx();commit_one(&mut server,&mut[0;128]);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();
 missing_close(&p,&mut o,&mut i,close);o.resolve_mcfg_close(&mut server).unwrap();assert_eq!(server.mcfg_progress().1,Some(1));acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.tftp_tx_progress()[2],1);
 let route=qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x49},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}};assert!(server.receive(route,STAT,&mut[0;128]).is_err(),"closed port remains reserved");
}
#[test]
fn mcfg_missing_close_keeps_fresh_validation_faults_terminal(){
 for fault in 0..3 {let(p,mut o,mut i,mut server,close)=missing_setup();missing_close(&p,&mut o,&mut i,close);let before=p.log().len();
  match fault {0=>p.0.borrow_mut().observation.ready=false,1=>p.0.borrow_mut().fail=Some("snapshot"),_=>p.0.borrow_mut().words[3]=1}
  assert!(o.resolve_mcfg_close(&mut server).is_err());assert_eq!(o.mcfg_close_trace().stage,3);assert_eq!(o.mcfg_close_trace().committed,0);assert_eq!(server.mcfg_progress(),(0,None,0));assert!(!p.log()[before..].iter().any(|v|matches!(v,Op::Write(..)|Op::Head(_)|Op::Tail(_)|Op::Notify)));
 }
}

#[test]
fn mcfg_missing_close_classification_preserves_ram_read_and_write_kinds(){
 let mut server=mc_file();let writer=qcom_tftp_server_check::Address{node:0,port:0x56};assert_eq!(server.mcfg_close_kind(writer),2);
 let reader=qcom_tftp_server_check::Address{node:0,port:0x49};server.receive(qcom_tftp_server_check::Route{source:reader,destination:qcom_tftp_server_check::Address{node:1,port:0x5000}},STAT,&mut[0;128]).unwrap();assert_eq!(server.mcfg_close_kind(reader),1);
 assert!(server.preflight_mcfg_close(reader).unwrap().is_some());assert_eq!(server.mcfg_close_kind(qcom_tftp_server_check::Address{node:0,port:0x57}),0);
}
