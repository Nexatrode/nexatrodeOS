# Fake modem spike (U6)

Software-only carrier telephony stub on the **default serial** image. Not SMS
identity for Messages (see [explicit-contact-messaging.md](explicit-contact-messaging.md)).

## What ships

- Passive PD `fake_modem` (`rust_fake_modem.elf`); only the supervisor holds the
  channel (`client` id 12).
- Boot: `MODEM|READY: fake telephony stub online`
- Supervisor smoke in `./dev test`: `MODEM|RING`, `MODEM|SMS IN|HELLO`, reject
  empty SMS; then push counts to Settings (`ALERTS RING 1 SMS 1`); isolation
  count is **29**.
- Console (mediated): `modemring`, `modemsms BODY` (1–16 alphanumeric) — also
  refreshes Settings.
- Settings PD (`settings_app`, channel 13): line 4 is `NO ALERTS YET` or
  `ALERTS RING N SMS M` after supervisor `REQUEST_SET_ALERTS`.
- Phone PD (`phone_app`, channel 14): read-only Home card showing
  `IDLE\nRING 0\nSMS 0` or `ALERT\nRING N\nSMS M` from the same supervisor push.

## Pass

`./dev test` green with modem PASS lines plus `SETTINGS|PASS` / `PHONE|PASS`
alert counts after modem smoke. `./dev test-input` Settings frames show
`ALERTS RING 1 SMS 1` and Phone shows `ALERT\nRING 1\nSMS 1`.

## Out of scope

Digit pad / outgoing CALL UI, auto-timer RING, AT/Qualcomm realism, Messages
vault SMS. Graphics+input+net is unblocked via shared `timer_driver`
(`./dev test-input-net`).
