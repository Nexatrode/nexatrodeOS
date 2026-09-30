# VirtIO input feasibility spike

The pinned sDDF checkout has no VirtIO input driver. `tests/input/driver.c`
therefore provides a narrow QEMU-only driver for a VirtIO keyboard and tablet.
It owns the MMIO queues and IRQs, then publishes records into a 32-entry
single-producer/single-consumer ring shared only with a separate Rust input
service. The active Rust service has an MCS budget, validates device/type/code/
value combinations, and sends validated events over separate protected
endpoints to isolated Notes, Contacts, or Calculator apps. Apps receive neither
device registers nor the raw ring. Focus starts empty. The trusted router maps
absolute tablet coordinates to seven visible launcher cards and opens an app on
a left-button press inside a card. In an app, a visible top bar switches between
Notes, Contacts, Calculator, Clock, Calendar, Messages, Settings, and Home;
Escape returns
to Home from a physical keyboard. Tab cycles Notes → Contacts → Calculator →
Clock → Calendar → Messages → Settings → Notes, starting at Notes, and the
router consumes
it. Clock and Settings remain read-only: the router does not forward app key
events to them. Messages receives routed input for its offline peer-key shell. The
300x400 launcher also shows a QWERTY touch keyboard with Space and Backspace;
each tap becomes a keyboard press/release pair delivered only to the active
app. A key release or repeat stays with the app that received its press, even
if focus changes while the key is held. The router writes the current app into
one shared 32-bit value; only the router can write it, while the GPU client
maps it read-only. A screen change notifies the graphics client, which draws
the full app view without a polling timer. Before changing to a writable app,
the router sends that app a bounded activation event so it can load its private
saved view before the GPU reads the snapshot. Clock needs no activation event;
the RTC service refreshes its snapshot each second. Messages is an interactive
peer-key shell with vaulted PMS2 sealed stubs. Settings needs
no activation
event either; each publishes a fixed display snapshot at startup.

Notes and Contacts each own a separate bounded text snapshot. Their app process
alone can write its snapshot; the display client maps both read-only and renders
the focused app's text. Notes stores up to four independent records, each with
a 21-character title and 128-byte body, in the encrypted `PNT2` collection.
Older raw single-note records and `PNT1` body-only collections migrate by using
the first body line as the title. The touch list opens a note, creates one, and
requires a second Delete tap to remove it. In the editor, LIST returns to the
collection, TITLE and BODY switch between independently editable fields, and
Older/Newer scroll the body's three-row view. The router sees only a shared
Notes mode word for list-row hit-testing; it does not map note contents.
Viewport position remains transient, while the selected note is saved when
opened.

Calculator runs in its own protection domain with a third read-only display
snapshot. It supports bounded signed `i64` integer entry, chained addition,
subtraction, multiplication, division, backspace, clear, and equals; overflow
and divide-by-zero appear as visible errors. Touchscreen keypad buttons become
the same routed key events as a physical keyboard. Calculator state and history
are currently transient.

The Clock screen is a read-only snapshot written by the RTC-owning clock
service, formatted as UTC `YYYY MM DD` plus `HH MM SS`. The GPU client maps
that page read-only, and the input router has no mapping to the clock snapshot.

Run `./dev test-input` to build the combined Microkit image and inject a key
press/release plus absolute X/Y and left-button events through QMP. It taps the
Notes panel, checks physical typing, taps the on-screen `N`, Backspace, Space,
and Backspace keys, uses Tab and the app tabs to switch between screens, returns
Home by touch and Escape, then opens Contacts, Calculator, Clock, Calendar,
Messages, and Settings from the launcher. QEMU must render the exact 300 x 400
launcher, Notes,
Contacts, Calculator, Clock, Calendar, Messages, and Settings frames, including
separate
title and body editing. The
Clock check masks its live date and time digit rows and compares the rest of
the frame exactly. The test types a four-row body, checks the
Older and Newer controls against exact frames, creates a second note, edits its
title and body independently, and verifies both survive a cold boot. It also
checks confirmed deletion, then creates two Contacts entries
using touchscreen letters and digits, and browses and deletes an entry with
on-screen controls. Switching apps proves their snapshots remain separate. It
also checks all 25 regular OS checks and the absence of guest faults. A host
test fills the ring,
checks explicit overflow accounting, and verifies counter wrap; Rust policy
tests reject unsupported event combinations, malformed routed records, and
touches outside the app panels. The test uses a disposable vault disk.
`./dev utm-graphics` exports the same system with UTM VirtIO GPU, keyboard, and
tablet devices in `dist/Personal OS Graphics.utm`; the default serial profile is
unchanged. Keep UTM on **USB Tablet** (absolute) mode; the guest draws an amber
cursor from the last tablet position so clicks are visible without a VirtIO
mouse. The bundle includes the touch keyboard, Contacts browse/delete
controls, Notes title/body editing, list/add/delete and Older/Newer body
scrolling, the Calculator keypad, the read-only Clock screen, the Calendar
reminder list (title plus optional MMDD day and HHMM time), the Messages
peer-key shell,
and the Settings stub. QEMU verifies
`7 + 5 = 12` through touch and `8 + 2 = 10` through the physical keyboard; it
also checks clock/calendar/messages navigation and exact frames while masking
the live date/time fields. The profile uses native resolution so the 300 x 400
touch targets map consistently.

The scripted QEMU check demonstrates bounded driver-to-service handoff,
visible-surface hit-testing, per-app protected endpoints, focused-only routing,
and a notification-driven focus indicator. The ring rejects new events when
full and counts those drops; the consumer and app endpoints reject malformed
records. Notes persists up to four bounded titles and 128-byte bodies through
its fixed encrypted blob endpoint. The editor wraps body text, shows three
rows, accepts Enter from both the physical and touchscreen keyboards, and lets
Older/Newer browse content outside the visible rows. The Notes list can open,
add, and confirmed-delete entries; legacy raw notes and `PNT1` collections
migrate to `PNT2` with first-line titles, and QEMU checks titles, bodies, the
collection, and selected note after cold boot.
Contacts stores up to four entries, each with a 12-character uppercase name, up
to 16 phone digits, and an optional 8-character hex key fingerprint (`A-F` /
`0-9`). Enter starts and advances through name, phone, and key fields; empty
key commits with `KEY -`. Up/Down selects entries, and Delete removes one. The
on-screen keyboard includes a number row, so names, phones, and fingerprints can
be entered by touch. QEMU checks exact editing frames and restores Notes and
Contacts (including fingerprints) after a cold boot. Broader
keymaps, touch accessibility, and production VirtIO input drivers remain future
work.
VirtIO input defines event and status queues, with event type/code/value fields
modeled after evdev ([OASIS VirtIO 1.2, input device](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html#x1-2630005)).
