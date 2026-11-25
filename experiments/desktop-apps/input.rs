//! Window-local input for the existing app frontend. Desktop controls use Slint
//! focus and text editing; phone gesture/hit-box fallbacks remain phone-only.
use super::*;
use slint::platform::{Key, PointerEventButton, WindowEvent};

impl SlintService {
    pub(super) fn desktop_input(&mut self,device:u64,kind:u64,code:u64,value:u64)
        -> Result<MessageInfo,Infallible>
    {
        if device==2 {
            if self.desktop_pointer.take().is_some(){self.window.window().dispatch_event(WindowEvent::PointerReleased{position:slint::LogicalPosition::new(-1.0,-1.0),button:PointerEventButton::Left});}
            for held in &mut self.desktop_held {
                if let Some(text)=held.take(){self.window.window().dispatch_event(WindowEvent::KeyReleased{text});}
            }
            self.shift_down=false;self.ctrl_down=false;self.meta_down=false;
            self.window.window().dispatch_event(WindowEvent::WindowActiveChanged(value!=0));
        } else if device==DEVICE_TABLET {
            let xy=POSTED_POSITION.load(Ordering::Relaxed);
            let position=slint::LogicalPosition::new((xy as u16 as i16) as f32/SCALE,((xy>>16) as u16 as i16) as f32/SCALE);
            if kind==EVENT_KEY && code==BTN_LEFT {
                self.desktop_pointer=if value!=0 {Some(position)}else{None};
                self.window.window().dispatch_event(if value!=0 {
                    WindowEvent::PointerPressed{position,button:PointerEventButton::Left}
                }else{WindowEvent::PointerReleased{position,button:PointerEventButton::Left}});
            } else if kind==3 {
                self.window.window().dispatch_event(WindowEvent::PointerMoved{position});
            } else if kind==2 && code==8 {
                self.window.window().dispatch_event(WindowEvent::PointerScrolled{position,
                    delta_x:0.0,delta_y:(value as i32) as f32*48.0});
            }
        } else if device==DEVICE_KEYBOARD && kind==EVENT_KEY && code<256 {
            if matches!(code,42|54){self.shift_down=value!=0;}
            if matches!(code,29|97){self.ctrl_down=value!=0;}
            if matches!(code,125|126){self.meta_down=value!=0;}
            if !cfg!(feature="desktop-chat-window") && value!=0 && self._ui.get_edit_target()!=0
                && apply_hardware_key(&self._ui,code,self.shift_down) {
                self.publish_frame();SLINT_INPUT.notify();return Ok(MessageInfo::new(REPLY_SLINT_EVENT_OK,0));
            }
            let text:Option<slint::SharedString>=match code {
                14=>Some(Key::Backspace.into()),28=>Some(Key::Return.into()),15=>Some(Key::Tab.into()),
                42|54=>Some(Key::Shift.into()),29|97=>Some(Key::Control.into()),125|126=>Some(Key::Meta.into()),
                102=>Some(Key::Home.into()),107=>Some(Key::End.into()),111=>Some(Key::Delete.into()),
                103=>Some(Key::UpArrow.into()),108=>Some(Key::DownArrow.into()),
                105=>Some(Key::LeftArrow.into()),106=>Some(Key::RightArrow.into()),
                _=>hardware_character(code,self.shift_down).map(Into::into),
            };
            if value==0 {
                if let Some(text)=self.desktop_held[code as usize].take(){self.window.window().dispatch_event(WindowEvent::KeyReleased{text});}
            } else if let Some(text)=text {
                self.desktop_held[code as usize]=Some(text.clone());
                self.window.window().dispatch_event(WindowEvent::KeyPressed{text});
            }
        }
        self.publish_frame();SLINT_INPUT.notify();self.arm_frame_timer();
        Ok(MessageInfo::new(REPLY_SLINT_EVENT_OK,0))
    }
}
