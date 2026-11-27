#[cfg(feature="sunfish-efs-reply-fifo")]
#[test]
fn efs_early_owner_normal_wire_and_real_receipt(){
 for enabled in [false,true]{
  let(p,mut o,mut i)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();
  #[cfg(feature="sunfish-efs-early-error-turn")]if enabled{o.enable_efs_error_turn();}
  #[cfg(not(feature="sunfish-efs-early-error-turn"))]let _=enabled;
  let rule=b"\0\x01/readwrite/ota_firewall/ruleset\0octet\0";mc_input(&p,&mut o,&mut i,0x30,0x5000,rule);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();take_owner_datagrams(&p,&mut o,&mut i);
  #[cfg(feature="sunfish-efs-early-error-turn")]assert_eq!(o.efs_error_turn_trace().calls,0);
  let req=b"\0\x01/readwrite/modem_efs\0octet\0";
  for n in 0..2{mc_input(&p,&mut o,&mut i,0x22,0x5000,req);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();
   #[cfg(feature="sunfish-efs-early-error-turn")]
   if enabled{let t=o.efs_error_turn_trace();assert_eq!(t.calls,1);assert_eq!(t.stop,1);assert!(t.writes>0&&t.writes<=4);if n==0{assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,31);}}
   #[cfg(not(feature="sunfish-efs-early-error-turn"))]let _=n;
   o.notified(8,&mut i).unwrap();let wire=p.take_tx();assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags&32,if n==0{0}else{32});acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[2+n].unwrap().flags,63);
   print!("EFS_EARLY_WIRE ");for b in wire{print!("{b:02x}");}println!();
  }
  assert!(o.error().is_none());
 }
}
#[cfg(feature="sunfish-efs-early-error-turn")]
#[test]
fn efs_early_owner_fresh_rx_veto_spends_once_then_normal_path_completes(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_error_turn();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");p.push(&control(13,0,0));o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let t=o.efs_error_turn_trace();assert_eq!((t.calls,t.writes,t.rx_veto,t.stop),(1,0,1,2));assert!(owner_datagrams(&p.take_tx()).is_empty());o.notified(8,&mut i).unwrap();let wire=p.take_tx();acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags,63);assert_eq!(o.efs_error_turn_trace().calls,1);assert!(o.error().is_none());
}
#[cfg(feature="sunfish-efs-early-error-turn")]
#[test]
fn efs_early_owner_uncertain_io_is_terminal_and_cannot_repeat(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_error_turn();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");p.0.borrow_mut().fail_after_store=true;assert!(o.service_tftp_server_check_step(&mut server,&mut i).is_err());assert!(o.error().is_some());assert_eq!(o.efs_error_turn_trace().calls,1);assert_eq!(o.startup_prerequisites().reads[2].unwrap().flags&48,0);assert!(o.service_tftp_server_check_step(&mut server,&mut i).is_err());assert_eq!(o.efs_error_turn_trace().calls,1);
}
#[cfg(feature="sunfish-efs-early-error-turn")]
#[test]
fn efs_early_owner_already_closed_request_gets_no_borrowed_turn(){
 let(p,mut o,mut i)=mc_owner();o.enable_efs_error_turn();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x22,0x5000,b"\0\x01/readwrite/modem_efs\0octet\0");mc_close(&p,&mut o,&mut i,0x22);let _=o.service_tftp_server_check_step(&mut server,&mut i);assert_eq!(o.efs_error_turn_trace().calls,0);
}
