//! Phone Maps adapter: private queue and read-only surface, no app data access.
use super::*;
use core::sync::atomic::{AtomicU32,fence};
const CHANNEL:Channel=Channel::new(20);
const PAGE:i32=19;
pub struct Maps {
    frame:*const u32,queue:*mut u32,state:*const u32,sent:u32,revision:u32,
    active:bool,searching:bool,height:u32,down:bool,reset_pending:bool,
}
impl Maps {
    pub fn initialize(ui:&NotesWindow)->Rc<RefCell<Self>> {
        ui.set_maps_available(true);
        let result=Rc::new(RefCell::new(Self {
            frame:memory_region_symbol!(mobile_maps_frame:*mut u32).as_ptr(),
            queue:memory_region_symbol!(mobile_maps_input:*mut u32).as_ptr(),
            state:memory_region_symbol!(mobile_maps_state:*mut u32).as_ptr(),
            sent:0,revision:0,active:false,searching:false,height:0,down:false,reset_pending:false,
        }));
        let b=result.clone();ui.on_maps_key(move |key| {
            let mut b=b.borrow_mut();if !b.active||b.reset_pending{return;}
            match key.as_str() {
                "DELETE"=>{b.send(4,0,8,0,0);},"ENTER"=>{b.send(4,0,10,0,0);},
                "CLEAR"=>{b.send(3,3,0,0,0);},"SPACE"=>{b.send(4,0,32,0,0);},
                text=>for c in text.chars().filter(|c|!c.is_control()){b.send(4,0,c as u32,0,0);},
            }
        });
        let b=result.clone();let weak=ui.as_weak();ui.on_maps_hide_keyboard(move || {
            let mut b=b.borrow_mut();if !b.active{return;}
            b.send(3,2,0,0,0);if let Some(ui)=weak.upgrade(){ui.set_maps_keyboard(false);}
        });
        result
    }
    fn read(p:*const u32)->u32 {unsafe {core::ptr::read_volatile(p)}}
    fn revision(p:*const u32)->u32 {unsafe {&*p.cast::<AtomicU32>()}.load(Ordering::Acquire)}
    fn send(&mut self,device:u32,kind:u32,code:u32,value:u32,position:u32)->bool {
        let tail=Self::revision(unsafe {self.queue.add(3)});
        if self.sent.wrapping_sub(tail)>=if device==3 {224}else{220} {
            if device!=3 {self.reset_pending=true;}return false;
        }
        for (i,w) in [device|(kind<<16),code,value,position].iter().enumerate() {
            unsafe {core::ptr::write_volatile(self.queue.add(4+(self.sent%224) as usize*4+i),*w);}
        }
        self.sent=self.sent.wrapping_add(1);
        unsafe {&*self.queue.add(2).cast::<AtomicU32>()}.store(self.sent,Ordering::Release);
        CHANNEL.notify();true
    }
    pub fn poll(&mut self,ui:&NotesWindow) {
        let active=ui.get_page()==PAGE&&!ui.get_device_lock_screen();
        if self.reset_pending {
            if !self.send(3,0,0,0,0){return;}
            self.active=false;self.down=false;self.reset_pending=false;
        }
        if active!=self.active {
            if !self.send(3,0,0,u32::from(active),0){return;}
            self.active=active;self.down=false;self.revision=0;ui.set_maps_keyboard(false);
            if !active {ui.set_maps_frame(slint::Image::default());return;}
        }
        if !active{return;}
        let rev=Self::revision(self.state);let keyboard=Self::read(unsafe{self.state.add(1)});let searching=Self::read(unsafe{self.state.add(2)});
        fence(Ordering::Acquire);
        if rev!=0&&rev&1==0&&keyboard<=1&&searching<=1&&rev==Self::revision(self.state){ui.set_maps_keyboard(keyboard!=0);self.searching=searching!=0;}
        let height=(((HEIGHT as f32-(ui.get_top_inset()+80.+if ui.get_maps_keyboard(){268.}else{0.})*SCALE).max(0.) as u32)/3).clamp(320,720);
        if height!=self.height&&self.send(3,1,0,height,0){self.height=height;}
        let r=Self::revision(self.frame);
        if r==0||r&1!=0||r==self.revision{return;}
        let (w,h,stride,format)=(Self::read(unsafe{self.frame.add(1)}),Self::read(unsafe{self.frame.add(2)}),Self::read(unsafe{self.frame.add(3)}),Self::read(unsafe{self.frame.add(4)}));
        if w!=1080||h!=self.height*3||stride!=3240||format!=1{return;}
        let mut pixels=slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(w,h);
        unsafe {personal_os_slint_notes_spike::frame_copy::copy_nonoverlapping_ram(pixels.make_mut_bytes().as_mut_ptr(),self.frame.add(7).cast(),(stride*h) as usize);}
        fence(Ordering::Acquire);if r!=Self::revision(self.frame){return;}
        self.revision=r;ui.set_maps_frame(slint::Image::from_rgb8(pixels));
    }
    pub fn input(&mut self,ui:&NotesWindow,device:u64,kind:u64,code:u64,value:u64,x:u32,y:u32)->bool {
        if ui.get_page()!=PAGE||ui.get_device_lock_screen(){return false;}
        if device==DEVICE_KEYBOARD&&kind==EVENT_KEY {
            if matches!(code,59..=62|64..=88|125|126)||(code==1&&!self.searching){return false;}
            if self.active&&!self.reset_pending{self.send(0,1,code as u32,value as u32,0);}return true;
        }
        if device!=DEVICE_TABLET{return false;}
        let y=y as i32-((ui.get_top_inset()+80.)*SCALE) as i32;
        let inside=x<1080&&y>=0&&y<(self.height*3) as i32;
        let packed=(x.min(32767)&65535)|((y.clamp(-32768,32767) as u32&65535)<<16);
        if !self.active||self.reset_pending{return inside;}
        if kind==EVENT_KEY&&code==BTN_LEFT {
            if value!=0&&inside {self.down=self.send(1,1,272,1,packed);return true;}
            if value==0&&self.down {self.down=false;self.send(1,1,272,0,packed);return true;}
        }
        if (kind==EVENT_ABS||kind==EVENT_SYN)&&(inside||self.down) {
            self.send(1,3,0,0,packed);return true;
        }
        if kind==2&&code==8&&inside {self.send(1,2,8,value as u32,packed);return true;}
        false
    }
}
