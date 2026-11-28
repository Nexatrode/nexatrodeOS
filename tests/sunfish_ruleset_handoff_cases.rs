const REVERSE_RULE:&[u8]=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0timeoutms\x001000\0rsize\x004\0";
const REVERSE_EFS:&[u8]=b"\0\x01/readwrite/modem_efs\0octet\0";
fn ruleset_step(o:&mut Owner<Fake>,s:&mut qcom_tftp_server_check::ServerCheck,i:&mut Irqs)->Result<Option<qcom_modem_owner::TftpServiceProgress>,qcom_modem_owner::Error>{o.service_ruleset_handoff(s,#[cfg(feature="sunfish-tftp-wlan-mux")]None,i)}
fn reverse_inputs(p:&Fake,o:&mut Owner<Fake>,i:&mut Irqs){mc_input(p,o,i,0x22,0x5000,REVERSE_EFS);mc_input(p,o,i,0x21,0x5000,REVERSE_RULE);}
#[test]
fn ruleset_reverse_service_queues_on_same_turn_and_real_completions(){
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();reverse_inputs(&p,&mut o,&mut i);
 assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None,"first EFS has not queued yet");o.service_tftp_server_check_step(&mut s,&mut i).unwrap();assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,1);
 assert_eq!(efs_pair_step(&mut o,&mut s,&mut i).unwrap(),None,"old handoff has no reverse case");
 assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::ResponseQueued));let t=o.ruleset_handoff_trace();assert_eq!((t.calls,t.outcome,t.writer.calls),(1,2,1));assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,31);assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);
 let wire=p.take_tx();let packets=owner_datagrams(&wire);assert_eq!(packets.len(),2);assert!(packets.iter().all(|p|&p[32..32+owner_word(p,20)as usize]==qcom_tftp_server_check::MISSING_REPLY));acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!((o.startup_prerequisites().reads[0].unwrap().flags,o.startup_prerequisites().reads[2].unwrap().flags),(63,63));assert!(o.error().is_none());
 println!("REVERSE_HANDOFF {:?}",t);print!("REVERSE_HANDOFF_WIRE ");for b in wire{print!("{b:02x}");}println!();
}
#[test]
fn ruleset_reverse_default_off_and_original_arrival_order(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,1);assert_eq!(o.ruleset_handoff_trace().calls,0);
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x21,0x5000,REVERSE_RULE);mc_input(&p,&mut o,&mut i,0x22,0x5000,REVERSE_EFS);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(efs_pair_step(&mut o,&mut s,&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::ResponseQueued));assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.ruleset_handoff_trace().writer.calls,0);
}
#[test]
fn ruleset_reverse_cannot_overtake_older_request_or_wrong_peer_close(){
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x22,0x5000,REVERSE_EFS);mc_input(&p,&mut o,&mut i,0x23,0x5000,STAT);mc_input(&p,&mut o,&mut i,0x21,0x5000,REVERSE_RULE);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();let older=o.pending_tftp_request();assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.pending_tftp_request(),older);assert_eq!(o.ruleset_handoff_trace().calls,0);
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();mc_close(&p,&mut o,&mut i,0x23);assert_eq!(o.ruleset_handoff_trace().flags&32,0);assert!(ruleset_step(&mut o,&mut s,&mut i).unwrap().is_some());
}
#[test]
fn ruleset_reverse_known_close_and_terminal_fault_cannot_repeat(){
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();mc_close(&p,&mut o,&mut i,0x21);assert_ne!(o.ruleset_handoff_trace().flags&32,0);assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.ruleset_handoff_trace().calls,0);
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();p.0.borrow_mut().observation.ready=false;assert!(ruleset_step(&mut o,&mut s,&mut i).is_err());assert_eq!((o.ruleset_handoff_trace().calls,o.ruleset_handoff_trace().outcome),(1,6));assert!(ruleset_step(&mut o,&mut s,&mut i).is_err());assert_eq!(o.ruleset_handoff_trace().calls,1);
}
#[test]
fn ruleset_reverse_retained_other_reply_prevents_service_borrow(){
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x50,0x5000,STAT);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();o.notified(8,&mut i).unwrap();p.take_tx();mc_input(&p,&mut o,&mut i,0x50,0x5000,STAT);assert_eq!(o.service_tftp_server_check_step(&mut s,&mut i).unwrap(),qcom_modem_owner::TftpServiceProgress::WaitingReply);reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,1);assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.ruleset_handoff_trace().calls,0);
}
#[test]
fn ruleset_reverse_rx_veto_preserves_normal_continuation(){
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();let earlier=p.take_tx();p.push(&control(13,0,0));assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::ResponseQueued));let t=o.ruleset_handoff_trace();assert_eq!((t.writer.calls,t.writer.rx_veto,t.writer.stop),(1,1,2));assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,15);o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&earlier);acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.ruleset_handoff_trace().writer.calls,1);assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,63);
}
#[cfg(feature="sunfish-tftp-wlan-mux")]
#[test]
fn ruleset_reverse_combined_provider_uses_same_static_error(){
 use crate::qcom_qrtr_tftp::{AuthorizedWlanImage,Server};let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();let mut fw=Server::new_bounded(1,0,0x5000,0x5100,0x51ff,&[],AuthorizedWlanImage::test_fixture(b"fw")).unwrap();reverse_inputs(&p,&mut o,&mut i);o.service_combined_tftp_step(&mut s,&mut fw,&mut i).unwrap();assert_eq!(o.service_ruleset_handoff(&mut s,Some(&mut fw),&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::ResponseQueued));let w=p.take_tx();assert_eq!(owner_datagrams(&w).len(),2);acknowledge_outgoing_glink(&p,&mut o,&mut i,&w);assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,63);
}
#[test]
fn ruleset_reverse_uncertain_writer_is_terminal_without_completion(){
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();p.take_tx();p.0.borrow_mut().fail_after_store=true;assert!(ruleset_step(&mut o,&mut s,&mut i).is_err());let t=o.ruleset_handoff_trace();assert_eq!((t.calls,t.outcome,t.writer.calls,t.writer.stop),(1,6,1,6));assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags&48,0);assert!(o.error().is_some());assert!(ruleset_step(&mut o,&mut s,&mut i).is_err());assert_eq!(o.ruleset_handoff_trace().writer.calls,1);
}
#[test]
fn ruleset_reverse_flow_busy_retains_original_until_real_receipt(){
 let(p,mut o,mut i)=mc_owner();o.enable_ruleset_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();o.queue_tftp_response(qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5001},destination:qcom_qmi_router::Address{node:0,port:0x21}},qcom_tftp_server_check::MISSING_REPLY,&mut i).unwrap();o.notified(8,&mut i).unwrap();let older=p.take_tx();reverse_inputs(&p,&mut o,&mut i);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();let efs=p.take_tx();assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::WaitingReply));assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,7);assert_eq!(o.ruleset_handoff_trace().writer.calls,0);assert_eq!(ruleset_step(&mut o,&mut s,&mut i).unwrap(),None);acknowledge_outgoing_glink(&p,&mut o,&mut i,&efs);assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,7);acknowledge_outgoing_glink(&p,&mut o,&mut i,&older);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();assert_eq!(o.ruleset_handoff_trace().writer.calls,1);let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,63);assert_eq!(o.ruleset_handoff_trace().calls,1);assert!(o.error().is_none());
}
