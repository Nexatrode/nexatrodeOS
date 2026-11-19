#![cfg(feature = "full-suite-ui")]

mod platform;

use personal_os_slint_notes_spike::NotesWindow;
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use slint::{
    ComponentHandle, LogicalPosition, ModelRc, PhysicalSize, Rgb8Pixel, SharedString, VecModel,
};
use std::{cell::RefCell, rc::Rc};

#[test]
fn tapping_retry_uses_the_failed_rows_saved_file() {
    let window = platform::install();
    let ui = NotesWindow::new().unwrap();
    ui.set_page(6);
    ui.set_message_service_ready(true);
    ui.set_message_view(true);
    ui.set_message_bubbles(ModelRc::new(VecModel::from(vec![SharedString::from(
        "notes.txt",
    )])));
    ui.set_message_bubble_side(ModelRc::new(VecModel::from(vec![1])));
    ui.set_message_bubble_files(ModelRc::new(VecModel::from(vec![SharedString::from("47")])));
    ui.set_message_bubble_time(ModelRc::new(VecModel::from(vec![SharedString::from(
        "Upload failed",
    )])));
    ui.set_message_bubble_upload_failed(ModelRc::new(VecModel::from(vec![true])));
    let received = Rc::new(RefCell::new(Vec::new()));
    let callback = received.clone();
    ui.on_retry_attachment(move |id, filename| callback.borrow_mut().push((id, filename)));
    ui.show().unwrap();
    let width = 414;
    window.set_size(PhysicalSize::new(width as u32, 736));
    let mut pixels = vec![Rgb8Pixel::default(); width * 736];
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width);
    }));

    // Find the painted button inside the outgoing bubble, below the chat header.
    let fill = Rgb8Pixel {
        r: 0x12,
        g: 0x38,
        b: 0x4a,
    };
    let hit = (239..480)
        .find_map(|y| {
            (width / 2..width - 45)
                .find(|&x| pixels[y * width + x..][..45].iter().all(|p| *p == fill))
                .map(|x| LogicalPosition::new((x + 15) as f32, (y + 7) as f32))
        })
        .expect("visible retry button");
    let button = PointerEventButton::Left;
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: hit,
        button,
    });
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: hit,
            button,
        });
    assert_eq!(
        received.borrow().as_slice(),
        &[(SharedString::from("47"), SharedString::from("notes.txt"))]
    );
}
