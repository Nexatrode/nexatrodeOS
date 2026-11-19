include!("sunfish_glink_channel_test.rs");
#[path="../rust/src/pixel_glink_intents.rs"] mod pixel_glink_intents;
fn add_intents(b:&mut Vec<u8>,pairs:&[(u32,u32)]){b.extend_from_slice(&[5,0,2,0]);b.extend_from_slice(&(pairs.len()as u32).to_le_bytes());for(size,id)in pairs{b.extend_from_slice(&size.to_le_bytes());b.extend_from_slice(&id.to_le_bytes());}}
fn full_control_fixture()->Vec<u8>{let mut b=vec![];for id in 1..=30{add_intents(&mut b,&[(if id<=10{0x800}else if id<=20{0x2000}else{0x4400},id)]);}b.extend_from_slice(&[15,0,2,0,0,0,0,0x80]);assert_eq!(b.len(),488);b}
fn intent_report()->pixel_glink_channel::Report{pixel_glink_channel::Report{error:None,staged:true,head_store:true,notifications:1,tx_consumed:false,words:[0;8],available:0,new_bytes:0,header:None,records:0,intent_records:0,intent_entries:0,samples:0,byte_writes:8,guard_stage:8,intents:None,intent_scan:None}}
#[test]
fn full_488_byte_synthetic_queue_collects_sizes_ids_and_signals(){
    let i=pixel_glink_intents::parse(&full_control_fixture()).unwrap();assert_eq!((i.records,i.intent_records,i.entries),(31,30,30));assert_eq!((i.group_count,i.min_id,i.max_id,i.duplicate_ids,i.zero_sizes),(3,1,30,0,0));assert_eq!(i.groups[..3],[pixel_glink_intents::Group{size:0x800,count:10},pixel_glink_intents::Group{size:0x2000,count:10},pixel_glink_intents::Group{size:0x4400,count:10}]);assert_eq!((i.signals,i.signal_bits),(1,0x80000000));
}
#[test]
fn zero_sizes_duplicate_ids_and_extreme_metadata_are_only_diagnostics(){
    let mut b=vec![];add_intents(&mut b,&[(0,0),(u32::MAX,u32::MAX),(0,u32::MAX)]);let i=pixel_glink_intents::parse(&b).unwrap();assert_eq!((i.entries,i.min_id,i.max_id,i.duplicate_ids,i.zero_sizes),(3,0,u32::MAX,1,2));assert_eq!(i.group_count,2);
}
#[test]
fn grouped_intents_and_read_notifications_share_checked_record_bounds(){
    let mut b=vec![];add_intents(&mut b,&[(8,4),(8,5),(16,6)]);b.extend_from_slice(&[13,0,0,0,0,0,0,0]);let i=pixel_glink_intents::parse(&b).unwrap();assert_eq!((i.records,i.intent_records,i.entries,i.read_notifications),(2,1,3,1));assert_eq!(i.groups[0].count,2);
    for cut in 1..b.len(){if cut!=32{assert!(pixel_glink_intents::parse(&b[..cut]).is_err());}}
}
#[test]
fn oversized_counts_unknown_channels_data_and_too_many_size_groups_are_rejected(){
    for b in [&[5,0,2,0,0xff,0xff,0xff,0xff][..],&[5,0,2,0,0,0,0,0][..],&[5,0,1,0,1,0,0,0][..],&[9,0,2,0,0,0,0,0][..],&[15,0,1,0,0,0,0,0][..],&[0;520][..]]{assert!(pixel_glink_intents::parse(b).is_err());}
    let mut b=vec![];for id in 0..9{add_intents(&mut b,&[(id,id)]);}assert_eq!(pixel_glink_intents::parse(&b),Err(Error::Bounds));
}
#[test]
fn malformed_byte_inputs_never_escape_slice_or_fixed_storage(){
    let mut state=0x12345678u32;for len in 0..=520{let mut b=vec![0;len];for v in &mut b{state=state.wrapping_mul(1664525).wrapping_add(1013904223);*v=(state>>24)as u8;}let _=pixel_glink_intents::parse(&b);}
}
#[test]
fn expanded_snapshot_is_read_only_stable_and_covers_the_full_synthetic_queue(){
    let(mut m,next,rx)=channel_fixture();let(ready,base,old,_)=next.into_parts();m[TX.offset+32..TX.offset+40].copy_from_slice(&pixel_glink_channel::ACK);w32(&mut m,DESCRIPTOR.offset,40);w32(&mut m,DESCRIPTOR.offset+4,40);w32(&mut m,DESCRIPTOR.offset+12,584);m[rx.offset+96..rx.offset+584].copy_from_slice(&full_control_fixture());let before=m.clone();
    struct Bounded<'a>{m:&'a[u8],rx:usize}impl Read for Bounded<'_>{fn read8(&self,at:usize)->u8{if(self.rx..self.rx+0x4000).contains(&at){assert!(at<self.rx+584)}self.m[at]}}
    let mut report=intent_report();assert_eq!(pixel_glink_channel::sample(&Bounded{m:&m,rx:rx.offset},&base,&ready,&old,32,96,&mut report),Ok(()));assert_eq!((report.new_bytes,report.available),(488,576));assert_eq!(report.intents.unwrap().entries,30);assert_eq!(m,before);
}
#[test]
fn instability_past_old_128_byte_bound_invalidates_inventory(){
    let(mut m,next,rx)=channel_fixture();let(ready,base,old,_)=next.into_parts();m[TX.offset+32..TX.offset+40].copy_from_slice(&pixel_glink_channel::ACK);w32(&mut m,DESCRIPTOR.offset,40);w32(&mut m,DESCRIPTOR.offset+4,40);w32(&mut m,DESCRIPTOR.offset+12,584);m[rx.offset+96..rx.offset+584].copy_from_slice(&full_control_fixture());let before=m.clone();
    struct Unstable<'a>{m:&'a[u8],rx:usize,reads:std::cell::Cell<u32>}impl Read for Unstable<'_>{fn read8(&self,at:usize)->u8{if at==self.rx+583{let n=self.reads.get();self.reads.set(n+1);if n>0{return self.m[at]^1}}self.m[at]}}
    let mut report=intent_report();report.intents=Some(pixel_glink_intents::parse(&[]).unwrap());assert_eq!(pixel_glink_channel::sample(&Unstable{m:&m,rx:rx.offset,reads:std::cell::Cell::new(0)},&base,&ready,&old,32,96,&mut report),Err(Error::Unstable));assert_eq!(report.intents,None);assert_eq!(m,before);
}
#[test]
fn expanded_reader_caps_at_512_and_never_claims_full_queue_beyond_it(){
    let(mut m,next,rx)=channel_fixture();let(ready,base,old,_)=next.into_parts();m[TX.offset+32..TX.offset+40].copy_from_slice(&pixel_glink_channel::ACK);w32(&mut m,DESCRIPTOR.offset,40);w32(&mut m,DESCRIPTOR.offset+4,40);w32(&mut m,DESCRIPTOR.offset+12,624);for b in m[rx.offset+96..rx.offset+608].chunks_exact_mut(8){b.copy_from_slice(&[13,0,0,0,0,0,0,0]);}let before=m.clone();
    struct Bounded<'a>{m:&'a[u8],rx:usize}impl Read for Bounded<'_>{fn read8(&self,at:usize)->u8{if(self.rx..self.rx+0x4000).contains(&at){assert!(at<self.rx+608)}self.m[at]}}
    let mut report=intent_report();assert_eq!(pixel_glink_channel::sample(&Bounded{m:&m,rx:rx.offset},&base,&ready,&old,32,96,&mut report),Ok(()));assert_eq!(report.new_bytes,512);assert!(report.new_bytes<report.available-88);assert_eq!(report.intents.unwrap().records,64);assert_eq!(m,before);
}
#[test]
fn passive_receive_intent_request_is_counted_without_allocating_or_responding(){
    let mut b=full_control_fixture();b[480..488].copy_from_slice(&[7,0,2,0,0xff,0xff,0xff,0xff]);let s=pixel_glink_intents::inspect(&b);assert_eq!(s.error,None);assert_eq!(s.parsed_bytes,488);assert_eq!((s.inventory.entries,s.inventory.intent_requests,s.inventory.last_request_size),(30,1,u32::MAX));assert_eq!(s.stop_header,None);
}
#[test]
fn unknown_tail_record_retains_exact_offset_header_and_complete_prefix_counts(){
    let mut b=full_control_fixture();b[480..488].copy_from_slice(&[9,0,2,0,0x80,0,0,0]);let s=pixel_glink_intents::inspect(&b);assert_eq!(s.error,Some(Error::Payload));assert_eq!((s.detail,s.stop_offset,s.parsed_bytes),(2,480,480));assert_eq!((s.inventory.records,s.inventory.entries),(30,30));assert_eq!(s.stop_header,Some(pixel_glink_receive::Header{command:9,param1:2,param2:128}));assert_eq!(s.preview_words,2);
}
#[test]
fn wrong_channel_and_count_failures_are_distinguished_after_valid_records(){
    for (header,detail)in [([5,0,3,0,1,0,0,0],3),([5,0,2,0,0xff,0xff,0xff,0xff],4)]{let mut b=vec![];add_intents(&mut b,&[(0x800,1)]);b.extend_from_slice(&header);let s=pixel_glink_intents::inspect(&b);assert_eq!(s.error,Some(Error::Payload));assert_eq!((s.detail,s.stop_offset,s.inventory.entries),(detail,16,1));}
}
#[test]
fn truncated_record_preview_and_partial_group_failure_do_not_commit_half_a_record(){
    let mut b=vec![5,0,2,0,2,0,0,0];b.extend_from_slice(&[0x00,0x08,0,0,7,0,0,0]);let s=pixel_glink_intents::inspect(&b);assert_eq!(s.error,Some(Error::Bounds));assert_eq!((s.detail,s.parsed_bytes,s.inventory.entries,s.preview_words),(5,0,0,4));assert_eq!(s.stop_words[2..],[0x800,7]);
    let mut b=vec![];for n in 0..8{add_intents(&mut b,&[(n,n)]);}add_intents(&mut b,&[(7,8),(9,9)]);let s=pixel_glink_intents::inspect(&b);assert_eq!(s.error,Some(Error::Bounds));assert_eq!((s.detail,s.stop_offset,s.inventory.records,s.inventory.entries),(7,128,8,8));assert_eq!(s.inventory.groups[7].count,1);
}
#[test]
fn native_read_only_snapshot_keeps_rejected_header_and_partial_inventory(){
    let(mut m,next,rx)=channel_fixture();let(ready,base,old,_)=next.into_parts();m[TX.offset+32..TX.offset+40].copy_from_slice(&pixel_glink_channel::ACK);w32(&mut m,DESCRIPTOR.offset,40);w32(&mut m,DESCRIPTOR.offset+4,40);w32(&mut m,DESCRIPTOR.offset+12,584);let mut b=full_control_fixture();b[480..488].copy_from_slice(&[9,0,2,0,8,0,0,0]);m[rx.offset+96..rx.offset+584].copy_from_slice(&b);let before=m.clone();let mut report=intent_report();
    struct Bytes<'a>(&'a[u8]);impl Read for Bytes<'_>{fn read8(&self,at:usize)->u8{self.0[at]}}
    assert_eq!(pixel_glink_channel::sample(&Bytes(&m),&base,&ready,&old,32,96,&mut report),Err(Error::Payload));assert!(report.tx_consumed);assert_eq!(report.intents,None);let s=report.intent_scan.unwrap();assert_eq!((s.parsed_bytes,s.stop_offset,s.inventory.entries),(480,480,30));assert_eq!(s.stop_header.unwrap().command,9);assert_eq!(m,before);
}
#[test]
fn receive_request_snapshot_keeps_all_memory_and_transport_indices_unchanged(){
    let(mut m,next,rx)=channel_fixture();let(ready,base,old,_)=next.into_parts();m[TX.offset+32..TX.offset+40].copy_from_slice(&pixel_glink_channel::ACK);w32(&mut m,DESCRIPTOR.offset,40);w32(&mut m,DESCRIPTOR.offset+4,40);w32(&mut m,DESCRIPTOR.offset+12,584);let mut b=full_control_fixture();b[480..488].copy_from_slice(&[7,0,2,0,0,0x10,0,0]);m[rx.offset+96..rx.offset+584].copy_from_slice(&b);let before=m.clone();let mut report=intent_report();
    struct Bytes<'a>(&'a[u8]);impl Read for Bytes<'_>{fn read8(&self,at:usize)->u8{self.0[at]}}
    assert_eq!(pixel_glink_channel::sample(&Bytes(&m),&base,&ready,&old,32,96,&mut report),Ok(()));let i=report.intents.unwrap();assert_eq!((i.entries,i.intent_requests,i.last_request_size),(30,1,4096));assert_eq!(report.words,[40,40,8,584,0,0,0,0]);assert_eq!(m,before);
}
#[test]
fn large_grouped_intent_records_use_the_512_byte_bound_instead_of_old_15_entry_limit(){
    let mut b=vec![];let pairs:Vec<_>=(1..=63).map(|id|(0x800,id)).collect();add_intents(&mut b,&pairs);assert_eq!(b.len(),512);let s=pixel_glink_intents::inspect(&b);assert_eq!(s.error,None);assert_eq!((s.parsed_bytes,s.inventory.entries,s.inventory.records),(512,63,1));
    b[4..8].copy_from_slice(&64u32.to_le_bytes());let s=pixel_glink_intents::inspect(&b);assert_eq!((s.error,s.detail,s.parsed_bytes),(Some(Error::Payload),4,0));
    let mut b=vec![];for id in 1..=8{add_intents(&mut b,&[(0x800,id)]);}let pairs:Vec<_>=(9..=52).map(|id|(0x2000,id)).collect();add_intents(&mut b,&pairs);assert_eq!(b.len(),488);let s=pixel_glink_intents::inspect(&b);assert_eq!(s.error,None);assert_eq!((s.inventory.intent_records,s.inventory.entries),(9,52));
}
