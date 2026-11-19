use std::{
    env, fs,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use slint::{
    ComponentHandle, LogicalPosition, PhysicalSize,
    platform::{
        Platform, PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType},
    },
};

#[cfg(not(feature = "full-suite-ui"))]
slint::include_modules!();
#[cfg(feature = "full-suite-ui")]
use personal_os_slint_notes_spike::NotesWindow;

struct HostLines<'a> {
    pixels: &'a mut [slint::Rgb8Pixel],
    stride: usize,
}

impl LineBufferProvider for HostLines<'_> {
    type TargetPixel = slint::Rgb8Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: std::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        let start = line * self.stride + range.start;
        render_fn(&mut self.pixels[start..start + range.len()]);
    }
}

#[cfg(feature = "full-suite-ui")]
struct CountingLines<'a> {
    pixels: &'a mut [slint::Rgb8Pixel],
    stride: usize,
    count: &'a mut usize,
}

#[cfg(feature = "full-suite-ui")]
impl LineBufferProvider for CountingLines<'_> {
    type TargetPixel = slint::Rgb8Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: std::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        let start = line * self.stride + range.start;
        *self.count += range.len();
        render_fn(&mut self.pixels[start..start + range.len()]);
    }
}

struct HeadlessPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        Duration::ZERO
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().collect();
    let width = args
        .get(1)
        .and_then(|value| value.to_string_lossy().parse::<u32>().ok())
        .unwrap_or(300);
    let height = args
        .get(2)
        .and_then(|value| value.to_string_lossy().parse::<u32>().ok())
        .unwrap_or(400);
    let output = args
        .get(3)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("slint-notes.ppm"));
    let scale_factor = args
        .get(4)
        .and_then(|value| value.to_string_lossy().parse::<f32>().ok())
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0);
    let keyboard_inset = args
        .iter()
        .find_map(|value| {
            value
                .to_string_lossy()
                .strip_prefix("--keyboard-inset=")?
                .parse::<f32>()
                .ok()
        })
        .filter(|inset| inset.is_finite() && *inset >= 0.0)
        .unwrap_or(0.0);
    let top_inset = args
        .iter()
        .find_map(|value| {
            value
                .to_string_lossy()
                .strip_prefix("--top-inset=")?
                .parse::<f32>()
                .ok()
        })
        .filter(|inset| inset.is_finite() && *inset >= 0.0)
        .unwrap_or(0.0);
    let exercise_input = args.iter().any(|value| value == "--exercise-input");
    #[cfg(feature = "full-suite-ui")]
    let exercise_wallet = args.iter().any(|value| value == "--exercise-wallet");
    #[cfg(feature = "workspace-ui")]
    let exercise_workspaces = args.iter().any(|value| value == "--exercise-workspaces");
    #[cfg(feature = "full-suite-ui")]
    let exercise_suite = args.iter().any(|value| value == "--exercise-suite");
    #[cfg(feature = "full-suite-ui")]
    let profile_dirty = args.iter().any(|value| value == "--profile-dirty");
    #[cfg(any(feature = "workspace-ui", feature = "full-suite-ui"))]
    let page_name = args.iter().find_map(|value| {
        value
            .to_string_lossy()
            .strip_prefix("--page=")
            .map(str::to_owned)
    });
    let compare_line = args.iter().any(|value| value == "--compare-line");
    let reused_buffer = args.iter().any(|value| value == "--reused-buffer");

    let window = MinimalSoftwareWindow::new(if reused_buffer {
        RepaintBufferType::ReusedBuffer
    } else {
        RepaintBufferType::NewBuffer
    });
    slint::platform::set_platform(Box::new(HeadlessPlatform {
        window: window.clone(),
    }))?;
    window
        .window()
        .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
    window.set_size(PhysicalSize::new(width, height));

    let app = NotesWindow::new().map_err(|error| format!("create Slint component: {error:?}"))?;
    app.set_keyboard_inset(keyboard_inset);
    app.set_top_inset(top_inset);
    #[cfg(any(feature = "workspace-ui", feature = "full-suite-ui"))]
    if let Some(name) = page_name.as_deref() {
        app.set_page(match name {
            "home" => 0,
            "notes" => 1,
            "contacts" => 2,
            #[cfg(feature = "full-suite-ui")]
            "calculator" => 3,
            #[cfg(feature = "full-suite-ui")]
            "clock" => 4,
            #[cfg(feature = "full-suite-ui")]
            "calendar" => 5,
            #[cfg(feature = "full-suite-ui")]
            "messages" => 6,
            #[cfg(feature = "full-suite-ui")]
            "phone" => 7,
            #[cfg(feature = "full-suite-ui")]
            "settings" => 8,
            #[cfg(feature = "full-suite-ui")]
            "files" => 9,
            #[cfg(feature = "full-suite-ui")]
            "tools" => 10,
            #[cfg(feature = "full-suite-ui")]
            "quick-settings" => 11,
            #[cfg(feature = "full-suite-ui")]
            "appearance" => 12,
            #[cfg(feature = "full-suite-ui")]
            "lockdown" => 13,
            #[cfg(feature = "full-suite-ui")]
            "lock-screen" => 14,
            #[cfg(feature = "full-suite-ui")]
            "wallet" => 16,
            _ => return Err(format!("unknown page {name:?}").into()),
        });
    }
    #[cfg(feature = "full-suite-ui")]
    if args.iter().any(|value| value == "--contact-detail") {
        app.set_contact_detail(true);
    }
    #[cfg(feature = "full-suite-ui")]
    if args.iter().any(|value| value == "--message-view") {
        app.set_message_view(true);
    }
    #[cfg(feature = "full-suite-ui")]
    if args.iter().any(|value| value == "--message-service-ready") {
        app.set_message_service_ready(true);
    }
    #[cfg(feature = "full-suite-ui")]
    if args.iter().any(|value| value == "--lockdown-preview") {
        app.set_lockdown_preview(true);
    }
    #[cfg(feature = "full-suite-ui")]
    personal_os_slint_notes_spike::suite::wire(&app);
    #[cfg(not(feature = "full-suite-ui"))]
    {
        let weak_app = app.as_weak();
        app.on_save_note(move |_| {
            if let Some(app) = weak_app.upgrade() {
                #[cfg(feature = "workspace-ui")]
                app.set_status("SAVE TAP RECEIVED".into());
                #[cfg(not(feature = "workspace-ui"))]
                app.set_status("SAVED THROUGH TOUCH".into());
            }
        });
    }
    #[cfg(feature = "full-suite-ui")]
    if let Some(note) = args.iter().find_map(|value| {
        value
            .to_string_lossy()
            .strip_prefix("--note=")
            .map(str::to_owned)
    }) {
        app.set_note(note.into());
    }
    #[cfg(feature = "full-suite-ui")]
    for (argument, setter) in [
        ("--contact-name=", 0),
        ("--contact-phone=", 1),
        ("--calendar-item=", 2),
        ("--message-peer=", 3),
        ("--message-body=", 4),
        ("--calc-display=", 5),
        ("--phone-number=", 6),
        ("--phone-status=", 7),
        ("--status=", 8),
        ("--contact-status=", 9),
        ("--calendar-status=", 10),
        ("--files-status=", 11),
        ("--edit-target=", 12),
        ("--message-selected=", 13),
        ("--message-service-state=", 16),
        ("--theme-choice=", 14),
        ("--quick-selected=", 15),
    ] {
        if let Some(value) = args.iter().find_map(|arg| {
            arg.to_string_lossy()
                .strip_prefix(argument)
                .map(str::to_owned)
        }) {
            match setter {
                0 => app.set_contact_name(value.into()),
                1 => app.set_contact_phone(value.into()),
                2 => app.set_calendar_item(value.into()),
                3 => app.set_message_peer(value.into()),
                4 => app.set_message_body(value.into()),
                5 => app.set_calc_display(value.into()),
                6 => app.set_phone_number(value.into()),
                7 => app.set_phone_status(value.into()),
                8 => app.set_status(value.into()),
                9 => app.set_contact_status(value.into()),
                10 => app.set_calendar_status(value.into()),
                11 => app.set_files_status(value.into()),
                12 => app.set_edit_target(value.parse().unwrap_or(0)),
                13 => app.set_message_selected(value.into()),
                14 => app.set_theme_choice(value.into()),
                15 => app.set_quick_selected(value.into()),
                16 => app.set_message_service_state(value.into()),
                _ => unreachable!(),
            }
        }
    }
    #[cfg(feature = "full-suite-ui")]
    for (argument, setter) in [
        ("--wallet-tab=", 0),
        ("--wallet-address=", 1),
        ("--wallet-balance=", 2),
        ("--wallet-send-dest=", 3),
        ("--wallet-send-amount=", 4),
        ("--wallet-send-status=", 5),
        ("--wallet-history=", 6),
        ("--wallet-sync=", 7),
    ] {
        if let Some(value) = args.iter().find_map(|arg| {
            arg.to_string_lossy()
                .strip_prefix(argument)
                .map(str::to_owned)
        }) {
            match setter {
                0 => app.set_wallet_tab(value.parse().unwrap_or(0)),
                1 => {
                    if value.len() >= 95 {
                        if let Some(image) = personal_os_slint_notes_spike::qr::qr_image(&value) {
                            app.set_wallet_address_qr(image);
                            app.set_wallet_address_qr_visible(true);
                        }
                    }
                    let short = if value.len() > 16 {
                        format!("{}…{}", &value[..8], &value[value.len() - 6..])
                    } else {
                        value.clone()
                    };
                    app.set_wallet_address(value.into());
                    app.set_wallet_address_short(short.into());
                }
                2 => app.set_wallet_balance(value.into()),
                3 => app.set_wallet_send_dest(value.into()),
                4 => app.set_wallet_send_amount(value.into()),
                5 => app.set_wallet_send_status(value.into()),
                6 => app.set_wallet_history(value.into()),
                7 => app.set_wallet_sync(value.into()),
                _ => unreachable!(),
            }
        }
    }
    app.show()
        .map_err(|error| format!("show Slint component: {error:?}"))?;
    #[cfg(feature = "full-suite-ui")]
    if profile_dirty {
        let mut pixels = vec![slint::Rgb8Pixel::default(); (width * height) as usize];
        let mut draw = || {
            let mut count = 0usize;
            let start = Instant::now();
            window.draw_if_needed(|renderer| {
                renderer.render_by_line(CountingLines {
                    pixels: &mut pixels,
                    stride: width as usize,
                    count: &mut count,
                });
            });
            (count, start.elapsed())
        };
        let first = draw();
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position: LogicalPosition::new(60.0, 300.0),
            button: PointerEventButton::Left,
        });
        let nav_press = draw();
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position: LogicalPosition::new(60.0, 300.0),
                button: PointerEventButton::Left,
            });
        let nav_release = draw();
        app.set_perf("DRAW 800 MS".into());
        let perf = draw();
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position: LogicalPosition::new(180.0, 410.0),
            button: PointerEventButton::Left,
        });
        let digit_press = draw();
        window
            .window()
            .dispatch_event(WindowEvent::PointerReleased {
                position: LogicalPosition::new(180.0, 410.0),
                button: PointerEventButton::Left,
            });
        let digit_release = draw();
        println!(
            "line pixels/time: first {:?}, navigation press/release {:?}/{:?}, perf {:?}, digit press/release {:?}/{:?}",
            first, nav_press, nav_release, perf, digit_press, digit_release
        );
        return Ok(());
    }
    if exercise_input && !cfg!(any(feature = "workspace-ui", feature = "full-suite-ui")) {
        app.set_note("".into());
        let events = [
            WindowEvent::PointerPressed {
                position: LogicalPosition::new(36.0, 180.0),
                button: PointerEventButton::Left,
            },
            WindowEvent::PointerReleased {
                position: LogicalPosition::new(36.0, 180.0),
                button: PointerEventButton::Left,
            },
            WindowEvent::KeyPressed { text: "h".into() },
            WindowEvent::KeyReleased { text: "h".into() },
            WindowEvent::KeyPressed { text: "i".into() },
            WindowEvent::KeyReleased { text: "i".into() },
            WindowEvent::PointerPressed {
                position: LogicalPosition::new(
                    width as f32 / scale_factor - 55.0,
                    height as f32 / scale_factor - keyboard_inset - 80.0,
                ),
                button: PointerEventButton::Left,
            },
            WindowEvent::PointerReleased {
                position: LogicalPosition::new(
                    width as f32 / scale_factor - 55.0,
                    height as f32 / scale_factor - keyboard_inset - 80.0,
                ),
                button: PointerEventButton::Left,
            },
        ];
        for event in events {
            window.window().dispatch_event(event);
        }
        assert_eq!(
            app.get_note().as_str(),
            "hi",
            "touch focus should route keys into TextInput"
        );
        assert_eq!(
            app.get_status().as_str(),
            "SAVED THROUGH TOUCH",
            "save touch target should fire"
        );
    }
    #[cfg(feature = "full-suite-ui")]
    if exercise_suite {
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window
                .window()
                .dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
        };
        app.set_page(0);
        let logical_height = height as f32 / scale_factor;
        // Notes and the other utility apps are grouped under Tools on Home.
        tap(260.0, logical_height * 0.435);
        assert_eq!(app.get_page(), 10, "Tools card should open Tools");
        tap(60.0, 210.0);
        assert_eq!(app.get_page(), 1, "Notes card should open Notes");
        tap(45.0, 509.0); // Q in the shared keyboard
        assert_eq!(app.get_note().as_str(), "Q");
        tap(350.0, 412.0); // Save
        assert_eq!(app.get_status().as_str(), "HELD IN MEMORY ONLY");
        tap(width as f32 / scale_factor - 60.0, 135.0);
        assert_eq!(app.get_page(), 0);
        tap(260.0, logical_height * 0.435);
        tap(260.0, 210.0);
        assert_eq!(app.get_page(), 3, "Calculator card should open Calculator");
        tap(60.0, 470.0); // 1
        tap(155.0, 470.0); // 2
        assert_eq!(app.get_calc_display().as_str(), "12");
        tap(width as f32 / scale_factor - 60.0, 135.0);
        assert_eq!(app.get_page(), 0);
        tap(260.0, logical_height * 0.225);
        assert_eq!(app.get_page(), 2);
        tap(width as f32 / scale_factor - 45.0, 195.0); // Add contact
        tap(45.0, 509.0); // Q
        assert_eq!(app.get_contact_name().as_str(), "Q");
        tap(50.0, 330.0); // Phone field
        tap(45.0, 460.0); // 1
        assert_eq!(app.get_contact_phone().as_str(), "1");
        tap(width as f32 / scale_factor - 60.0, 412.0); // Save
        assert_eq!(app.get_contact_status().as_str(), "SAVE TAP RECEIVED");
        tap(width as f32 / scale_factor - 60.0, 135.0);
        tap(width as f32 / scale_factor - 60.0, 135.0);
        tap(260.0, logical_height * 0.435);
        tap(60.0, 300.0);
        assert_eq!(app.get_page(), 4);
        tap(width as f32 / scale_factor - 60.0, 135.0);
        tap(260.0, logical_height * 0.435);
        tap(260.0, 300.0);
        assert_eq!(app.get_page(), 5);
        tap(45.0, 509.0);
        assert_eq!(app.get_calendar_item().as_str(), "Q");
        tap(width as f32 / scale_factor - 60.0, 412.0);
        tap(width as f32 / scale_factor - 60.0, 135.0);
        tap(60.0, logical_height * 0.225);
        assert_eq!(app.get_page(), 6);
        tap(50.0, 210.0); // Open first conversation
        tap(80.0, 418.0); // Message composer
        tap(80.0, 509.0); // W
        assert_eq!(app.get_message_body().as_str(), "W");
        tap(width as f32 / scale_factor - 60.0, 135.0);
        tap(width as f32 / scale_factor - 60.0, 135.0);
        tap(260.0, logical_height * 0.435);
        tap(100.0, 390.0);
        assert_eq!(app.get_page(), 7);
        tap(185.0, 410.0); // 5
        assert_eq!(app.get_phone_number().as_str(), "5");
        tap(330.0, 570.0);
        assert_eq!(app.get_phone_status().as_str(), "NO MODEM · DEMO ONLY");
        tap(width as f32 / scale_factor - 60.0, 135.0);
        tap(100.0, logical_height * 0.605);
        assert_eq!(app.get_page(), 8);
        app.set_page(0);
    }
    #[cfg(feature = "workspace-ui")]
    if exercise_workspaces {
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window
                .window()
                .dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
        };
        app.set_page(0);
        tap(50.0, 250.0);
        assert_eq!(app.get_page(), 1, "Notes card should open Notes");
        tap(
            width as f32 / scale_factor - 65.0,
            height as f32 / scale_factor - 60.0,
        );
        assert_eq!(app.get_status().as_str(), "SAVE TAP RECEIVED");
        tap(width as f32 / scale_factor - 65.0, 170.0);
        assert_eq!(app.get_page(), 2, "People tab should open Contacts");
        tap(50.0, 170.0);
        assert_eq!(app.get_page(), 0, "Home tab should return to launcher");
        tap(
            50.0,
            218.0 + 16.0 + ((height as f32 / scale_factor - 320.0) / 2.0).max(72.0) + 35.0,
        );
        assert_eq!(app.get_page(), 2, "Contacts card should open Contacts");
        app.set_page(0);
    }

    #[cfg(feature = "full-suite-ui")]
    if exercise_wallet {
        let tap = |x, y| {
            let position = LogicalPosition::new(x, y);
            window.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
        };
        let logical_height = height as f32 / scale_factor;
        let logical_width = width as f32 / scale_factor;
        app.set_page(16);
        app.set_wallet_tab(2);
        // Address field, then the "4" key on the number row.
        tap(logical_width / 2.0, 154.0);
        assert_eq!(app.get_edit_target(), 11, "address field should focus");
        let key_y = logical_height - 240.0 + 5.0 + 19.5;
        let slot = (logical_width - 10.0) / 10.0;
        tap(5.0 + 3.0 * slot + (slot - 2.0) / 2.0, key_y);
        assert_eq!(app.get_wallet_send_dest().as_str(), "4");
        tap(logical_width / 2.0, 270.0);
        assert_eq!(app.get_edit_target(), 12, "amount field should focus");
        tap(5.0 + (slot - 2.0) / 2.0, key_y);
        let decimal_y = logical_height - 240.0 + 181.0 + 19.5;
        let decimal_x = 5.0 + (logical_width - 20.0) / 6.0;
        tap(decimal_x, decimal_y);
        assert_eq!(app.get_wallet_send_amount().as_str(), "1.");
        tap(5.0 + (slot - 2.0) / 2.0, key_y);
        assert_eq!(app.get_wallet_send_amount().as_str(), "1.1");
        tap(logical_width / 2.0, 398.0);
        assert_eq!(
            app.get_wallet_send_status().as_str(),
            "No wallet service on this phone"
        );
        assert!(!app.get_wallet_fee_ready(), "preview must not invent a fee");
        tap(logical_width / 2.0, 454.0);
        assert_eq!(
            app.get_wallet_send_status().as_str(),
            "No wallet service on this phone"
        );
        tap(1.5 * logical_width / 4.0, logical_height - 32.0);
        assert_eq!(app.get_wallet_tab(), 1, "Receive tab should open");
        tap(logical_width / 2.0, 600.0);
        assert_eq!(app.get_wallet_sync().as_str(), "Address copied");
        assert!(app.get_wallet_address().starts_with("4A1ZVG4S"));
        println!("wallet exercise: type, preview without a fee, copy address");
    }

    if let Some(click) = args.iter().find_map(|value| {
        value
            .to_string_lossy()
            .strip_prefix("--wallet-click=")
            .map(str::to_owned)
    }) {
        let mut parts = click.split(',');
        let x = parts.next().unwrap_or("0").parse::<f32>().unwrap_or(0.0);
        let y = parts.next().unwrap_or("0").parse::<f32>().unwrap_or(0.0);
        let position = LogicalPosition::new(x, y);
        window.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        #[cfg(feature = "full-suite-ui")]
        println!("wallet-click {x},{y} tab {}", app.get_wallet_tab());
    }

    window.window().request_redraw();

    let mut pixels = vec![slint::Rgb8Pixel::default(); (width * height) as usize];
    let mut region = None;
    let drew = window.draw_if_needed(|renderer| {
        region = Some(renderer.render(&mut pixels, width as usize));
    });
    if !drew {
        return Err("Slint did not request an initial frame".into());
    }

    if compare_line {
        let mut line_pixels = vec![slint::Rgb8Pixel::default(); pixels.len()];
        window.window().request_redraw();
        let line_drew = window.draw_if_needed(|renderer| {
            renderer.render_by_line(HostLines {
                pixels: &mut line_pixels,
                stride: width as usize,
            });
        });
        if !line_drew || line_pixels != pixels {
            let mismatches = pixels
                .iter()
                .zip(&line_pixels)
                .filter(|(a, b)| a.r != b.r || a.g != b.g || a.b != b.b)
                .count();
            return Err(format!("full and line render differ at {mismatches} pixels").into());
        }
        println!("full and line render match at every pixel");
    }

    let mut image = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in pixels {
        image.extend_from_slice(&[pixel.r, pixel.g, pixel.b]);
    }
    fs::write(&output, image)?;
    println!(
        "rendered {width}x{height} at scale {scale_factor} with keyboard inset {keyboard_inset} to {} ({:?}){}",
        output.display(),
        region.unwrap(),
        if exercise_input {
            "; touch focus, typing, and save passed"
        } else {
            ""
        }
    );
    Ok(())
}
