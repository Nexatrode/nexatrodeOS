#![cfg(feature = "full-suite-ui")]

mod platform;

use personal_os_slint_notes_spike::NotesWindow;
use slint::platform::WindowAdapter;
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};

#[test]
fn setup_shows_feedback_but_unlock_never_displays_it() {
    let window = platform::install();
    let ui = NotesWindow::new().unwrap();
    ui.set_device_lock_screen(true);
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(414, 736));

    let render = || {
        let mut pixels = vec![Rgb8Pixel::default(); 414 * 736];
        window.window().request_redraw();
        window.draw_if_needed(|renderer| { renderer.render(&mut pixels, 414); });
        pixels
    };

    ui.set_device_lock_stage(2);
    ui.set_device_lock_setup_feedback("0 characters entered".into());
    let empty_setup = render();
    ui.set_device_lock_setup_feedback("9 characters entered".into());
    assert_ne!(empty_setup, render(), "setup must show the input count");

    ui.set_device_lock_stage(3);
    let nine_in_confirmation = render();
    ui.set_device_lock_setup_feedback("0 characters entered".into());
    assert_ne!(nine_in_confirmation, render(), "confirmation must show the input count");

    ui.set_device_lock_stage(1);
    let unlock = render();
    ui.set_device_lock_setup_feedback("9 characters entered".into());
    assert_eq!(unlock, render(), "unlock must not display setup feedback");
    ui.set_page(6);
    assert!(unlock == render(), "the locked screen must cover content from every app");
    ui.set_aurora_accent(false);
    assert!(unlock != render(), "turning off Aurora Accents must remove the lock backdrop");
}
