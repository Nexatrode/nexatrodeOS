#[test]
fn mcfg_idle_adapter_fresh_pending_rx_stops_without_writes(){
 let(mut a,p)=setup();p.push(&[13,0,0,0,0,0,0,0]);let before=p.log().len();assert_eq!(a.publish_mcfg_idle_turn(1).unwrap(),0);assert_eq!(a.mcfg_idle_turn_trace(),super::super::mcfg_idle::McfgIdleTurnTrace{calls:1,writes:0,rx_veto:1,blocked:0});assert!(!p.log()[before..].iter().any(|v|matches!(v,crate::Op::Head(_))));
}
#[test]
fn mcfg_idle_adapter_blocked_fallback_keeps_exact_packet(){
 let(mut a,p)=setup();for _ in 0..(FIFO_BYTES-8)/8{a.ring.write(&[13,0,0,0,0,0,0,0],0).unwrap();}assert_eq!(a.publish_mcfg_idle_turn(1).unwrap(),0);assert_eq!(a.mcfg_idle_turn_trace().blocked,1);p.take_tx();drain(&mut a,&p);assert_eq!(a.session.tftp_tx_progress()[2],1);
}
#[test]
fn mcfg_idle_adapter_uncertain_write_stays_terminal(){
 let(mut a,p)=setup();p.0.borrow_mut().revoke_after_write=true;assert!(a.publish_mcfg_idle_turn(1).is_err());assert_eq!(a.mcfg_idle_turn_trace().writes,0);assert!(a.publish_next(1).is_err());assert_eq!(a.session.tftp_tx_progress()[2],0);
}

#[test]
fn mcfg_idle_adapter_committed_read_notification_counts_as_activity(){
 let(mut a,p)=setup();for _ in 0..(FIFO_BYTES-16)/8{a.ring.write(&[13,0,0,0,0,0,0,0],0).unwrap();}let before=p.log().len();assert_eq!(a.publish_mcfg_idle_turn(1).unwrap(),1);assert_eq!(a.mcfg_idle_turn_trace().writes,0);assert_eq!(a.mcfg_idle_turn_trace().blocked,1);assert_eq!(p.log()[before..].iter().filter(|v|matches!(v,crate::Op::Head(_))).count(),1);p.take_tx();drain(&mut a,&p);assert_eq!(a.session.tftp_tx_progress()[2],1);
}
