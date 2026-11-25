// Included inside post_mcfg_cases: actual Owner/FIFO helpers and RAM server.
use crate::qcom_glink_qmi::MAX_REQUESTS;
use std::sync::atomic::{AtomicU64,Ordering::Relaxed};
fn mc_owner()->(Fake,Owner<Fake>,Irqs){
    static CLOCK:AtomicU64=AtomicU64::new(100);
    let(p,mut o)=owner();let mut i=Irqs::default();o.set_rmtfs_event_clock(||CLOCK.fetch_add(1,Relaxed));o.prepare_tftp_server(0x5000).unwrap();o.prepare_service_registry().unwrap();o.start_control().unwrap();
    let mut adv=control(5,2,16);for id in 0u32..16{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}p.push(&adv);o.notified(8,&mut i).unwrap();p.take_tx();owner_control_peer_hello(&p,&mut o,&mut i);take_owner_datagrams(&p,&mut o,&mut i);(p,o,i)
}
fn mc_offer(p:&Fake,o:&mut Owner<Fake>,i:&mut Irqs)->u32{let id=o.offer_receive().unwrap();o.notified(8,i).unwrap();p.take_tx();id}
fn mc_input(p:&Fake,o:&mut Owner<Fake>,i:&mut Irqs,peer:u32,port:u32,b:&[u8]){
    let id=mc_offer(p,o,i);p.push(&data(id,&owner_qrtr_packet(1,peer,port,false,b)));o.notified(1,i).unwrap();
}
fn mc_close(p:&Fake,o:&mut Owner<Fake>,i:&mut Irqs,peer:u32){let id=mc_offer(p,o,i);p.push(&data(id,&owner_control_frame(6,[0,peer,0,0])));o.notified(1,i).unwrap();}
fn mc_file()->qcom_tftp_server_check::ServerCheck{let mut s=qcom_tftp_server_check::ServerCheck::new();commit_one(&mut s,&mut[0;128]);s}
#[test]
fn mcfg_close_held_requests_return_normally_without_late_oack(){
    let(p,mut o,mut i)=mc_owner();let mut server=mc_file();
    mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);
    mc_close(&p,&mut o,&mut i,0x4a);assert_eq!(o.pending_mcfg_close(),Some(qcom_qmi_router::Address{node:0,port:0x4a}));
    assert!(o.dispatch_service_registry_step(&mut i).is_err()); // paused, no service may pass the close
    o.resolve_mcfg_close(&mut server).unwrap();let t=o.mcfg_close_trace();assert_eq!((t.observed,t.committed,t.held,t.retained,t.unsent,t.published),(1,1,2,0,0,0));
    assert!(o.pending_tftp_request().is_none());assert_eq!(server.mcfg_progress().1,Some(1));
    o.notified(8,&mut i).unwrap();assert!(take_owner_datagrams(&p,&mut o,&mut i).is_empty());
    mc_input(&p,&mut o,&mut i,0x4b,0x5000,STAT);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();assert_eq!(take_owner_datagrams(&p,&mut o,&mut i).len(),1);
    let late=mc_offer(&p,&mut o,&mut i);p.push(&data(late,&owner_qrtr_packet(1,0x4a,0x5000,false,STAT)));assert!(o.notified(1,&mut i).is_err(),"closed ports retain the existing generation fence");
}
#[test]
fn mcfg_close_cancels_unsent_reply_and_preserves_file_and_port(){
    let(p,mut o,mut i)=mc_owner();let mut server=mc_file();mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);let close=mc_offer(&p,&mut o,&mut i);
    o.service_tftp_server_check_step(&mut server,&mut i).unwrap();
    p.push(&data(close,&owner_control_frame(6,[0,0x4a,0,0])));o.notified(1,&mut i).unwrap();o.resolve_mcfg_close(&mut server).unwrap();
    let t=o.mcfg_close_trace();assert_eq!((t.committed,t.unsent,t.published),(1,1,0));o.notified(8,&mut i).unwrap();assert!(take_owner_datagrams(&p,&mut o,&mut i).is_empty());
    let mut output=[0;128];let route=qcom_tftp_server_check::Route{source:qcom_tftp_server_check::Address{node:0,port:0x4a},destination:qcom_tftp_server_check::Address{node:1,port:0x5000}};
    assert!(server.receive(route,STAT,&mut output).is_err());assert_eq!(server.mcfg_progress().1,Some(1));
}
#[test]
fn mcfg_close_retained_retry_keeps_published_original_until_real_receipt(){
    let(p,mut o,mut i)=mc_owner();let mut server=mc_file();mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();o.notified(8,&mut i).unwrap();
    let records=p.take_tx();let first=owner_datagrams(&records);assert_eq!(first.len(),1);
    mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);o.service_tftp_server_check_step(&mut server,&mut i).unwrap();
    mc_close(&p,&mut o,&mut i,0x4a);o.resolve_mcfg_close(&mut server).unwrap();let t=o.mcfg_close_trace();assert_eq!((t.retained,t.unsent,t.published),(1,0,1));
    assert!(!o.tftp_queue_snapshot().1);assert!(o.error().is_none());
    acknowledge_outgoing_glink(&p,&mut o,&mut i,&records);o.notified(8,&mut i).unwrap();assert!(take_owner_datagrams(&p,&mut o,&mut i).is_empty());
    assert_eq!(o.tftp_tx_progress()[2],1,"only the actual published reply completed");
}
#[test]
fn mcfg_close_mixed_request_and_wrong_server_fail_closed_before_cleanup(){
    for wrong_server in [false,true]{
        let(p,mut o,mut i)=mc_owner();let mut server=mc_file();mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);
        if wrong_server{o.service_tftp_server_check_step(&mut server,&mut i).unwrap();}
        else{mc_input(&p,&mut o,&mut i,0x4a,0x5000,qcom_tftp_server_check::REQUEST);}
        mc_close(&p,&mut o,&mut i,0x4a);
        let result=if wrong_server{o.resolve_mcfg_close(&mut qcom_tftp_server_check::ServerCheck::new())}else{o.resolve_mcfg_close(&mut server)};
        assert!(result.is_err());assert!(o.error().is_some());assert_eq!(o.mcfg_close_trace().committed,0);assert_eq!(server.mcfg_progress().1,Some(1));
    }
}
#[test]
fn mcfg_close_live_gate_fault_prevents_commit_and_wrong_peer_does_not_cancel(){
    for fault in 0..3 {
        let(p,mut o,mut i)=mc_owner();let mut server=mc_file();mc_input(&p,&mut o,&mut i,0x4a,0x5000,STAT);
        mc_close(&p,&mut o,&mut i,0x4b);assert!(o.pending_mcfg_close().is_none());assert!(o.pending_tftp_request().is_some());
        mc_close(&p,&mut o,&mut i,0x4a);let before=p.log().len();
        match fault {0=>p.0.borrow_mut().observation.ready=false,1=>p.0.borrow_mut().fail=Some("snapshot"),_=>p.0.borrow_mut().words[3]=1}
        assert!(o.resolve_mcfg_close(&mut server).is_err());assert!(o.error().is_some());assert_eq!(o.mcfg_close_trace().committed,0);assert_eq!(server.mcfg_progress().1,Some(1));
        assert!(!p.log()[before..].iter().any(|op|matches!(op,Op::Write(..)|Op::Head(_)|Op::Tail(_)|Op::Notify)));
    }
}
#[test]
fn mcfg_close_aborts_staging_without_replacing_committed_file(){
    let(p,mut o,mut i)=mc_owner();let mut server=mc_file();let mut output=[0;1024];let peer=qcom_tftp_server_check::Address{node:0,port:0x4a};
    let mut route=qcom_tftp_server_check::Route{source:peer,destination:qcom_tftp_server_check::Address{node:1,port:0x5000}};
    let write=b"\0\x02/readwrite/mcfg.tmp\0octet\0blksize\x007680\0timeoutms\x001000\0wsize\x0010\0";
    route.destination.port=server.receive(route,write,&mut output).unwrap().route.source.port;
    let mut block=vec![0,3,0,1];block.extend([b'Y';512]);server.receive(route,&block,&mut output).unwrap();assert_eq!(server.mcfg_progress().1,Some(1));
    o.service_tftp_server_check_step(&mut server,&mut i).unwrap();mc_close(&p,&mut o,&mut i,0x4a);o.resolve_mcfg_close(&mut server).unwrap();
    assert!(server.receive(route,&block,&mut output).is_err());assert_eq!(server.mcfg_progress().1,Some(1));
    let mut r=route;r.source.port=0x4b;r.destination.port=0x5000;let port=server.receive(r,STAT,&mut output).unwrap().route.source.port;r.destination.port=port;
    let reply=server.receive(r,b"\0\x04\0\0",&mut output).unwrap();assert_eq!(&output[..reply.len],b"\0\x03\0\x01X");
    r.source.port=0x4c;r.destination.port=0x5000;assert!(server.receive(r,write,&mut output).is_ok(),"aborted staging releases writer ownership");
}
fn mc_session()->(Session,Vec<u8>){
    let mut s=session();let mut wire=Vec::new();s.prepare_tftp_server(1,0x5000).unwrap();s.start_control(1).unwrap();
    let mut adv=control(5,2,16);for id in 7u32..23{adv.extend(4096u32.to_le_bytes());adv.extend(id.to_le_bytes());}s.ingest(1,&adv).unwrap();drain(&mut s,&mut wire);let id=offer(&mut s,&mut wire);s.ingest(1,&data(id,&owner_control_frame(2,[0;4]))).unwrap();drain(&mut s,&mut wire);(s,wire)
}
#[test]
fn mcfg_close_partial_publication_finishes_with_original_intent_and_receipt(){
    let(mut s,mut wire)=mc_session();let close=offer(&mut s,&mut wire);let peer=qcom_qmi_router::Address{node:0,port:0x4a};let mut known=[None;qcom_tftp_server_check::TRANSFER_COUNT];known[0]=Some((peer,0x5009));s.set_mcfg_close_transfers(1,known).unwrap();
    let mut payload=vec![0,3,0,1];payload.extend([b'X';512]);let route=qcom_glink_qmi::TftpReplyRoute{source:qcom_qmi_router::Address{node:1,port:0x5009},destination:peer};s.queue_tftp_response(1,route,&payload).unwrap();
    let mut bytes=Vec::new();let mut publish=|s:&mut Session|s.publish_next(1,|r|{let cmd=u16::from_le_bytes([r[0],r[1]]);if matches!(cmd,9|12){let n=lo_word(r,8)as usize;bytes.extend_from_slice(&r[16..16+n]);}Ok(())}).unwrap();
    let remote=loop {if let Some(Published::Data{remote_intent,last_fragment:false})=publish(&mut s){break remote_intent;}};
    assert_eq!(s.ingest(1,&data(close,&owner_control_frame(6,[0,0x4a,0,0]))).unwrap(),Event::McfgClosePending{peer});
    assert_eq!(s.commit_mcfg_close(2,peer,Some(0x5009),&[None;MAX_REQUESTS]),Err(qcom_glink_qmi::Error::StaleGeneration));
    let out=s.commit_mcfg_close(1,peer,Some(0x5009),&[None;MAX_REQUESTS]).unwrap();assert_eq!((out.unsent,out.published),(0,1));
    for _ in 0..32 {if let Some(Published::Data{remote_intent,last_fragment:true})=publish(&mut s){assert_eq!(remote_intent,remote);break;}}
    drop(publish);assert_eq!(lo_word(&bytes,20)as usize,payload.len());assert_eq!(&bytes[32..32+payload.len()],payload);assert_eq!(bytes.len(),32+payload.len());
    assert_eq!(s.ingest(1,&control(14,2,remote)).unwrap(),Event::TransmitDone);assert_eq!(s.tftp_tx_progress()[2],1);
}
#[test]
fn mcfg_close_keeps_qrtr_credit_and_returns_held_request_through_normal_writer(){
    let(mut s,mut wire)=mc_session();let id=offer(&mut s,&mut wire);let close=offer(&mut s,&mut wire);let peer=qcom_qmi_router::Address{node:0,port:0x4a};
    let Event::TftpRequest(h)=s.ingest(1,&data(id,&owner_qrtr_packet(1,0x4a,0x5000,true,STAT))).unwrap()else{panic!("request");};
    assert_eq!(s.ingest(1,&data(close,&owner_control_frame(6,[0,0x4a,0,0]))).unwrap(),Event::McfgClosePending{peer});
    let mut held=[None;MAX_REQUESTS];held[0]=Some(h);let out=s.commit_mcfg_close(1,peer,None,&held).unwrap();assert_eq!((out.request_mask,out.receives,out.credits,out.unsent),(1,1,1,0));
    let mut returned=false;let mut credit=false;
    for _ in 0..32 {let before=wire.len();match publish(&mut s,&mut wire){Some(Published::ReceiveDone(i)) if i==id=>returned=true,Some(Published::Data{remote_intent,..})=>{let b=&wire[before+16..];assert_eq!((lo_word(b,4),lo_word(b,12),lo_word(b,28)),(7,0x5000,0x4a));credit=true;s.ingest(1,&control(14,2,remote_intent)).unwrap();},None=>break,_=>{}}}
    assert!(returned&&credit);assert!(s.tftp_datagram(&h).is_err());
}

#[test]
fn mcfg_close_fence_capacity_exhaustion_never_releases_uncommitted_request(){
    let(mut s,mut wire)=mc_session();
    for n in 0..65 {
        let id=offer(&mut s,&mut wire);let close=offer(&mut s,&mut wire);let peer=qcom_qmi_router::Address{node:0,port:0x100+n};
        let Event::TftpRequest(h)=s.ingest(1,&data(id,&owner_qrtr_packet(1,peer.port,0x5000,false,STAT))).unwrap()else{panic!("request");};
        assert_eq!(s.ingest(1,&data(close,&owner_control_frame(6,[0,peer.port,0,0]))).unwrap(),Event::McfgClosePending{peer});
        let mut held=[None;MAX_REQUESTS];held[0]=Some(h);
        if n==64 {assert_eq!(s.commit_mcfg_close(1,peer,None,&held),Err(qcom_glink_qmi::Error::NoCapacity));assert!(s.publish_next(1,|_|panic!("quarantined writer")).is_err());}
        else {s.commit_mcfg_close(1,peer,None,&held).unwrap();drain(&mut s,&mut wire);}
    }
}

#[test]
fn mcfg_close_normal_return_backpressure_retries_exactly_and_uncertainty_quarantines(){
    for uncertain in [false,true] {
        let(mut s,mut wire)=mc_session();let id=offer(&mut s,&mut wire);let close=offer(&mut s,&mut wire);let peer=qcom_qmi_router::Address{node:0,port:0x4a};
        let Event::TftpRequest(h)=s.ingest(1,&data(id,&owner_qrtr_packet(1,0x4a,0x5000,false,STAT))).unwrap()else{panic!("request");};
        s.ingest(1,&data(close,&owner_control_frame(6,[0,0x4a,0,0]))).unwrap();let mut held=[None;MAX_REQUESTS];held[0]=Some(h);s.commit_mcfg_close(1,peer,None,&held).unwrap();
        let mut record=Vec::new();let result=s.publish_next(1,|r|{record=r.to_vec();Err(if uncertain {qcom_glink_qmi::WriteError::Uncertain}else{qcom_glink_qmi::WriteError::WouldBlock})});
        assert!(!record.is_empty());
        if uncertain {assert_eq!(result,Err(qcom_glink_qmi::Error::PublicationUncertain));assert!(s.publish_next(1,|_|panic!("uncertain publication must not retry")).is_err());}
        else {assert_eq!(result,Err(qcom_glink_qmi::Error::WouldBlock));let mut retry=Vec::new();assert!(s.publish_next(1,|r|{retry=r.to_vec();Ok(())}).unwrap().is_some());assert_eq!(record,retry);drain(&mut s,&mut wire);}
    }
}
