# Fake modem spike (U6)

Software-only carrier telephony stub on the **default serial** image. Not SMS
identity for Messages (see [explicit-contact-messaging.md](explicit-contact-messaging.md)).

## What ships

- Passive PD `fake_modem` (`rust_fake_modem.elf`).
  - Supervisor channel (`client` id 12 / modem id 0): `REQUEST_RING` /
    `REQUEST_SMS` only.
  - Phone channel (phone id 1 / modem id 1): `REQUEST_DIAL` only (1–16 digits).
- Boot: `MODEM|READY: fake telephony stub online`
- Supervisor smoke in `./dev test`: `MODEM|RING`, `MODEM|SMS IN|HELLO`, reject
  empty SMS; supervisor `REQUEST_DIAL` rejected; then push counts to Settings
  (`ALERTS RING 1 SMS 1`); isolation count is **29**.
- Console (mediated): `modemring`, `modemsms BODY` (1–16 alphanumeric) — also
  refreshes Settings.
- Settings PD (`settings_app`, channel 13): line 4 is `NO ALERTS YET` or
  `ALERTS RING N SMS M` after supervisor `REQUEST_SET_ALERTS`.
- Phone PD (`phone_app`, channel 14 alerts + id 1 dial): digit pad with local
  `DIAL` / `CALLING`; CALL packs digits and `pp_call`s modem `REQUEST_DIAL`
  (`MODEM|CALL|<digits>`). Alert counts still come from the supervisor.

## Pass

`./dev test` green with modem PASS lines plus `SETTINGS|PASS` / `PHONE|PASS`
alert counts after modem smoke. `./dev test-input` Settings frames show
`ALERTS RING 1 SMS 1`; Phone covers `ALERT`, touch/keyboard `DIAL` /
`CALLING` with `MODEM|CALL|5551234`, and CLR back to alert.

## Out of scope

AT/Qualcomm realism, auto-timer RING, Messages vault SMS, real carrier
signaling / call audio. Graphics+input+net is unblocked via shared
`timer_driver` (`./dev test-input-net`).
