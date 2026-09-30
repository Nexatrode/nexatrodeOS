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

## Pass

`./dev test` green with modem PASS lines plus `SETTINGS|PASS: alert counts
pushed after modem smoke`. `./dev test-input` Settings frames show
`ALERTS RING 1 SMS 1` after boot smoke.

## Out of scope

Phone/dialer UI, auto-timer RING, AT/Qualcomm realism, graphics+net merge
(`timer_driver` clash), Messages vault SMS.
