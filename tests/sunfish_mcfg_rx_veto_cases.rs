#[test]
fn mcfg_rx_veto_actual_pending_record_then_empty_and_normal_retry(){
 let(mut a,p)=setup();p.push(&[13,0,0,0,0,0,0,0]);a.publish_mcfg_queued_turn(1).unwrap();assert_eq!(a.mcfg_rx_veto_trace().total,1);let e=a.mcfg_rx_veto_trace().records[0].unwrap();assert_eq!((e.tag,e.unread,e.early,e.call),(1,8,0,1));assert!(e.at.is_none()&&e.header.is_none());
 a.poll_receive(1).unwrap();a.poll_receive(1).unwrap();let t=a.mcfg_rx_veto_trace();assert_eq!(t.total,3);let h=t.records[1].unwrap().header.unwrap();assert_eq!((h.command,h.channel,h.parameter,h.bytes),(13,0,0,8));assert_eq!(t.records[2].unwrap().tag,5);assert_eq!(t.records[2].unwrap().unread,0);
 a.publish_mcfg_queued_turn(1).unwrap();assert_eq!(a.mcfg_queued_turn_trace().writes,4);assert_eq!(a.session.tftp_tx_progress()[2],0);drain(&mut a,&p); // remains pending until exact receipt from already published wire
}
#[test]
fn mcfg_rx_veto_actual_arriving_record_identifies_prehead_stage(){
 let(mut a,p)=setup();p.0.borrow_mut().append_notification_on_write=true;a.publish_mcfg_queued_turn(1).unwrap();let e=a.mcfg_rx_veto_trace().records[0].unwrap();assert_eq!(e.tag,2);assert_eq!(e.unread,8);a.poll_receive(1).unwrap();a.poll_receive(1).unwrap();assert_eq!(a.mcfg_rx_veto_trace().records[1].unwrap().header.unwrap().command,13);assert_eq!(a.mcfg_rx_veto_trace().records[2].unwrap().tag,5);assert_eq!(a.mcfg_queued_turn_trace().writes,0);drain(&mut a,&p);assert_eq!(a.session.tftp_tx_progress()[2],1);
}
#[test]
fn mcfg_rx_veto_semantic_fault_does_not_advance_or_publish(){
 let(mut a,p)=setup();p.push(&crate::control(7,99,52));a.publish_mcfg_queued_turn(1).unwrap();let before=p.log().len();assert!(a.poll_receive(1).is_err());let h=a.mcfg_rx_veto_trace().records[1].unwrap().header.unwrap();assert_eq!((h.command,h.channel),(7,99));assert!(!p.log()[before..].iter().any(|v|matches!(v,crate::Op::Tail(_)|crate::Op::Head(_)|crate::Op::Notify)));assert!(a.publish_next(1).is_err());
}
