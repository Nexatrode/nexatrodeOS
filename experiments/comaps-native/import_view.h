/* SPDX-License-Identifier: Apache-2.0 */
static void import_metadata(void){
    struct map_store_page *p=(void*)map_store_control;memcpy(import_regions,p->local_regions,sizeof(import_regions));
    for(unsigned i=0;i<MAP_REGION_COUNT;i++)if(import_regions[i].present){map_regions[i].name=import_regions[i].name;map_regions[i].length=import_regions[i].length;memcpy(map_regions[i].digest,import_regions[i].digest,32);}
}
static void import_request(unsigned command){
    struct map_store_page *p=(void*)map_store_control;import_state=command==MAP_IMPORT_PROBE?1:command==MAP_IMPORT_READ?3:4;storage_state=4;
    selection_clear();search_close();held=false;map_gesture_cancel(&gesture);p->command=command;p->requested_us=us();p->progress=0;p->import_phase=0;
    __atomic_store_n(&p->sequence,++storage_sequence,__ATOMIC_RELEASE);microkit_notify(2);changed=true;
}
static void import_reply(void){
    struct map_store_page *p=(void*)map_store_control;storage_state=0;import_metadata();
    if(p->result){import_state=p->result==MAP_STORE_CANCELLED?7:6;storage_error=p->result;changed=true;draw();return;}
    if(p->command==MAP_IMPORT_PROBE){memcpy(&import_preview,p->local_pack,sizeof(import_preview));import_state=2;}
    else if(p->command==MAP_IMPORT_READ){
        /* Decoding and capacity checks happen before the store may write. */
        if(p->length!=import_preview.length||!map_load(candidate,(void*)map_store_bytes,p->length)){import_state=6;storage_error=MAP_STORE_CORRUPT;}
        else {import_request(MAP_IMPORT_COMMIT);draw();return;}
    }else{
        struct offline_map *old=current_map;current_map=candidate;candidate=old;loaded_region=import_preview.region;requested_region=loaded_region;region_names_ready=false;search_rebuild();reset();import_state=5;region_picker=false;
        recovering=false;storage_request(MAP_STORE_COMMIT,loaded_region);
    }changed=true;draw();
}
