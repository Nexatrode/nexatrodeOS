pub(crate) fn idle_fixture(slack:Option<usize>)->(Adapter<crate::Fake>,crate::Fake,core::TftpHandle,u32){
 let p=crate::Fake::new(0,0);let mut s=crate::session();s.prepare_tftp_server(1,0x5000).unwrap();let mut a=Adapter::attach(p.clone(),s,1).unwrap();a.session.start_control(1).unwrap();
 let mut adv=crate::control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}a.session.ingest(1,&adv).unwrap();drain(&mut a,&p);
 let hello=a.session.offer_receive(1).unwrap();drain(&mut a,&p);a.session.ingest(1,&crate::data(hello,&crate::owner_control_frame(2,[0;4]))).unwrap();drain(&mut a,&p);
 let id=a.session.offer_receive(1).unwrap();let close=a.session.offer_receive(1).unwrap();drain(&mut a,&p);let Event::TftpRequest(h)=a.session.ingest(1,&crate::data(id,&crate::owner_qrtr_packet(1,0x4a,0x5000,false,b"\0\x01/readwrite/mcfg.tmp\0octet\0"))).unwrap()else{panic!("request");};
 while a.session.offer_receive(1).is_ok(){}drain(&mut a,&p);assert_eq!(a.session.ingest(1,&crate::control(7,2,52)).unwrap(),Event::ReceiveIntentRequest{granted:false});a.session.finish_tftp_request(&h).unwrap();
 let route=core::TftpReplyRoute{source:crate::qcom_qmi_router::Address{node:1,port:0x5006},destination:crate::qcom_qmi_router::Address{node:0,port:0x4a}};a.session.queue_tftp_response(1,route,crate::qcom_tftp_server_check::MISSING_REPLY).unwrap();p.take_tx();if let Some(slack)=slack{for _ in 0..(FIFO_BYTES-slack)/8{a.ring.write(&[13,0,0,0,0,0,0,0],0).unwrap();}}(a,p,h,close)
}
