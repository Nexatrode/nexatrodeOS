include!("sunfish_glink_inventory_test.rs");
#[path="../rust/src/pixel_glink_receive.rs"] mod pixel_glink_receive;
fn receive_fixture(cached:bool,tail:u32,head:u32)->(Vec<u8>,pixel_smp2p_handshake::ReadyObservation,[qcom_modem_status::Item;3]){
    let mut m=mapped();let ready=ready_observation(&mut m);
    let items=[allocate(&mut m,478,32,cached),allocate(&mut m,479,16384,cached),allocate(&mut m,480,16384,cached)];
    m[items[0].offset..items[0].offset+32].fill(0);w32(&mut m,items[0].offset+8,tail);w32(&mut m,items[0].offset+12,head);
    let header=[0,0,1,0,1,0,0,0];
    for (i,v)in header.into_iter().enumerate(){m[items[2].offset+(tail as usize+i)%16384]=v;}
    (m,ready,items)
}
struct BoundedContent<'a>{m:&'a[u8],items:[qcom_modem_status::Item;3],tail:usize,payload_reads:std::cell::Cell<usize>}
impl Read for BoundedContent<'_>{fn read8(&self,at:usize)->u8{
    for (i,item)in self.items.iter().enumerate(){if(item.offset..item.offset+item.size).contains(&at){
        assert!(i==0 || i==2 && (0..8).any(|n|at==item.offset+(self.tail+n)%16384),"unexpected GLINK payload read at {at:x}");
        self.payload_reads.set(self.payload_reads.get()+1);
    }}self.m[at]
}}
#[test]
fn native_shape_without_ap_tx_reports_version_offer_without_consuming_or_writing(){
    let mut m=mapped();let ready=ready_observation(&mut m);let desc=allocate(&mut m,478,32,false);let rx=allocate(&mut m,480,16384,false);
    m[desc.offset..desc.offset+32].fill(0);w32(&mut m,desc.offset+12,8);m[rx.offset..rx.offset+8].copy_from_slice(&[0,0,1,0,1,0,0,0]);
    let original=m.clone();let mut locks=[0xa5a5,0,0x5a5a];let mut window=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};
    let r=pixel_glink_receive::read(&mut window,ready).unwrap();
    assert_eq!(r.inventory.present_mask(),5);assert_eq!(r.available,8);assert!(r.header.unwrap().version_offer());
    assert_eq!(r.words[..4],[0,0,0,8]);assert_eq!(r.content_passes,2);assert_eq!(m,original);assert_eq!(locks,[0xa5a5,0,0x5a5a]);
}
#[test]
fn cached_and_uncached_rings_peek_only_one_header_including_wrapped_availability(){
    for cached in [false,true]{for(tail,head)in[(0,8),(16376,8)]{
        let(m,ready,items)=receive_fixture(cached,tail,head);let original=m.clone();
        let spy=BoundedContent{m:&m,items,tail:tail as usize,payload_reads:std::cell::Cell::new(0)};
        let r=pixel_glink_receive::inspect(&spy,ready).unwrap();assert!(r.header.unwrap().version_offer());
        assert_eq!(r.available,if tail==0{8}else{16});assert_eq!(spy.payload_reads.get(),2*(32+8));assert_eq!(m,original);
    }}
}
#[test]
fn empty_ring_has_no_header_and_reads_no_fifo_bytes(){
    let(m,ready,items)=receive_fixture(false,8,8);let spy=BoundedContent{m:&m,items,tail:8,payload_reads:std::cell::Cell::new(0)};
    let r=pixel_glink_receive::inspect(&spy,ready).unwrap();assert_eq!(r.available,0);assert_eq!(r.header,None);assert_eq!(spy.payload_reads.get(),64);
}
#[test]
fn other_commands_versions_and_feature_bits_remain_raw_and_never_negotiate(){
    for(command,version,features)in[(1u16,1u16,1u32),(0,2,u32::MAX),(19,0,0),(0,1,u32::MAX)]{
        let(mut m,ready,items)=receive_fixture(false,0,8);let at=items[2].offset;
        w16(&mut m,at,command);w16(&mut m,at+2,version);w32(&mut m,at+4,features);let original=m.clone();
        let r=pixel_glink_receive::inspect(&Bytes(&m),ready).unwrap();let h=r.header.unwrap();
        assert_eq!((h.command,h.param1,h.param2),(command,version,features));assert_eq!(h.version_offer(),command==0 && version==1);assert_eq!(m,original);
    }
}
#[test]
fn invalid_indices_stop_before_any_fifo_read_and_do_not_write(){
    for index in 0..4{for value in [1,7,0x4000,u32::MAX]{
        let(mut m,ready,items)=receive_fixture(false,0,8);w32(&mut m,items[0].offset+index*4,value);let original=m.clone();
        let spy=NoPayload{bytes:&m,forbidden:vec![items[1],items[2]],reads:std::cell::Cell::new(0)};
        assert!(matches!(pixel_glink_receive::inspect(&spy,ready),Err(Error::Bounds)));assert_eq!(m,original);
    }}
}
#[test]
fn missing_or_wrong_size_allocations_are_rejected_before_contents(){
    for fault in 0..5{
        let(mut m,ready,items)=receive_fixture(false,0,8);
        match fault{0=>w16(&mut m,items[0].offset-16+2,999),1=>w16(&mut m,items[2].offset-16+2,999),
            2=>w16(&mut m,items[0].offset-16+8,8),3=>w16(&mut m,items[2].offset-16+8,8),_=>w16(&mut m,items[1].offset-16+8,8)};
        let spy=NoPayload{bytes:&m,forbidden:items.to_vec(),reads:std::cell::Cell::new(0)};let original=m.clone();
        assert!(matches!(pixel_glink_receive::inspect(&spy,ready),Err(Error::ItemSize)));assert_eq!(m,original);
    }
}
#[test]
fn descriptor_and_pending_header_mutation_between_reads_reject_without_advancing_tail(){
    struct Changing<'a>{bytes:&'a[u8],at:usize,seen:std::cell::Cell<usize>,replacement:u8}
    impl Read for Changing<'_>{fn read8(&self,at:usize)->u8{if at==self.at{let n=self.seen.get();self.seen.set(n+1);if n>0{return self.replacement}}self.bytes[at]}}
    for fault in 0..3{
        let(m,ready,items)=receive_fixture(false,0,8);let(at,replacement)=match fault{0=>(items[0].offset+12,16),1=>(items[0].offset+16,1),_=>(items[2].offset+4,3)};
        let original=m.clone();let spy=Changing{bytes:&m,at,seen:std::cell::Cell::new(0),replacement};
        assert!(matches!(pixel_glink_receive::inspect(&spy,ready),Err(Error::Unstable)));assert_eq!(m,original);
    }
}
#[test]
fn receive_snapshot_revalidates_the_same_ready_and_ap_identity(){
    for fault in 0..3{
        let(mut m,ready,_)=receive_fixture(false,0,8);
        let expected=match fault{0=>{w32(&mut m,PART+408+36,0);Error::ReadyLost},1=>{w32(&mut m,PART+408+36,1);Error::Fatal},_=>{m[PART+48+20]^=1;Error::Outbound}};
        let original=m.clone();assert!(matches!(pixel_glink_receive::inspect(&Bytes(&m),ready),Err(e)if e==expected));assert_eq!(m,original);
    }
}
#[test]
fn ready_lost_after_header_reads_fails_the_final_gate(){
    let(m,ready,items)=receive_fixture(false,0,8);
    struct Revoked<'a>{m:&'a[u8],last_header_byte:usize,headers:std::cell::Cell<usize>}
    impl Read for Revoked<'_>{fn read8(&self,at:usize)->u8{
        if at==self.last_header_byte{self.headers.set(self.headers.get()+1);}
        if at==PART+408+36 && self.headers.get()>=2{return 0}self.m[at]
    }}
    let spy=Revoked{m:&m,last_header_byte:items[2].offset+7,headers:std::cell::Cell::new(0)};let original=m.clone();
    assert!(matches!(pixel_glink_receive::inspect(&spy,ready),Err(Error::ReadyLost)));assert_eq!(spy.headers.get(),2);assert_eq!(m,original);
}
