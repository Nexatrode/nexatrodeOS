include!("sunfish_qrtr_wait_test.rs");
#[path="../rust/src/qcom_modem_crash.rs"] mod qcom_modem_crash;
#[path="../rust/src/pixel_modem_crash.rs"] mod pixel_modem_crash;
const GLOBAL:usize=0x20000;
const GSIZE:usize=0x4000;
fn crash_fixture()->Vec<u8>{
    let mut m=fixture();let t=SMEM_BYTES-4096;w32(&mut m,t+8,2);
    let e=t+80;w32(&mut m,e,GLOBAL as u32);w32(&mut m,e+4,GSIZE as u32);
    w16(&mut m,e+12,0xfffe);w16(&mut m,e+14,0xfffe);w32(&mut m,e+16,32);
    w32(&mut m,GLOBAL,0x54525024);w16(&mut m,GLOBAL+4,0xfffe);w16(&mut m,GLOBAL+6,0xfffe);
    w32(&mut m,GLOBAL+8,GSIZE as u32);w32(&mut m,GLOBAL+12,32);w32(&mut m,GLOBAL+16,GSIZE as u32);m
}
fn crash_publish(m:&mut[u8],cached:bool,size:usize)->usize{
    let h=if cached{GLOBAL+GSIZE-32}else{GLOBAL+32};let n=(size+7)&!7;
    w16(m,h,0xa5a5);w16(m,h+2,421);w32(m,h+4,n as u32);w16(m,h+8,(n-size)as u16);
    let at=if cached{h-n}else{h+16};
    if cached{w32(m,GLOBAL+16,(at-GLOBAL)as u32)}else{w32(m,GLOBAL+12,(at+n-GLOBAL)as u32)}at
}
#[test]
fn crash_reason_uses_only_global_partition_and_never_the_private_host_item(){
    let mut m=crash_fixture();publish(&mut m,6,false);w16(&mut m,0x10022,421);
    let s=qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();assert_eq!(s.directory.item,None);
    for hosts in [(0,1),(1,0),(0xfffe,1)]{
        let mut b=m.clone();let e=SMEM_BYTES-4096+80;w16(&mut b,e+12,hosts.0);w16(&mut b,e+14,hosts.1);
        assert_eq!(qcom_modem_crash::snapshot(&Bytes(&b)),Err(Error::Route));
    }
}
#[test]
fn crash_reason_reads_both_lists_and_distinguishes_present_empty_and_missing(){
    for cached in [false,true]{
        let mut m=crash_fixture();assert_eq!(qcom_modem_crash::snapshot(&Bytes(&m)).unwrap().copied,0);
        let at=crash_publish(&mut m,cached,256);
        let empty=qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();assert!(empty.terminated);assert_eq!(empty.text_len,0);
        m[at..at+18].copy_from_slice(b"modem fatal: test\0");
        let s=qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();
        assert_eq!(s.text_len,17);assert_eq!(s.directory.item.unwrap().offset,at);assert!(qcom_modem_crash::changed(&empty,&s));
        assert_eq!(&qcom_modem_crash::text_row(&s,0)[..17],b"MODEM FATAL  TEST");
    }
}
#[test]
fn crash_directory_rejects_bad_global_headers_counts_and_routes(){
    let m=crash_fixture();let t=SMEM_BYTES-4096;
    for(at,v)in[(192,0),(92,11<<16),(t,0),(t+8,81),(t+80,u32::MAX),(t+84,u32::MAX),
        (t+80,0x10000),(GLOBAL,0),(GLOBAL+8,1),(GLOBAL+12,31),(GLOBAL+16,GSIZE as u32+1)]{
        let mut b=m.clone();w32(&mut b,at,v);assert!(qcom_modem_crash::snapshot(&Bytes(&b)).is_err(),"{at:x}");
    }
    let mut b=m.clone();w16(&mut b,GLOBAL+4,0);w16(&mut b,GLOBAL+6,1);assert_eq!(qcom_modem_crash::snapshot(&Bytes(&b)),Err(Error::Partition));
    let mut b=m.clone();w16(&mut b,t+44,0xfffe);w16(&mut b,t+46,0xfffe);assert_eq!(qcom_modem_crash::snapshot(&Bytes(&b)),Err(Error::Route));
    // A non-global host-pair route is not followed while reading the global
    // heap, even if its range aliases the global route's range.
    let mut b=m.clone();w16(&mut b,t+44,2);w16(&mut b,t+46,3);w32(&mut b,t+32,GLOBAL as u32);w32(&mut b,t+36,GSIZE as u32);
    assert!(qcom_modem_crash::snapshot(&Bytes(&b)).is_ok());
}
#[test]
fn crash_bad_allocation_padding_canary_and_cached_alignment_are_rejected(){
    for cached in [false,true]{
        let mut m=crash_fixture();crash_publish(&mut m,cached,256);let h=if cached{GLOBAL+GSIZE-32}else{GLOBAL+32};
        for(off,v)in[(0,0),(4,u32::MAX),(8,256)]{
            let mut b=m.clone();w32(&mut b,h+off,v);assert!(qcom_modem_crash::snapshot(&Bytes(&b)).is_err());
        }
        if cached {w32(&mut m,SMEM_BYTES-4096+96,24)}else{w16(&mut m,h+10,0xffff)}
        assert!(qcom_modem_crash::snapshot(&Bytes(&m)).is_err());
    }
}
#[test]
fn crash_duplicate_items_across_cached_and_uncached_lists_are_rejected(){
    let mut m=crash_fixture();crash_publish(&mut m,false,256);crash_publish(&mut m,true,256);
    assert_eq!(qcom_modem_crash::snapshot(&Bytes(&m)),Err(Error::Duplicate));
}
#[test]
fn crash_capture_is_bounded_for_unterminated_large_and_short_items(){
    for size in [1,64,256,1024,4096]{
        let mut m=crash_fixture();let at=crash_publish(&mut m,false,size);m[at..at+size].fill(b'x');
        let s=qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();
        assert_eq!(s.copied,size.min(256));assert_eq!(s.text_len,size.min(256));assert!(!s.terminated);assert_eq!(s.truncated,size>256);
        assert_eq!(qcom_modem_crash::text_row(&s,usize::MAX),[b' ';24]);
    }
    let mut m=crash_fixture();crash_publish(&mut m,false,4104);assert_eq!(qcom_modem_crash::snapshot(&Bytes(&m)),Err(Error::ItemSize));
}
#[test]
fn crash_reason_prefix_comparison_does_not_claim_old_text_is_new(){
    let mut m=crash_fixture();let at=crash_publish(&mut m,false,256);m[at..at+5].copy_from_slice(b"old!\0");
    let before=qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();let after=qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();
    assert!(!qcom_modem_crash::changed(&before,&after));m[at]=b'n';
    assert!(qcom_modem_crash::changed(&before,&qcom_modem_crash::snapshot(&Bytes(&m)).unwrap()));
}
#[test]
fn crash_unstable_payload_and_changed_directory_fail_closed(){
    use std::cell::Cell;
    struct Change{m:Vec<u8>,at:usize,count:Cell<usize>}
    impl Read for Change{fn read8(&self,at:usize)->u8{
        if at==self.at{self.count.set(self.count.get()+1)}
        self.m[at]^u8::from(at==self.at && self.count.get()>1)
    }}
    for metadata in [false,true]{
        let mut m=crash_fixture();let data=crash_publish(&mut m,false,256);
        let c=Change{m,at:if metadata{GLOBAL+12}else{data},count:Cell::new(0)};
        assert!(qcom_modem_crash::snapshot(&c).is_err());
    }
}
#[test]
fn crash_snapshot_and_status_after_fatal_have_no_smem_or_neighbor_writes(){
    let mut m=crash_fixture();let at=crash_publish(&mut m,false,256);m[at..at+6].copy_from_slice(b"fatal\0");publish(&mut m,1,false);
    let before=m.clone();let mut locks=[0x12345678,0,0xabcdef01];
    let mut w=unsafe{Window::new(m.as_ptr(),locks.as_mut_ptr().add(1))};
    let s=pixel_modem_crash::capture(&mut w).unwrap();assert_eq!(s.text_len,5);
    assert!(pixel_modem_crash::status(&mut w).unwrap().unwrap().fatal_bit());
    assert_eq!(m,before);assert_eq!(locks,[0x12345678,0,0xabcdef01]);
}
#[test]
fn crash_text_sanitization_is_read_only_bounded_and_non_executable(){
    let mut m=crash_fixture();let at=crash_publish(&mut m,false,256);
    m[at..at+12].copy_from_slice(b"ab\n$x\xff!09_z\0");let s=qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();
    assert_eq!(&qcom_modem_crash::text_row(&s,0)[..11],b"AB  X  09 Z");
    assert!(qcom_modem_crash::text_row(&s,10).iter().all(|b|*b==b' '));
}
#[test]
fn crash_fatal_wait_does_not_publish_a_followup_or_consume_receive_bytes(){
    let mut first=pixel_glink_rx::Buffer::new();let mut second=pixel_glink_rx::Buffer::new();
    let (mut m,cap,rx)=discovery_fixture(&mut first,&mut second,0x42,false);
    let mut buffers=pool();let mut lock=0;let mut door=[0;4];
    struct FatalPeer(WaitPeer);
    impl Clock for FatalPeer {
        fn frequency(&self)->u64{self.0.frequency()}
        fn now(&mut self)->u64{
            let n=self.0.now();
            if n>=25_000_000 {unsafe {let m=std::slice::from_raw_parts_mut(self.0.base,SMEM_BYTES);let item=smem_item(m,435).unwrap();w32(m,item.offset+36,1)}}
            n
        }
    }
    let mut clock=FatalPeer(WaitPeer::new(&mut m,rx.offset));
    // The existing readiness-loss path is terminal; diagnostic reads cannot
    // create a continuation or recover/reuse any protocol capability.
    let mut w=unsafe{Window::new(m.as_ptr(),&mut lock)};
    let mut once=unsafe{pixel_qrtr_discovery::DiscoveryOnce::wait(cap,&mut buffers,m.as_mut_ptr().add(PART),door.as_mut_ptr())};
    let r=once.run(&mut w,&mut clock,|_|{});assert_eq!(r.error,Some(Error::Fatal));
    assert!(r.status_samples>0);
    assert_eq!(r.first_status,6);
    assert_eq!(r.first_fatal_status,1);
    assert!(r.first_fatal_ms>=25_000);
    assert!(r.status_changes>0);
    let before=m.clone();let door_before=door;let _=pixel_modem_crash::capture(&mut w);let _=pixel_modem_crash::status(&mut w);
    assert_eq!(m,before);assert_eq!(door,door_before);assert_eq!(once.run(&mut w,&mut clock,|_|{}).error,Some(Error::Outbound));
}

#[test]
fn global_partition_header_accepts_either_host_order_as_the_kernel_does(){
    for pair in [(0xfffe,0xfffe),(0xfffe,0),(0,0xfffe)] {
        let mut m=crash_fixture();w16(&mut m,GLOBAL+4,pair.0);w16(&mut m,GLOBAL+6,pair.1);
        assert_eq!(qcom_modem_crash::snapshot(&Bytes(&m)).unwrap().directory.route.offset,GLOBAL);
    }
    for pair in [(0,1),(1,0),(0,0)] {
        let mut m=crash_fixture();w16(&mut m,GLOBAL+4,pair.0);w16(&mut m,GLOBAL+6,pair.1);
        assert_eq!(qcom_modem_crash::snapshot(&Bytes(&m)),Err(Error::Partition));
    }
}

#[test]
fn partition_failure_stage_is_classified_without_reading_item_data(){
    let m=crash_fixture();assert_eq!(qcom_modem_crash::partition_stage(&Bytes(&m)),0);
    let mut b=m.clone();w32(&mut b,SMEM_BYTES-4096+80,0);assert_eq!(qcom_modem_crash::partition_stage(&Bytes(&b)),1);
    let mut b=m.clone();w32(&mut b,SMEM_BYTES-4096+32,GLOBAL as u32);w32(&mut b,SMEM_BYTES-4096+36,GSIZE as u32);assert_eq!(qcom_modem_crash::partition_stage(&Bytes(&b)),0);
    let mut b=m.clone();w32(&mut b,GLOBAL,0);assert_eq!(qcom_modem_crash::partition_stage(&Bytes(&b)),2);
    let mut b=m.clone();w32(&mut b,GLOBAL+12,31);assert_eq!(qcom_modem_crash::partition_stage(&Bytes(&b)),3);
}

#[test]
fn crash_evidence_requires_a_baseline_before_claiming_change() {
    let m = crash_fixture();
    let baseline = qcom_modem_crash::snapshot(&Bytes(&m)).unwrap();
    let mut changed = m.clone();
    let at = crash_publish(&mut changed, false, 16);
    changed[at..at+5].copy_from_slice(b"FAIL\0");
    let fatal = qcom_modem_crash::snapshot(&Bytes(&changed)).unwrap();
    assert_eq!(qcom_modem_crash::evidence_changed(None, Some(Ok(fatal))), None);
    assert_eq!(qcom_modem_crash::evidence_changed(Some(Ok(baseline)), Some(Ok(fatal))), Some(true));
    assert_eq!(qcom_modem_crash::evidence_changed(Some(Ok(fatal)), Some(Ok(fatal))), Some(false));
}
