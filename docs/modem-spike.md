# Fake modem spike (U6)

Software-only carrier telephony stub on the **default serial** image. Not SMS
identity for Messages (see [explicit-contact-messaging.md](explicit-contact-messaging.md)).

## What ships

- Passive PD `fake_modem` (`rust_fake_modem.elf`); only the supervisor holds the
  channel (`client` id 12).
- Boot: `MODEM|READY: fake telephony stub online`
- Supervisor smoke in `./dev test`: `MODEM|RING`, `MODEM|SMS IN|HELLO`, reject
  empty SMS; isolation count is **28**.
- Console (mediated): `modemring`, `modemsms BODY` (1–16 alphanumeric).

## Pass

`./dev test` green with `MODEM|READY`, `MODEM|RING`, `MODEM|SMS IN|HELLO`, and
`MODEM|PASS: RING and SMS inject accepted`.

## Out of scope

Phone/dialer UI, Settings alert counts, auto-timer RING, AT/Qualcomm realism,
graphics+net merge (`timer_driver` clash), Messages vault SMS.
