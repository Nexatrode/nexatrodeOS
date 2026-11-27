// Actual Owner replay: an older request survives while a lower array slot is reused.
#[cfg(feature="sunfish-startup-prerequisites")]
#[test]
fn tftp_arrival_order_owner_lower_hole_cannot_overtake_older_efs(){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();
 let dummy=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";let efs=b"\0\x01/readwrite/modem_efs\0octet\0";
 mc_input(&p,&mut o,&mut i,0x30,0x5000,dummy);let released=o.pending_tftp_request().unwrap();
 mc_input(&p,&mut o,&mut i,0x21,0x5000,efs);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let older=o.pending_tftp_request().unwrap();assert_ne!(released,older);
 o.notified(8,&mut i).unwrap();take_owner_datagrams(&p,&mut o,&mut i);
 mc_input(&p,&mut o,&mut i,0x21,0x5000,efs);let next=o.pending_tftp_request().unwrap();
 #[cfg(feature="sunfish-tftp-arrival-order")]assert_eq!(next,older);
 #[cfg(not(feature="sunfish-tftp-arrival-order"))]assert_ne!(next,older);
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let c=o.startup_prerequisites();
 #[cfg(feature="sunfish-tftp-arrival-order")]let selected=2;
 #[cfg(not(feature="sunfish-tftp-arrival-order"))]let selected=3;
 assert_eq!((c.reads[selected].unwrap().flags,c.reads[5-selected].unwrap().flags),(15,1));
 let first_seq=c.reads[selected].unwrap().record_sequence().unwrap();
 // Same-peer flow remains busy; the new order does not invent a completion.
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let waiting=o.startup_prerequisites().reads[5-selected].unwrap();assert_eq!((waiting.flags,waiting.waits),(7,[1,0]));
 o.notified(8,&mut i).unwrap();let wire=p.take_tx();assert_eq!(o.startup_prerequisites().reads[selected].unwrap().flags,31);assert_eq!(o.startup_prerequisites().reads[5-selected].unwrap().flags,7);
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[selected].unwrap().flags,63);
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let c=o.startup_prerequisites();assert_eq!(c.reads[5-selected].unwrap().flags,15);assert!(c.reads[5-selected].unwrap().record_sequence().unwrap()>first_seq);
 o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[5-selected].unwrap().flags,63);assert!(o.error().is_none());
 println!("ARRIVAL_ORDER_REPLAY order={} older_selected={} both_real_completions=1 flowbusy_preserved=1",if cfg!(feature="sunfish-tftp-arrival-order"){1}else{0},selected==2);
}
#[cfg(feature="sunfish-tftp-arrival-order")]
#[test]
fn tftp_arrival_order_owner_compacted_handle_survives_payload_and_close_mask(){
 let(p,mut o,mut i)=mc_owner();let mut server=mc_file();
 let rule=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";let efs=b"\0\x01/readwrite/modem_efs\0octet\0";
 mc_input(&p,&mut o,&mut i,0x30,0x5000,rule);mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let older=o.pending_tftp_request().unwrap();o.notified(8,&mut i).unwrap();take_owner_datagrams(&p,&mut o,&mut i);
 mc_input(&p,&mut o,&mut i,0x4b,0x5000,efs);assert_eq!(o.pending_tftp_request(),Some(older));assert_eq!(o.tftp_datagram(&older).unwrap().1,STAT);
 mc_close(&p,&mut o,&mut i,0x4a);assert!(o.pending_mcfg_close().is_some());o.resolve_mcfg_close(&mut server).unwrap();assert_eq!(o.mcfg_close_trace().held,1);assert_eq!(server.mcfg_progress().1,Some(1));
 let next=o.pending_tftp_request().unwrap();assert_ne!(next,older);let(route,body)=o.tftp_datagram(&next).unwrap();assert_eq!((route.destination.port,body),(0x4b,efs.as_slice()));
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert!(o.pending_tftp_request().is_none());assert!(o.error().is_none());
}
#[cfg(feature="sunfish-tftp-arrival-order")]
#[test]
fn tftp_arrival_order_owner_exact_wrq_coalescing_keeps_original_handle(){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();
 let rule=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";let efs=b"\0\x01/readwrite/modem_efs\0octet\0";
 mc_input(&p,&mut o,&mut i,0x30,0x5000,rule);mc_input(&p,&mut o,&mut i,0x1f,0x5000,qcom_tftp_server_check::REQUEST);
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let original=o.pending_tftp_request().unwrap();o.notified(8,&mut i).unwrap();take_owner_datagrams(&p,&mut o,&mut i);
 mc_input(&p,&mut o,&mut i,0x1f,0x5000,qcom_tftp_server_check::REQUEST);assert_eq!(o.tftp_wrq_coalesced(),1);assert_eq!(o.pending_tftp_request(),Some(original));
 mc_input(&p,&mut o,&mut i,0x21,0x5000,efs);assert_eq!(o.pending_tftp_request(),Some(original));let(route,body)=o.tftp_datagram(&original).unwrap();assert_eq!((route.destination.port,body),(0x1f,qcom_tftp_server_check::REQUEST));
 o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let wire=p.take_tx();let datagrams=owner_datagrams(&wire);assert!(datagrams.iter().any(|b|b.len()>=32+qcom_tftp_server_check::OACK.len()&&&b[32..32+qcom_tftp_server_check::OACK.len()]==qcom_tftp_server_check::OACK));acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert!(o.error().is_none());
}
#[cfg(feature="sunfish-efs-reply-fifo")]
#[test]
fn efs_reply_fifo_owner_tail_close_foreign_receipt_and_exact_done(){
 let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();let efs=b"\0\x01/readwrite/modem_efs\0octet\0";let rule=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";
 mc_input(&p,&mut o,&mut i,0x22,0x5000,efs);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let wire=p.take_tx();let w=o.efs_reply_fifo().unwrap();assert_eq!(w.record,o.startup_prerequisites().reads[2].unwrap().record_sequence().unwrap());assert!(!w.fifo.finished);
 o.notified(8,&mut i).unwrap();assert!(o.efs_reply_fifo().unwrap().fifo.peer_tail_at.is_some());assert!(!o.efs_reply_fifo().unwrap().fifo.finished);
 mc_input(&p,&mut o,&mut i,0x30,0x5000,rule);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();let other=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&other);assert!(!o.efs_reply_fifo().unwrap().fifo.finished);
 mc_close(&p,&mut o,&mut i,0x22);assert!(o.efs_reply_fifo().unwrap().unread_at_close.is_some());assert!(!o.efs_reply_fifo().unwrap().fifo.finished);
 acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);let w=o.efs_reply_fifo().unwrap();assert!(w.fifo.finished);assert!(w.fifo.completion.is_some());assert_eq!(w.fifo.completion.unwrap().visible_at_close,Some(false));assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,0xbf);assert!(o.error().is_none());
}

include!("sunfish_efs_early_cases.rs");
include!("sunfish_efs_service_cases.rs");
include!("sunfish_efs_overlap_cases.rs");
