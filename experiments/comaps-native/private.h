/* SPDX-License-Identifier: Apache-2.0
 * Opt-in, app-scoped saved-places vault. No private records reach map_store.
 * The latter receives only two fixed-size authenticated ciphertext banks.
 */
#define PRIVATE_MAGIC 0x3156415350414d4eULL
static void private_clear_input(void){private_wipe(private_password,sizeof(private_password));private_length=0;}
static bool private_valid(void){
    if(places.magic!=PRIVATE_MAGIC||places.reserved||places.count>PRIVATE_MAX)return false;
    for(unsigned i=0;i<places.count;i++){struct saved_place *p=&places.items[i];if(!p->name[0]||!memchr(p->name,0,96)||p->region>=MAP_REGION_COUNT||p->kind>1||!isfinite(p->x)||!isfinite(p->y)||fabs(p->x)>180||fabs(p->y)>181)return false;}
    return true;
}
static bool private_entropy(void *out,unsigned n){
    uint8_t *p=out;while(n){microkit_msginfo r=microkit_ppcall(3,microkit_msginfo_new(0x401,0));
        if(microkit_msginfo_get_label(r)!=0x400||microkit_msginfo_get_count(r)!=2)return false;
        uint64_t a[]={microkit_mr_get(0),microkit_mr_get(1)};unsigned take=n<16?n:16;memcpy(p,a,take);private_wipe(a,sizeof(a));p+=take;n-=take;
    }return true;
}
static void private_lock(void){
    mp_lock();private_ready=false;private_clear_input();private_wipe(private_confirm,sizeof(private_confirm));private_wipe(&places,sizeof(places));private_wipe(&private_pending,sizeof(private_pending));
    private_pending_save=false;private_enrolling=false;private_pressed=-1;private_mode=0;private_viewed=private_target=private_cursor=private_offset=0;
    // Clear the entire shared surface, including rows outside the current
    // keyboard-reduced viewport, before publishing a public replacement.
    slint_frame_header_t *frame=(void*)map_frame;unsigned revision=atomic_load(&frame->revision);
    atomic_store(&frame->revision,revision+1);atomic_thread_fence(memory_order_seq_cst);
    private_wipe((void*)(map_frame+sizeof(*frame)),SLINT_FRAME_REGION_BYTES-sizeof(*frame));
    atomic_store_explicit(&frame->revision,revision+2,memory_order_release);
    search_close();private_wipe(search.query,sizeof(search.query));map_search_run(&search,&map);selection_clear();private_wipe(selection_title,sizeof(selection_title));
    if(map.feature_count)reset();private_status="";changed=true;
#ifdef MAP_PROFILE
    private_wipe((void*)map_observation,8192);
#endif
    printf("MAPPRIVATE|LOCKED|key=%u\n",mp_opened());
}
static void private_io(unsigned command,unsigned bank){
    struct map_store_page *p=(void*)map_store_control;private_busy=true;private_phase=command;private_sequence=++storage_sequence;
    p->command=command;p->region=bank;p->length=0;p->requested_us=us();__atomic_store_n(&p->sequence,private_sequence,__ATOMIC_RELEASE);microkit_notify(2);changed=true;
}
static void private_begin(void){
    if(storage_state==1||storage_state==3||private_busy)return;
    search_close();region_picker=false;private_viewed=0;private_clear_input();private_last_activity=us()/1000;
    if(mp_opened()){private_mode=1;private_status="Tap a place to open";}
    else if(!private_ready){private_mode=5;private_status="Loading encrypted saved places…";private_io(MAP_PRIVATE_READ,0);}
#ifdef MAP_SESSION_PRIVATE
    else {
        uint8_t *page=(void*)map_session_key;private_wipe(page,4096);
        microkit_msginfo reply=microkit_ppcall(5,microkit_msginfo_new(0x458,0));
        unsigned length;memcpy(&length,page,4);
        unsigned opened=0;
        if(microkit_msginfo_get_label(reply)==0x300&&microkit_msginfo_get_count(reply)==1&&microkit_mr_get(0)&&length==32){
            private_session_epoch=microkit_mr_get(0);
            opened=mp_session_open(page+4,(void*)map_private_cipher,&places,&private_generation,&private_bank);
        }
        private_wipe(page,4096);
        if(opened==2)places.magic=PRIVATE_MAGIC;
        if(!opened||!private_valid()){mp_lock();private_wipe(&places,sizeof(places));private_mode=5;private_status="Saved places could not be opened";}
        else {private_mode=1;private_status="Unlocked with device";printf("MAPPRIVATE|SESSION_OPEN|epoch=%llu|count=%u\n",(unsigned long long)private_session_epoch,places.count);private_add_pending();}
    }
#else
    else {unsigned status=mp_status((void*)map_private_cipher);private_mode=status==0?2:status==1?4:5;private_status=status==0?"Choose a separate passphrase (8+ characters)":status==1?"Enter your saved-places passphrase":"Saved-places storage is damaged";}
#endif
    changed=true;
}
static void private_commit(bool enrollment){
    if(private_busy||!mp_opened())return;
    uint8_t nonce[12];if(!private_entropy(nonce,sizeof(nonce))){private_lock();private_mode=5;private_status="Entropy unavailable; saved places locked";return;}
    unsigned bank=enrollment?0:1-private_bank;uint64_t gen=enrollment?1:private_generation+1;
    if(!mp_seal(&places,nonce,gen,(void*)(map_private_cipher+bank*4096))){private_wipe(nonce,sizeof(nonce));private_lock();private_mode=5;private_status="Unable to encrypt saved places";return;}
    private_wipe(nonce,sizeof(nonce));private_enrolling=enrollment;private_status="Saving encrypted places…";private_io(MAP_PRIVATE_WRITE,bank);
}
static void private_add_pending(void){
    if(!private_pending_save)return;
    unsigned i=0;for(;i<places.count;i++)if(places.items[i].region==private_pending.region&&places.items[i].x==private_pending.x&&places.items[i].y==private_pending.y)break;
    if(i<places.count){private_pending_save=false;private_wipe(&private_pending,sizeof(private_pending));private_status="Already saved · existing name kept";changed=true;return;}
    if(i==places.count){if(i==PRIVATE_MAX){private_status="Saved places is full (16)";private_pending_save=false;return;}places.count++;}
    places.items[i]=private_pending;private_pending_save=false;private_wipe(&private_pending,sizeof(private_pending));private_commit(false);
}
static void private_save_selection(void){
    if(!selected_feature||selected_feature>map.feature_count||private_busy||storage_state!=0)return;
    const struct map_feature *f=&map.features[selected_feature-1];struct map_point p=map.points[f->first+f->count/2];
    private_wipe(&private_pending,sizeof(private_pending));memcpy(private_pending.name,f->name,96);private_pending.x=map.origin_x+p.x;private_pending.y=map.origin_y+p.y;private_pending.region=loaded_region;private_pending.kind=f->geometry==0?0:1;private_pending_save=true;
    private_begin();if(mp_opened())private_add_pending();
}
static void private_reply(void){
    struct map_store_page *p=(void*)map_store_control;if(__atomic_load_n(&p->complete,__ATOMIC_ACQUIRE)!=private_sequence)return;
    private_busy=false;
    if(p->result){private_lock();private_mode=5;private_status="Saved-places I/O failed; locked";draw();return;}
    if(private_phase==MAP_PRIVATE_READ){private_ready=true;private_mode=0;private_begin();}
    else if(private_phase==MAP_PRIVATE_WRITE){
        // A hidden/locked application never becomes unlocked on a late reply.
        if(!mp_opened()){private_mode=0;draw();return;}
        if(private_enrolling&&p->region==0){memcpy((void*)(map_private_cipher+4096),(void*)map_private_cipher,4096);private_io(MAP_PRIVATE_WRITE,1);draw();return;}
        private_generation=private_enrolling?1:private_generation+1;private_bank=(unsigned)p->region;private_enrolling=false;
        private_mode=1;private_status="Saved · encrypted on disk";printf("MAPPRIVATE|SAVED|generation=%llu|count=%u\n",(unsigned long long)private_generation,places.count);
        if(private_pending_save)private_add_pending();
    }changed=true;draw();
}
static void private_submit(void){
    if(private_busy)return;
    if(private_mode==2){if(private_length<8){private_clear_input();private_status="Use at least 8 characters";return;}memcpy(private_confirm,private_password,private_length+1);private_clear_input();private_mode=3;private_status="Enter the same passphrase again";return;}
    if(private_mode==3){
        unsigned diff=0;for(unsigned i=0;i<129;i++)diff|=(unsigned char)private_confirm[i]^(unsigned char)private_password[i];
        if(diff){private_wipe(private_confirm,sizeof(private_confirm));private_clear_input();private_mode=2;private_status="Passphrases differed; start again";return;}
        private_wipe(private_confirm,sizeof(private_confirm));uint8_t entropy[60];
        private_wipe(&places,sizeof(places));places.magic=PRIVATE_MAGIC;
        bool ok=private_entropy(entropy,sizeof(entropy))&&mp_enroll(private_password,private_length,entropy,&places,(void*)map_private_cipher);
        private_wipe(entropy,sizeof(entropy));private_clear_input();if(!ok){private_lock();private_mode=5;private_status="Unable to create saved-places vault";return;}
        private_mode=1;private_commit(true);return;
    }
    if(private_mode==4){
        uint64_t now=us()/1000;if(now<private_retry_at){private_clear_input();private_status="Unable to unlock. Wait and try again";return;}
        bool ok=mp_unlock(private_password,private_length,(void*)map_private_cipher,&places,&private_generation,&private_bank)&&private_valid();private_clear_input();
        if(!ok){mp_lock();private_wipe(&places,sizeof(places));if(private_failures<6)private_failures++;private_retry_at=us()/1000+((uint64_t)1000<<private_failures);private_status="Unable to unlock. Wait and try again";return;}
        private_failures=0;private_mode=1;private_status="Tap a place to open";printf("MAPPRIVATE|OPEN|generation=%llu|count=%u\n",(unsigned long long)private_generation,places.count);private_add_pending();return;
    }
    if(private_mode==6){
        if(private_cursor<places.count&&private_length&&private_length<96){private_wipe(places.items[private_cursor].name,96);memcpy(places.items[private_cursor].name,private_password,private_length);private_clear_input();private_mode=1;private_commit(false);}
        return;
    }
}
static void private_character(unsigned cp){
    if(!private_keyboard()||private_busy)return;private_last_activity=us()/1000;
    if(cp==10||cp==13){private_submit();changed=true;return;}
    if(cp==8||cp==127){if(private_length){unsigned old=private_length;private_length--;while(private_length&&((unsigned char)private_password[private_length]&192)==128)private_length--;private_wipe(private_password+private_length,old-private_length+1);}changed=true;return;}
    if(cp<32||cp>0x10ffff||(cp>=0xd800&&cp<=0xdfff))return;
    unsigned n=cp<128?1:cp<2048?2:cp<65536?3:4,limit=private_mode==6?95:128;if(n>limit-private_length)return;
    if(n==1)private_password[private_length++]=cp;else{private_password[private_length++]=(n==2?0xc0:n==3?0xe0:0xf0)|(cp>>(6*(n-1)));for(unsigned i=n-1;i;i--)private_password[private_length++]=0x80|((cp>>(6*(i-1)))&63);}
    private_password[private_length]=0;changed=true;
}
static void private_restore(void){
    if(!private_target||!mp_opened()||private_target>places.count)return;
    if(places.items[private_target-1].region!=loaded_region){private_target=0;return;}
    private_viewed=private_target;private_target=0;struct saved_place *p=&places.items[private_viewed-1];cx=p->x-map.origin_x;cy=p->y-map.origin_y;zoom_level=3;units=initial_units/3.375;constrain();selection_clear();search_close();private_mode=0;changed=true;
}
static void private_choose(unsigned i){
    if(i>=places.count||!mp_opened()||private_busy)return;private_target=i+1;private_mode=0;
    if(places.items[i].region!=loaded_region)storage_request(MAP_STORE_LOAD,places.items[i].region);else private_restore();
}
static unsigned private_rows(void){int n=((int)BOTTOM-(TOP+62)-42)/48;return n<1?1:(unsigned)n;}
static void private_draw(void){
    fill(0,TOP,W,BOTTOM-TOP,0x071521);last_layout.count=0;
    if(private_mode==1){
        text(12,TOP+30,21,0xe9f7fb,"Saved places");text(W-79,TOP+29,14,0x92e3e0,PRIVATE_LOCK_CAPTION);text(W-32,TOP+31,25,0x92e3e0,"×");
        if(!places.count)text(12,TOP+94,16,0x91b5c5,"Choose a map result, then Save");
        for(unsigned row=0;row<private_rows()&&row+private_offset<places.count;row++){
            unsigned i=row+private_offset;int y=TOP+62+row*48;struct saved_place *p=&places.items[i];
            fill(8,y,W-16,44,i==private_cursor?0x183c4a:0x102433);clipped_text(16,y+22,17,0xe9f7fb,p->name,W-140,false);
            text(16,y+39,12,0x91b5c5,map_regions[p->region].name);text(W-115,y+28,13,0x92e3e0,"Rename");text(W-32,y+31,24,0xffa6ad,"×");
        }
    }else if(private_keyboard()){
        text(12,TOP+35,21,0xe9f7fb,private_mode==2?"Set saved-places passphrase":private_mode==3?"Confirm passphrase":private_mode==6?"Rename saved place":"Unlock saved places");
        fill(10,TOP+57,W-20,42,0x123143);
        if(private_mode==2||private_mode==3){char n[48];snprintf(n,sizeof(n),"%u characters entered",private_length);text(20,TOP+85,17,0xe9f7fb,n);}
        else if(private_mode==6)clipped_text(20,TOP+85,17,0xe9f7fb,private_password,W-45,true);
        else text(20,TOP+85,17,0xe9f7fb,"Passphrase hidden");
        fill(10,BOTTOM-48,W-20,38,0x183c4a);text(24,BOTTOM-21,18,0x92e3e0,private_busy?"Saving…":"Continue");
        text(W-78,TOP+133,15,0x92e3e0,"Cancel");
    }else{text(12,TOP+40,21,0xe9f7fb,"Saved places");text(W-32,TOP+31,25,0x92e3e0,"×");}
    clipped_text(12,private_mode==1?BOTTOM-12:BOTTOM-59,13,0x91b5c5,private_status,W-30,false);
}
static void private_card(void){
    if(private_viewed&&mp_opened()&&private_viewed<=places.count){
        struct saved_place *p=&places.items[private_viewed-1];int top=BOTTOM-94;
        int px=sx(p->x-map.origin_x),py=sy(p->y-map.origin_y);
        if(px>=10&&px<W-10&&py>=TOP+10&&py<top-10)for(int yy=-9;yy<=9;yy++)for(int xx=-9;xx<=9;xx++)if(xx*xx+yy*yy<=81)fill(px+xx,py+yy,1,1,xx*xx+yy*yy>36?0xffffff:0x19bfd1);
        fill(6,top,W-12,88,0x102c3a);fill(6,top,W-12,2,0x34cddd);clipped_text(18,top+27,19,0xe9f7fb,p->name,W-80,false);text(W-40,top+29,26,0x92e3e0,"×");
        text(18,top+49,14,0x91b5c5,"Saved place · private");char coords[80];snprintf(coords,sizeof(coords),"Map point: %.5f, %.5f",atan(sinh(p->y*3.141592653589793/180))*180/3.141592653589793,p->x);text(18,top+72,13,0x91b5c5,coords);
    }else if(selected_feature&&!search_open&&!private_mode){fill(W-90,BOTTOM-35,73,25,0x183c4a);text(W-80,BOTTOM-17,13,0x92e3e0,"SAVE");}
}
static void private_row_action(unsigned i,int x){
    private_cursor=i;
    if(x>=W-45){memmove(places.items+i,places.items+i+1,(places.count-i-1)*sizeof(struct saved_place));private_wipe(&places.items[--places.count],sizeof(struct saved_place));if(private_cursor>=places.count)private_cursor=0;private_offset=0;private_commit(false);}
    else if(x>=W-124){private_mode=6;private_clear_input();private_status="Enter a new name";}
    else private_choose(i);
}
static bool private_button(unsigned value){
    if(!value){
        if(private_mode==1&&held){
            motion();bool tap=map_gesture_end(&gesture);held=false;
            if(tap&&private_pressed>=0&&my>=TOP+62&&my<(int)BOTTOM-42){unsigned i=private_offset+(my-TOP-62)/48;if(i<places.count&&i==(unsigned)private_pressed)private_row_action(i,private_press_x);}
            private_pressed=-1;changed=true;return true;
        }
        return private_mode!=0;
    }
    private_last_activity=us()/1000;
    if(private_busy)return true;
    if(my>=(int)BOTTOM&&mx>=W-175&&mx<W-95){private_begin();return true;}
    if(!private_mode){
        if(private_viewed&&my>=(int)BOTTOM-94){if(mx>=W-56)private_viewed=0;changed=true;return true;}
        if(selected_feature&&!search_open&&my>=(int)BOTTOM-35&&my<(int)BOTTOM-6&&mx>=W-95){private_save_selection();return true;}return false;
    }
    if(private_mode==1){
        if(my<TOP)return true;
        if(my<TOP+46){if(mx>=W-85&&mx<W-40){private_lock();}else if(mx>=W-40){private_mode=0;}changed=true;return true;}
        if(my>=TOP+62&&my<(int)BOTTOM-42){unsigned i=private_offset+(my-TOP-62)/48;if(i<places.count){private_cursor=i;private_pressed=(int)i;private_press_x=mx;private_scroll=0;held=true;map_gesture_begin(&gesture,mx,my);}}changed=true;return true;
    }
    if(private_keyboard()){
        if(my>=(int)BOTTOM-48)private_submit();else if(my>=TOP+103&&my<TOP+145&&mx>=W-100){private_lock();}
    }else if(mx>=W-45&&my<TOP+46){private_lock();}
    changed=true;return true;
}
static bool private_key(unsigned code,unsigned value){
    if(private_mode==0)return false;if(!value||value>2)return true;private_last_activity=us()/1000;
    if(code==1){private_lock();return true;}
    if(private_mode==1){
        if(code==28)private_choose(private_cursor);
        if(code==103&&private_cursor)private_cursor--;
        if(code==108&&private_cursor+1<places.count)private_cursor++;
        if(private_cursor<private_offset)private_offset=private_cursor;if(private_cursor>=private_offset+private_rows())private_offset=private_cursor-private_rows()+1;
        if(code==38)private_lock();changed=true;return true;
    }
    if(search_ctrl&&code==22){private_clear_input();changed=true;return true;}
    if(code==28){private_submit();changed=true;return true;}
    if(code==14||code==111){private_character(8);return true;}
    unsigned cp=key_character(code,search_shift);if(cp)private_character(cp);return true;
}
static void private_tick(void){
    if((mp_opened()||private_keyboard()||private_pending_save)&&us()/1000-private_last_activity>=60000){private_lock();}
    bool was=active;if(changed&&!active)active=true;draw();active=was;publish_search_state();
#ifdef MAP_PROFILE
    profile(0,NULL,0,0);
#endif
    microkit_mr_set(0,1000000000);microkit_ppcall(4,microkit_msginfo_new(1,1));
}
