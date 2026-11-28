// Identical actual Owner traffic with only bounded observation capacity changed.
#[test]
fn startup_coverage_four_ruleset_real_lifecycle_wire_and_io(){
 let(p,mut o,mut irq)=mc_owner();let mut server=qcom_tftp_server_check::ServerCheck::new();let mut all=Vec::new();let mut sequences=Vec::new();
 for (n,(path,peer)) in [(b"/readwrite/ota_firewall/ruleset".as_slice(),0x21),(b"/readwrite/ota_firewall/ruleset",0x21),(b"/readwrite/ota_firewall/ruleset",0x22),(b"/readwrite/ota_firewall/ruleset",0x22),(b"/readwrite/modem_efs",0x23),(b"/readwrite/modem_efs",0x23)].iter().enumerate(){
  let mut req=b"\0\x01".to_vec();req.extend(*path);req.extend(b"\0octet\0timeoutms\x001000\0");mc_input(&p,&mut o,&mut irq,*peer,0x5000,&req);
  o.service_tftp_server_check_step(&mut server,&mut irq).unwrap();o.notified(8,&mut irq).unwrap();let wire=p.take_tx();let packets=owner_datagrams(&wire);assert_eq!(packets.len(),1);assert_eq!(&packets[0][32..32+owner_word(&packets[0],20)as usize],qcom_tftp_server_check::MISSING_REPLY);all.extend_from_slice(&wire);
  let index=match n{0=>Some(0),1=>Some(1),2=>cfg!(feature="sunfish-startup-reply-coverage").then_some(6),3=>cfg!(feature="sunfish-startup-reply-coverage").then_some(7),4=>Some(2),_=>Some(3)};
  if let Some(index)=index{let r=o.startup_prerequisites().reads[index].unwrap();assert_eq!((r.flags,r.opcode,r.number,r.reply_bytes),(31,5,1,19));sequences.push(r.record_sequence().unwrap());
   #[cfg(feature="sunfish-startup-reply-coverage")]assert_eq!(r.timeout_ms,1000);
  }
  acknowledge_outgoing_glink(&p,&mut o,&mut irq,&wire);
  if let Some(index)=index{assert_eq!(o.startup_prerequisites().reads[index].unwrap().flags,63);}
 }
 assert!(sequences.windows(2).all(|w|w[0]!=w[1]));let c=o.startup_prerequisites();assert_eq!(c.total,[4,2]);assert_eq!(c.omitted,[if cfg!(feature="sunfish-startup-reply-coverage"){0}else{2},0]);assert!(o.error().is_none());
 let before=p.log();let _=o.startup_prerequisites();assert_eq!(p.log(),before);
 print!("COVERAGE_WIRE ");for b in all{print!("{b:02x}");}println!();println!("COVERAGE_IO {:?}",p.log());
}
