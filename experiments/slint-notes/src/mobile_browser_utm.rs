//! Opt-in phone browser surface/input adapter. No browser access to app data.
use super::*;
use core::sync::atomic::{AtomicU32,fence};

const CHANNEL:Channel=Channel::new(18);
const PAGE:i32=18;
pub struct Browser {
    frame:*const u32, queue:*mut u32, state:*const u32,
    sent:u32, revision:u32, active:bool, height:u32,
    down:Option<(i32,i32)>, last_y:i32, dragging:bool,
}
impl Browser {
    pub fn initialize(ui:&NotesWindow)->Rc<RefCell<Self>> {
        let result=Rc::new(RefCell::new(Self {
            frame:memory_region_symbol!(mobile_browser_frame:*mut u32).as_ptr(),
            queue:memory_region_symbol!(mobile_browser_input:*mut u32).as_ptr(),
            state:memory_region_symbol!(mobile_browser_state:*mut u32).as_ptr(),
            sent:0,revision:0,active:false,height:0,down:None,last_y:0,dragging:false,
        }));
        ui.set_browser_available(true);
        let b=result.clone();ui.on_browser_key(move |key| {
            let mut b=b.borrow_mut();if !b.active{return;}
            match key.as_str() {
                "DELETE"=>{b.send(4,0,8,0,0);},"ENTER"=>{b.send(4,0,10,0,0);},
                "CLEAR"=>{b.send(3,3,0,0,0);},"SPACE"=>{b.send(4,0,32,0,0);},
                text=>for c in text.chars().filter(|c|c.is_ascii_graphic()){b.send(4,0,c as u32,0,0);},
            }
        });
        let b=result.clone();let weak=ui.as_weak();ui.on_browser_hide_keyboard(move || {
            b.borrow_mut().send(3,2,0,0,0);if let Some(ui)=weak.upgrade(){ui.set_browser_keyboard(false);}
        });
        result
    }
    fn revision(p:*const u32)->u32 {unsafe {&*p.cast::<AtomicU32>()}.load(Ordering::Acquire)}
    fn read(p:*const u32)->u32 {unsafe {core::ptr::read_volatile(p)}}
    fn send(&mut self,device:u32,kind:u32,code:u32,value:u32,position:u32)->bool {
        let tail=unsafe {&*self.queue.add(3).cast::<AtomicU32>()}.load(Ordering::Acquire);
        if self.sent.wrapping_sub(tail)>=if device==3 {224}else{220}{return false;}
        let words=[device|(kind<<16),code,value,position];
        for (i,w) in words.iter().enumerate(){unsafe {core::ptr::write_volatile(self.queue.add(4+(self.sent%224) as usize*4+i),*w);}}
        self.sent=self.sent.wrapping_add(1);
        unsafe {&*self.queue.add(2).cast::<AtomicU32>()}.store(self.sent,Ordering::Release);CHANNEL.notify();true
    }
    pub fn poll(&mut self,ui:&NotesWindow) {
        let active=ui.get_page()==PAGE&&!ui.get_device_lock_screen();
        if active!=self.active {
            let sent=self.send(3,0,0,u32::from(active),0);
            if sent {self.active=active;}
            self.revision=0;self.down=None;ui.set_browser_keyboard(false);
            if !active {ui.set_browser_frame(slint::Image::default());return;}
            if !sent {return;}
        }
        if !active{return;}
        let rev=Self::revision(self.state);
        let keys=Self::read(unsafe {self.state.add(1)});
        fence(Ordering::Acquire);
        if rev!=0&&rev&1==0&&keys<=1&&rev==Self::revision(self.state){ui.set_browser_keyboard(keys!=0);}
        let available=(HEIGHT as f32-(ui.get_top_inset()+80.+if ui.get_browser_keyboard(){268.}else{0.})*SCALE).max(0.) as u32;
        let height=(available/3).clamp(240,720);
        if height!=self.height&&self.send(3,1,0,height,0){self.height=height;}
        let r=Self::revision(self.frame);
        if r==0||r&1!=0||r==self.revision{return;}
        let (width,height,stride,format)=(Self::read(unsafe{self.frame.add(1)}),Self::read(unsafe{self.frame.add(2)}),Self::read(unsafe{self.frame.add(3)}),Self::read(unsafe{self.frame.add(4)}));
        if width!=1080||height<720||height>2160||stride!=3240||format!=1{return;}
        let mut pixels=slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width,height);
        unsafe {personal_os_slint_notes_spike::frame_copy::copy_nonoverlapping_ram(pixels.make_mut_bytes().as_mut_ptr(),self.frame.add(7).cast(),(stride*height) as usize);}
        fence(Ordering::Acquire);if r!=Self::revision(self.frame){return;}
        self.revision=r;ui.set_browser_frame(slint::Image::from_rgb8(pixels));
    }
    pub fn input(&mut self,ui:&NotesWindow,device:u64,kind:u64,code:u64,value:u64,x:u32,y:u32)->bool {
        if ui.get_page()!=PAGE||ui.get_device_lock_screen(){return false;}
        if device==DEVICE_KEYBOARD&&kind==EVENT_KEY {
            if matches!(code,1|59..=62|64..=88|125|126){return false;}
            self.send(0,1,code as u32,value as u32,0);return true;
        }
        if device!=DEVICE_TABLET{return false;}
        let y=y as i32-((ui.get_top_inset()+80.)*SCALE) as i32;
        let inside=x<1080&&y>=0&&y<(self.height*3) as i32;
        let packed=(x.min(32767)&65535)|((y.clamp(-32768,32767) as u32&65535)<<16);
        if kind==EVENT_KEY&&code==BTN_LEFT {
            if value!=0&&inside {self.down=Some((x as i32,y));self.last_y=y;self.dragging=false;return true;}
            if value==0 {if let Some((sx,sy))=self.down.take(){
                if !self.dragging&&inside {
                    let p=(sx as u32&65535)|((sy as u32&65535)<<16);
                    self.send(1,1,272,1,p);self.send(1,1,272,0,packed);
                }return true;
            }}
        }
        if let Some((sx,sy))=self.down {
            if kind==EVENT_ABS||kind==EVENT_SYN {
                if (y-sy).abs()>18||(x as i32-sx).abs()>18 {self.dragging=true;}
                if self.dragging {let delta=(self.last_y-y)/3;if delta!=0{self.send(1,2,8,delta as u32,packed);self.last_y=y;}}
                return true;
            }
        }
        if kind==2&&code==8&&inside {self.send(1,2,8,(-(value as i32)*80) as u32,packed);return true;}
        false
    }
}
