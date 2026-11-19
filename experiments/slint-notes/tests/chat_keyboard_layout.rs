#![cfg(feature = "full-suite-ui")]

use personal_os_slint_notes_spike::NotesWindow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, PhysicalSize, SharedString, VecModel};
use std::{rc::Rc, time::Instant};

struct KeyboardTestPlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for KeyboardTestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> std::time::Duration {
        self.start.elapsed()
    }
}

#[test]
fn chat_search_and_composer_paint_the_same_docked_keyboard() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(KeyboardTestPlatform {
        window: window.clone(),
        start: Instant::now(),
    }))
    .unwrap();
    let ui = NotesWindow::new().unwrap();
    ui.set_page(6);
    ui.set_message_service_ready(true);
    ui.set_message_chats(ModelRc::new(VecModel::from(vec![
        SharedString::from("Alpha"),
        "Beta".into(),
        "Gamma".into(),
        "Delta".into(),
    ])));
    ui.show().unwrap();

    for (height, top_inset) in [(897usize, 0.), (736, 0.), (736, 25.)] {
        let width = 414;
        ui.set_top_inset(top_inset);
        window.set_size(PhysicalSize::new(width as u32, height as u32));
        let render = || {
            let mut pixels = vec![slint::Rgb8Pixel::default(); width * height];
            assert!(window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, width);
            }));
            pixels
        };
        ui.set_message_view(true);
        ui.set_edit_target(6);
        let composer = render();
        ui.set_message_view(false);
        ui.set_edit_target(10);
        let search = render();
        let rows_height = ui.get_chat_list_rows_height();
        assert!(rows_height > 0.);
        assert!(258. + rows_height <= ui.get_chat_keyboard_top() - 28.);
        let keyboard = (height - 278) * width..(height - 22) * width;
        let keys = &composer[keyboard.clone()];
        assert!(keys.iter().any(|pixel| pixel.r > 150 && pixel.g > 150));
        let different = keys
            .iter()
            .zip(&search[keyboard])
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(different, 0, "keyboard pixels differ at height {height}");
        ui.set_edit_target(0);
        assert_eq!(ui.get_chat_list_rows_height(), 260.);
    }
}
