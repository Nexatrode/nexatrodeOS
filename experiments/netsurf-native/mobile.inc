/* SPDX-License-Identifier: GPL-2.0-only */
/* Phone adapter: private UI queue; no raw input, wallet or vault mapping. */
int native_mobile_height=720;
uintptr_t mobile_state;
static bool mobile_active;
static void mobile_publish(void) {
    uint32_t *s=(void*)mobile_state,r=__atomic_load_n(s,__ATOMIC_RELAXED);
    __atomic_store_n(s,r+1,__ATOMIC_RELEASE);__atomic_thread_fence(__ATOMIC_SEQ_CST);
    s[1]=editing||mobile_keyboard;s[2]=window.scroll;s[3]=mobile_active;s[4]=HEIGHT;
    __atomic_store_n(s,r+2,__ATOMIC_RELEASE);
}
static void mobile_text(unsigned c) {
    if(c==8){key(14,1);return;}if(c==10){key(28,1);return;}
    if(c<32||c>126)return;
    if(editing){size_t n=strlen(typed_url);if(n+1<sizeof(typed_url)){typed_url[n]=c;typed_url[n+1]=0;}dirty=true;}
    else {browser_window_key_press(window.bw,c);dirty=true;}
}
static void input(void) {
    uint32_t *q=(void*)input_handoff,head=__atomic_load_n(q+2,__ATOMIC_ACQUIRE);
    if(head-input_tail>224){input_tail=head;mouse_down=shift=false;}
    while(input_tail!=head){
        uint32_t e[4];memcpy(e,q+4+(input_tail++%224)*4,sizeof(e));
        unsigned device=e[0]&65535,kind=e[0]>>16;
        if(device==3){
            if(kind==0){
                mobile_active=e[2]!=0;mouse_down=shift=editing=mobile_keyboard=false;mobile_keyboard_dismissed=true;window.caret_h=0;
                if(!mobile_active)stop_page();
                dirty=true;
            }else if(kind==1 && e[2]>=240 && e[2]<=MAX_HEIGHT){
                native_mobile_height=e[2];browser_window_schedule_reformat(window.bw);dirty=true;
            }else if(kind==2){editing=mobile_keyboard=false;mobile_keyboard_dismissed=true;window.caret_h=0;dirty=true;}
            else if(kind==3){if(editing)typed_url[0]=0;else {browser_window_key_press(window.bw,NS_KEY_SELECT_ALL);browser_window_key_press(window.bw,NS_KEY_DELETE_LEFT);}dirty=true;}
            continue;
        }
        if(!mobile_active)continue;
        if(device==4){mobile_text(e[1]);continue;}
        if(device==INPUT_DEVICE_KEYBOARD&&kind==1&&e[2]<=2){key(e[1],e[2]);continue;}
        if(device!=INPUT_DEVICE_TABLET)continue;
        mouse_x=(int16_t)(e[3]&65535)/NATIVE_SCALE;mouse_y=(int16_t)(e[3]>>16)/NATIVE_SCALE;
        if(kind==1&&e[1]==272&&e[2]<=1){pointer_move();pointer_button(e[2]);}
        else if(kind==3)pointer_move();
        else if(kind==2)scroll_to(window.scroll+(int32_t)e[2]);
    }
    __atomic_store_n(q+3,input_tail,__ATOMIC_RELEASE);mobile_publish();microkit_notify(0);
}
