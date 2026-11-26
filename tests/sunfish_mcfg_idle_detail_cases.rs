#[test]
fn mcfg_idle_detail_owner_copies_actual_context_and_four_commits(){
 let(p,mut o,mut i,_)=setup();o.notified(8,&mut i).unwrap();let d=o.mcfg_idle_detail();assert!(d.captured);assert_eq!(d.meta,[0x4a,0x5006,19,0]);assert_eq!(d.summary,[4,0,0]);assert_eq!(d.call,1);assert_eq!(d.attempts.iter().flatten().map(|r|r.header.unwrap().command).collect::<Vec<_>>(),[6,5,8,9]);assert!(d.attempts.iter().flatten().all(|r|r.result==1));assert_eq!(d.attempts[1].unwrap().advertisement.unwrap()[1],4096);assert_eq!(d.rx_total,1);assert_eq!(d.rx[0].unwrap().tag,5);assert_eq!(o.tftp_tx_progress()[2],0);p.take_tx();
}
#[test]
fn mcfg_idle_detail_one_control_then_veto_follows_normal_rx(){
 let(p,mut o,mut i,_)=setup();p.0.borrow_mut().append_after_notify=Some(vec![13,0,0,0,0,0,0,0]);o.notified(8,&mut i).unwrap();let d=o.mcfg_idle_detail();assert_eq!(d.summary,[1,1,0]);let a=d.attempts[0].unwrap();assert_eq!((a.header.unwrap().command,a.result),(6,1));let b=d.attempts[1].unwrap();assert!(b.header.is_none());assert_eq!((b.result,b.unread),(4,8));assert_eq!(d.rx[0].unwrap().header.unwrap().command,13);assert_eq!(d.rx[1].unwrap().tag,5);
}
#[test]
fn mcfg_idle_detail_exact_close_works_before_any_post_elf_watcher(){
 let(p,mut o,mut i,close)=setup();assert!(o.post_elf_mcfg().is_none());p.0.borrow_mut().append_after_notify=Some(crate::data(close,&crate::owner_control_frame(6,[0,0x4a,0,0])));o.notified(8,&mut i).unwrap();let d=o.mcfg_idle_detail();assert_eq!(d.summary,[1,1,0]);assert_eq!(d.rx_total,1);assert_eq!(d.rx[0].unwrap().header.unwrap().del_client,Some([0,0x4a]));assert!(o.pending_mcfg_close.is_some());assert_eq!(o.tftp_tx_progress()[1],0);
}
#[test]
fn mcfg_idle_detail_never_rebinds_first_loan_to_later_reply(){
 let(_,mut o,mut i,_)=setup();o.notified(8,&mut i).unwrap();let expected=o.mcfg_idle_detail().meta;let r=o.peer_tftp_replies[0].unwrap();o.adapter.arm_idle_detail(r.meta.route,123,7);assert_eq!(o.mcfg_idle_detail().meta,expected);assert_eq!(o.mcfg_idle_detail().rx_total,1);
}
