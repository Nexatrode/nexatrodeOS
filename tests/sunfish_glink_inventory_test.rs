// The real status-handshake harness supplies a sealed ready observation;
// secure replies, peer publications and counter values remain synthetic.
include!("sunfish_smp2p_handshake_test.rs");
#[path="../rust/src/pixel_glink_inventory.rs"] mod pixel_glink_inventory;
fn ready_observation(m:&mut[u8])->pixel_smp2p_handshake::ReadyObservation{
    let mut lock=0;let mut door=[0;4];let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};let b=w.before_auth().unwrap();
    let mut a=available();let mut h=unsafe{Handshake::new(m.as_mut_ptr().add(PART),door.as_mut_ptr())};
    assert!(h.take_ready_observation().is_err());h.prepare(&mut w,&b,permit(&mut a)).unwrap();
    assert!(h.take_ready_observation().is_err());remote(m,Some(6));w32(m,PART+408+4,1);
    let r=h.observe(&mut w,b,complete(a),&mut Time{now:0,step:50000},|_|{});assert!(r.status.ready_bit_seen);
    let observation=h.take_ready_observation().unwrap();assert!(h.take_ready_observation().is_err());observation
}
fn allocate(m:&mut[u8],id:u16,size:usize,cached:bool)->qcom_modem_status::Item{
    assert_eq!(size%8,0);
    let (header,data)=if cached{
        let free=u32::from_le_bytes(m[PART+16..PART+20].try_into().unwrap())as usize;
        let header=PART+free-32;let data=header-size;w32(m,PART+16,(data-PART)as u32);(header,data)
    }else{
        let free=u32::from_le_bytes(m[PART+12..PART+16].try_into().unwrap())as usize;
        let header=PART+free;w32(m,PART+12,(free+16+size)as u32);(header,header+16)
    };
    w16(m,header,0xa5a5);w16(m,header+2,id);w32(m,header+4,size as u32);m[data..data+size].fill(0xa7);
    qcom_modem_status::Item{offset:data,size}
}
struct NoPayload<'a>{bytes:&'a[u8],forbidden:Vec<qcom_modem_status::Item>,reads:std::cell::Cell<usize>}
impl Read for NoPayload<'_>{
    fn read8(&self,at:usize)->u8{
        assert!(!self.forbidden.iter().any(|i|(i.offset..i.offset+i.size).contains(&at)),"GLINK payload read at {at:x}");
        self.reads.set(self.reads.get()+1);self.bytes[at]
    }
}
#[test]
fn absent_glink_items_are_valid_directory_results_after_real_ready_gate(){
    let mut m=mapped();let ready=ready_observation(&mut m);let original=m.clone();
    let r=pixel_glink_inventory::inspect(&Bytes(&m),ready).unwrap();
    assert_eq!(r.items,[None;3]);assert_eq!(r.present_mask(),0);assert_eq!(r.metadata_passes,2);assert_eq!(m,original);
}
#[test]
fn both_allocation_lists_allow_32_byte_descriptors_without_reading_any_glink_payload(){
    for cached in [false,true]{
        let mut m=mapped();let ready=ready_observation(&mut m);
        let items=[allocate(&mut m,478,32,cached),allocate(&mut m,479,16384,cached),allocate(&mut m,480,16384,cached)];
        let original=m.clone();let spy=NoPayload{bytes:&m,forbidden:items.to_vec(),reads:std::cell::Cell::new(0)};
        let r=pixel_glink_inventory::inspect(&spy,ready).unwrap();
        assert_eq!(r.items,items.map(Some));assert_eq!(r.present_mask(),7);assert!(spy.reads.get()>0);assert_eq!(m,original);
        // The SMP2P-specific reader must continue rejecting a32-byte payload.
        assert_eq!(qcom_modem_status::directory_for(&Bytes(&m),478),Err(Error::ItemSize));
    }
}
#[test]
fn native_lock_adapter_releases_only_its_semaphore_and_keeps_all_smem_bytes_unchanged(){
    let mut m=mapped();let ready=ready_observation(&mut m);allocate(&mut m,480,16384,false);let original=m.clone();
    let mut locks=[0x12345678,0,0xa5a5a5a5];let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};
    let r=pixel_glink_inventory::read(&mut w,ready).unwrap();assert_eq!(r.present_mask(),4);
    assert_eq!(locks,[0x12345678,0,0xa5a5a5a5]);assert_eq!(m,original);
}
#[test]
fn lost_ready_fatal_ap_tampering_and_changed_peer_identity_reject_the_snapshot(){
    for fault in 0..5{
        let mut m=mapped();let ready=ready_observation(&mut m);
        let expected=match fault{
            0=>{w32(&mut m,PART+408+36,0);Error::ReadyLost},
            1=>{w32(&mut m,PART+408+36,1);Error::Fatal},
            2=>{m[PART+48+20]^=1;Error::Outbound},
            3=>{w16(&mut m,PART+392+2,999);Error::RouteChanged},
            _=>{w32(&mut m,SMEM_BYTES-4096+48,64);Error::RouteChanged},
        };
        let original=m.clone();let r=pixel_glink_inventory::inspect(&Bytes(&m),ready);
        assert!(matches!(r,Err(e)if e==expected));assert_eq!(m,original);
    }
}
#[test]
fn duplicate_glink_ids_or_malformed_allocation_headers_are_not_payloads(){
    for duplicate in [false,true]{
        let mut m=mapped();let ready=ready_observation(&mut m);let item=allocate(&mut m,478,32,false);
        if duplicate{allocate(&mut m,478,32,false);}else{w16(&mut m,item.offset-16,0);}
        let original=m.clone();let result=pixel_glink_inventory::inspect(&Bytes(&m),ready);
        assert!(matches!(result,Err(e)if e==if duplicate{Error::Duplicate}else{Error::Allocation}));assert_eq!(m,original);
    }
}
#[test]
fn different_metadata_snapshots_are_rejected_without_mutating_or_reading_glink_payload(){
    let mut m=mapped();let ready=ready_observation(&mut m);let item=allocate(&mut m,478,32,false);
    struct Changing<'a>{inner:NoPayload<'a>,at:usize,seen:std::cell::Cell<usize>}
    impl Read for Changing<'_>{fn read8(&self,at:usize)->u8{
        let value=self.inner.read8(at);
        if at==self.at{let n=self.seen.get();self.seen.set(n+1);if n>=6{return 8}}
        value
    }}
    // Directory headers stay valid and keep the same allocated extent; only
    // descriptor padding/decoded length changes between the two snapshots.
    let reader=Changing{inner:NoPayload{bytes:&m,forbidden:vec![item],reads:std::cell::Cell::new(0)},
        at:item.offset-16+8,seen:std::cell::Cell::new(0)};
    let original=m.clone();let result=pixel_glink_inventory::inspect(&reader,ready);
    assert!(matches!(result,Err(Error::Unstable)));assert!(reader.seen.get()>=8);assert_eq!(m,original);
}
