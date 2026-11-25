#[test]
fn mcfg_queued_owner_opt_in_publishes_once_and_waits_for_real_receipt(){
 let(p,mut o,mut i)=mc_owner();o.enable_mcfg_queued_turn();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let wire=p.take_tx();let b=owner_datagrams(&wire);assert_eq!(b.len(),1);assert_eq!(&b[0][32..51],qcom_tftp_server_check::MISSING_REPLY);let t=o.mcfg_queued_turn_trace();assert_eq!(t.calls,1);assert!(t.writes>0&&t.writes<=4);assert_eq!(o.tftp_tx_progress()[2],0);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();assert_eq!(o.mcfg_queued_turn_trace().calls,1);acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.tftp_tx_progress()[2],1);
}
#[test]
fn mcfg_queued_owner_default_and_foreign_missing_paths_keep_old_schedule(){
 for enabled in [false,true]{let(p,mut o,mut i)=mc_owner();if enabled{o.enable_mcfg_queued_turn();}let mut server=qcom_tftp_server_check::ServerCheck::new();let b=if enabled{b"\0\x01/readwrite/modem_efs\0octet\0".as_slice()}else{STAT};mc_input(&p,&mut o,&mut i,0x4a,0x5000,b);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();assert_eq!(o.mcfg_queued_turn_trace().calls,0);assert!(owner_datagrams(&p.take_tx()).is_empty());o.notified(8,&mut i).unwrap();assert_eq!(take_owner_datagrams(&p,&mut o,&mut i).len(),1);}
}
#[test]
fn mcfg_queued_owner_pending_close_vetoes_and_cancels_unsent(){
 let(p,mut o,mut i)=mc_owner();o.enable_mcfg_queued_turn();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);let close=mc_offer(&p,&mut o,&mut i);p.push(&data(close,&owner_control_frame(6,[0,0x4a,0,0])));let before=p.log().len();o.service_tftp_server_check_step(&mut server,&mut i).unwrap();assert_eq!(o.mcfg_queued_turn_trace().rx_veto,1);assert!(!p.log()[before..].iter().any(|v|matches!(v,Op::Head(_)|Op::Notify)));o.notified(1,&mut i).unwrap();assert!(o.pending_mcfg_close().is_some());o.resolve_mcfg_close(&mut server).unwrap();assert_eq!(o.mcfg_close_trace().unsent,1);assert_eq!(o.tftp_tx_progress()[2],0);
}
#[test]
fn mcfg_queued_owner_published_error_survives_close_until_receipt(){
 let(p,mut o,mut i)=mc_owner();o.enable_mcfg_queued_turn();let mut server=qcom_tftp_server_check::ServerCheck::new();mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();let wire=p.take_tx();mc_close(&p,&mut o,&mut i,0x4a);o.resolve_mcfg_close(&mut server).unwrap();assert_eq!(o.mcfg_close_trace().published,1);assert_eq!(o.tftp_tx_progress()[2],0);acknowledge_outgoing_glink(&p,&mut o,&mut i,&wire);assert_eq!(o.tftp_tx_progress()[2],1);
}
