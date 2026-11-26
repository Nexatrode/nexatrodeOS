#[cfg(feature="sunfish-efs-service-handoff")]
fn efs_pair_step(o:&mut Owner<Fake>,s:&mut qcom_tftp_server_check::ServerCheck,i:&mut Irqs)->Result<Option<qcom_modem_owner::TftpServiceProgress>,qcom_modem_owner::Error>{
 o.service_efs_handoff(s,#[cfg(feature="sunfish-tftp-wlan-mux")]None,i)
}
#[cfg(feature="sunfish-efs-reply-fifo")]
#[test]
fn efs_service_ruleset_pair_uses_normal_dispatch_and_real_completions(){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();
 #[cfg(feature="sunfish-efs-service-handoff")]o.enable_efs_service_handoff();
 let rule=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";let efs=b"\0\x01/readwrite/modem_efs\0octet\0";
 mc_input(&p,&mut o,&mut i,0x21,0x5000,rule);mc_input(&p,&mut o,&mut i,0x22,0x5000,efs);
 assert_eq!(o.service_tftp_server_check_step(&mut server,&mut i).unwrap(),qcom_modem_owner::TftpServiceProgress::ResponseQueued);
 assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,1);
 #[cfg(feature="sunfish-efs-service-handoff")]{assert_eq!(efs_pair_step(&mut o,&mut server,&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::ResponseQueued));assert_eq!(o.efs_service_trace().calls,1);assert_eq!(o.efs_service_trace().outcome,2);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags&15,15);assert_eq!(efs_pair_step(&mut o,&mut server,&mut i).unwrap(),None);}
 #[cfg(not(feature="sunfish-efs-service-handoff"))]{assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,1);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();}
 println!("EFS_SERVICE_PAIR first_efs_dispatched_in_extra_step={}",cfg!(feature="sunfish-efs-service-handoff"));
 o.notified(8,&mut i).unwrap();let wire=p.take_tx();print!("EFS_SERVICE_WIRE ");for b in &wire{print!("{b:02x}");}println!();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);
 assert_eq!(o.startup_prerequisites().reads[0].unwrap().flags,63);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,63);assert!(o.error().is_none());
}
#[cfg(feature="sunfish-efs-service-handoff")]
#[test]
fn efs_service_default_off_or_no_ruleset_cannot_borrow(){
 for enable in [false,true]{let(p,mut o,mut i)=mc_owner();let mut s=qcom_tftp_server_check::ServerCheck::new();if enable{o.enable_efs_service_handoff();}mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");assert_eq!(efs_pair_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.efs_service_trace().calls,0);assert!(o.pending_tftp_request().is_some());}
}
#[cfg(feature="sunfish-efs-service-handoff")]
#[test]
fn efs_service_never_overtakes_an_older_held_request(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();let rule=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";
 mc_input(&p,&mut o,&mut i,0x21,0x5000,rule);mc_input(&p,&mut o,&mut i,0x23,0x5000,rule);mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");o.service_tftp_server_check_step(&mut s,&mut i).unwrap();let older=o.pending_tftp_request();assert_eq!(efs_pair_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.pending_tftp_request(),older);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,1);assert_eq!(o.efs_service_trace().calls,0);
}
#[cfg(feature="sunfish-efs-service-handoff")]
#[test]
fn efs_service_known_close_prevents_borrowed_dispatch(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x21,0x5000,b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0");mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");o.service_tftp_server_check_step(&mut s,&mut i).unwrap();mc_close(&p,&mut o,&mut i,0x22);assert_eq!(efs_pair_step(&mut o,&mut s,&mut i).unwrap(),None);assert_eq!(o.efs_service_trace().calls,0);
}
#[cfg(feature="sunfish-efs-service-handoff")]
#[test]
fn efs_service_fault_is_terminal_and_cannot_repeat(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x21,0x5000,b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0");mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");o.service_tftp_server_check_step(&mut s,&mut i).unwrap();p.0.borrow_mut().observation.ready=false;assert!(efs_pair_step(&mut o,&mut s,&mut i).is_err());assert!(o.error().is_some());assert_eq!(o.efs_service_trace().calls,1);assert!(efs_pair_step(&mut o,&mut s,&mut i).is_err());assert_eq!(o.efs_service_trace().calls,1);
}
#[cfg(feature="sunfish-efs-service-handoff")]
#[test]
fn efs_service_same_peer_flow_pressure_retains_exact_reply_until_real_done(){
 let(p,mut o,mut i)=mc_owner();let mut s=qcom_tftp_server_check::ServerCheck::new();let efs=b"\0\x01/readwrite/modem_efs\0octet\0";
 // A previously published same-peer response holds its real flow token.
 mc_input(&p,&mut o,&mut i,0x22,0x5000,efs);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();o.notified(8,&mut i).unwrap();let older_wire=p.take_tx();assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,31);
 o.enable_efs_service_handoff();mc_input(&p,&mut o,&mut i,0x21,0x5000,b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0");mc_input(&p,&mut o,&mut i,0x22,0x5000,efs);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();assert_eq!(efs_pair_step(&mut o,&mut s,&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::WaitingReply));assert_eq!((o.efs_service_trace().calls,o.efs_service_trace().outcome),(1,3));assert_eq!(o.startup_prerequisites().reads[3].unwrap().flags,7);assert_eq!(efs_pair_step(&mut o,&mut s,&mut i).unwrap(),None);
 o.notified(8,&mut i).unwrap();let rule_wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&rule_wire);assert_eq!(o.startup_prerequisites().reads[3].unwrap().flags,7);acknowledge_outgoing_glink(&p,&mut o,&mut i,&older_wire);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,63);o.service_tftp_server_check_step(&mut s,&mut i).unwrap();o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[3].unwrap().flags,63);assert_eq!(o.efs_service_trace().calls,1);assert!(o.error().is_none());
}
#[cfg(all(feature="sunfish-efs-service-handoff",feature="sunfish-tftp-wlan-mux"))]
#[test]
fn efs_service_combined_provider_uses_same_server_check_policy(){
 use crate::qcom_qrtr_tftp::{AuthorizedWlanImage,Server};let(p,mut o,mut i)=mc_owner();o.enable_efs_service_handoff();let mut s=qcom_tftp_server_check::ServerCheck::new();let mut fw=Server::new_bounded(1,0,0x5000,0x5100,0x51ff,&[],AuthorizedWlanImage::test_fixture(b"fw")).unwrap();mc_input(&p,&mut o,&mut i,0x21,0x5000,b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0");mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");o.service_combined_tftp_step(&mut s,&mut fw,&mut i).unwrap();assert_eq!(o.service_efs_handoff(&mut s,Some(&mut fw),&mut i).unwrap(),Some(qcom_modem_owner::TftpServiceProgress::ResponseQueued));o.notified(8,&mut i).unwrap();let wire=p.take_tx();let packets=owner_datagrams(&wire);assert_eq!(packets.len(),2);assert!(packets.iter().all(|p|&p[32..32+owner_word(p,20)as usize]==qcom_tftp_server_check::MISSING_REPLY));acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,63);assert!(o.error().is_none());
}
