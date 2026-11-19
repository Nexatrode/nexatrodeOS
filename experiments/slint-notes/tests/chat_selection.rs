#![cfg(feature = "full-suite-ui")]

use personal_os_slint_notes_spike::NotesWindow;
mod platform;

use slint::platform::WindowAdapter;
use slint::{ComponentHandle, ModelRc, PhysicalSize, SharedString, VecModel};
use std::rc::Rc;

#[test]
fn rejected_chat_selection_keeps_the_composer_closed() {
    use slint::LogicalPosition;
    use slint::platform::{PointerEventButton, WindowEvent};

    let window = platform::install();
    let ui = NotesWindow::new().unwrap();
    ui.set_page(6);
    ui.set_message_service_ready(true);
    ui.set_message_chats(ModelRc::new(VecModel::from(vec![SharedString::from(
        "Other",
    )])));
    ui.set_message_chat_index_map(ModelRc::new(VecModel::from(vec![1])));
    ui.set_message_selected("Original".into());
    ui.set_message_body("unsent draft".into());
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(414, 736));
    window.draw_if_needed(|renderer| {
        renderer.render(&mut vec![slint::Rgb8Pixel::default(); 414 * 736], 414);
    });

    let attempts = Rc::new(std::cell::Cell::new(0));
    let selected = attempts.clone();
    ui.on_open_chat(move |index| {
        assert_eq!(index, 1);
        selected.set(selected.get() + 1);
        selected.get() == 2
    });
    let position = LogicalPosition::new(120., 285.);
    let button = PointerEventButton::Left;
    let tap_other = || {
        window
            .window()
            .dispatch_event(WindowEvent::PointerPressed { position, button });
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased { position, button });
    };
    tap_other();
    assert_eq!(attempts.get(), 1);
    assert!(!ui.get_message_view());
    assert_eq!(ui.get_message_selected(), "Original");
    assert_eq!(ui.get_message_chat_index(), 0);
    assert_eq!(ui.get_message_body(), "unsent draft");

    tap_other();
    assert!(ui.get_message_view());
    assert_eq!(ui.get_message_selected(), "Other");
    assert_eq!(ui.get_message_chat_index(), 1);
}
