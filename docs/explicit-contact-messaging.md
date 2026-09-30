# Explicit-contact messaging vision (PGP / QR)

Product sketch from 28 September 2026. Not an implementation plan. Aligns with
the offline-first Personal OS and the “rewrite apps for seL4” path—not with
running Linux SimpleX or stock cellular dialer stacks as the identity model.

## Thesis

Treat the radio (cellular data, later Wi‑Fi) as a **dumb pipe**. The phone number
is not the user’s social identity. Contacts are added **explicitly** (e.g. show /
scan a QR code carrying a long-lived PGP public key or equivalent). A routing
service (initially a simple central server, later possibly relays) delivers
messages, and eventually audio/video, only along those agreed links.

Goal: a **small, privacy-forward “dumb smart” phone**—few messages, all wanted,
cryptographically bound to keys the user chose to add. Not an open inbox for the
world.

Carrier expectations (SMS/voice to the MSISDN for account signup, emergency
calling, legal obligations) remain a separate, possibly minimal compatibility
layer and are **not** the primary messaging identity.

## Why this fits Personal OS

| Personal OS bias | How this idea matches |
| --- | --- |
| Explicit authority / seL4 capabilities | Explicit contact add = explicit trust edge |
| Small rewritten apps | Messages UI + crypto agent we own; no Haskell/Linux port |
| Offline-first vault | Keys and threads live in encrypted local storage |
| U6 fake net before Pixel radios | Prototype against Ethernet/NAT or a test relay first |
| Defer browser / app store / spam surfaces | Closed social graph by design |

## Rough architecture (eventual)

1. **Identity:** PGP (or similar) keypair generated on device; public key (+ short
   fingerprint / device label) shown as QR; private key never leaves a narrow
   vault/crypto domain.
2. **Introduction:** In-person (or out-of-band) QR exchange; both sides confirm
   fingerprint; contact record stored in vault (like Contacts, but with key
   material and routing handle).
3. **Transport:** Device uses “data only” when possible. App talks TLS (or better)
   to a **router** that maps “ciphertext for fingerprint F” → connected sessions
   or mailboxes. First implementation may be one trusted server; design should
   allow swapping for multi-relay / store-and-forward later.
4. **Payloads:** Phase A text; Phase B files; Phase C voice/video as RTP-like or
   framed media **after** text trust UX works. Media is still to explicit peers.
5. **Cellular:** Modem optional for IP. SMS/PSTN only if we must satisfy a
   carrier or emergency story—not for day-to-day chat identity.

## Relation to SimpleX Chat

SimpleX already rejects global user IDs and uses invite/QR-style links and
relays. This vision is **philosophically close** (explicit peers, no public
username graph) but **technically different**: we proposed long-lived PGP
identity + a simple central router first; SimpleX uses disposable queue
addresses, its own SMP stack, and Haskell clients we will not run natively on
Microkit.

Practical options later:

- Stay bespoke (PGP + our router + Rust client on Personal OS).
- Interoperate with or learn from SimpleX protocols after U6 (large effort).
- Run upstream SimpleX only inside a future Linux guest, if ever.

See [utm-vs-hardware.md](utm-vs-hardware.md#messaging-and-simplex-chat-long-term).

## Risks and open questions

- **Central server:** metadata (who talks when), availability, coercion; mitigate
  with sealed payloads, minimal logs, later multi-hop relays.
- **Long-lived PGP keys:** compromise and rotation UX; “keys don’t change” is
  simple but harsh on loss/theft—need a deliberate recovery/re-key story.
- **QR in person vs remote:** remote intro needs a trustworthy channel or
  fingerprint verification discipline.
- **Push / battery:** polling vs push wake on Pixel is P6-class work.
- **Audio/video:** far past U5; needs net, media path, and permissions UI.
- **Spam:** explicit add helps; compromised keys or malicious “friends” remain.

## Suggested roadmap placement

| Stage | Work |
| --- | --- |
| U5 | Contacts optional key-fingerprint field (PCT2, hex stub) done; Messages peer-key shell with vaulted PMS2 peers + local PSL1 sealed stubs on Home |
| U6 | VirtIO Ethernet + Personal OS merge + Messages→dhcp_client sealed TX (demo UDP + UI compose UDP/TLS) + host toy router + DNS + TLS 1.2 ECDHE-ECDSA TOFU smoke (`./dev test-net`); `utm-net` dual-UTM Shared Network bundles + console `netpeer`/`netseal`; next: record dual-UTM results / fake modem |
| U7 | Key lifecycle, backup of key material, threat notes |
| Pixel P3+ | Same client over real IP; still not MSISDN-as-identity |
| Later | Voice/video; replace or federate the central router |

Do **not** block calendar/Notes polish on this. It is the north star for
**Messages**. Local seal stubs, host↔guest PSL1 UDP, Messages sealed TX, and a
host toy router exist; remaining messaging batches are richer peer UX and a
real LAN/relay story, not a SimpleX port.
