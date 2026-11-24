# Bounded registry publication between intent grants

IMG_9701 showed transaction3's88-byte registry reply waiting10.053287seconds before publication. Of81 recorded first-priority observations,78 encountered a pending intent ACK. While the reply remained unpublished,26ACKs,26receive returns and26advertisements committed. A fitting modem receive intent was observed on every recorded publication call. The longest completed ACK took445ms. This is repeated control pressure, rather than evidence of one ACK hanging for ten seconds. See `sunfish-registry-pressure-IMG_9701.md` for the exact observations and limits.

The optional `sunfish-registry-between-grants` feature allows one registry DATA attempt after an actual intent ACK committed and before the next fresh positive grant's backing advertisement. The normal registry selector must find a ready reply whose own request buffer has been returned. Protected TFTP/first-WRQ turns, the registry cooldown and ready QRTR credits still exclude the opportunity. An already-advertised positive grant keeps its ACK ahead of registry DATA. Negative ACKs are not borrowed across.

The opportunity belongs to the live transmit record. It is consumed immediately before calling the normal checked writer, including a known blocked write. The next attempt follows ordinary advertisement/ACK/fallback scheduling. An uncertain publication remains quarantined. Retirement or transport reset clears the latch together with the transmit record. Identity, outstanding-reply order, suitable remote intent selection, actual QRTR credit reservation, receipt matching and the validated FIFO writer are unchanged. Diagnostics and clocks do not control eligibility.

The saved Linux GLINK handler allocates, advertises and then ACKs a new receive intent. The Android handler can ACK an already-advertised reusable intent, otherwise it also allocates, advertises, then ACKs. This change preserves that ordering for every grant; it inserts the bounded DATA opportunity before a fresh backing advertisement, never between its advertisement and ACK. These sources constrain protocol behavior, not the handset's native scheduling timeline.

The new panel is **REGISTRY BETWEEN GRANTS**, reusing the27-field predecessor-pressure layout. `OLD MASK` retains bits0..7 and adds bit8 (`0x100`) for an actual borrowed writer attempt and bit9 (`0x200`) for its successful DATA commit. A blocked attempt sets only bit8. The old gate counters retain their original first-condition precedence: `ACK RXDONE` can count an ACK present even when this new exception actually lends a turn. Use the new mask bits to identify that exception. The counters are occurrence counts, not durations or a complete eligibility history. The predecessor snapshot freezes at actual full publication, survives its matching completion/retirement and clears on reset.

Offline checks cover the exact reply-before-next-advertisement boundary, already-advertised ACK protection with a newly queued reply, own request return, ready QRTR credit, missing suitable modem intent, byte-identical blocked fallback, uncertain publication quarantine and reuse of the same session after a generation reset. The prior committed scheduler fails the focused boundary replay by choosing a receive advertisement where the new test expects the exact registry reply. No model tick is treated as a predicted handset duration.

Delivery profile: `.cache/sunfish-native-wifi/private/native-registry-between-grants-v1`. Its private manifest binds the actual ELF, boot image, sources, stack budget, capability audit and test artifacts. Previous images remain preserved. No device commands are executed by the build or test work.

The next physical test asks whether this opportunity occurs in the real repeated-grant traffic and reduces transaction3/transaction4/ELF delay. It does not establish WLAN firmware loading, WLFW startup or working network traffic until those outcomes appear on hardware.

Verified delivery:68,915,200bytes, SHA256 `1cab2be8115886d6be9e406a0f45ecfb006486ed7c8bf01e219ec266507e06fd`. Current/prior owner suites575/575pass with one prior ignored each; Core/link230/223pass; seven new tests exercise nine boundary cases. The prior-code replay fails at the intended boundary. The24paired owner schedules retain exact I/O parity; the dedicated tests separately exercise the changed grant boundary. Native stack audit retains946400bytes selected headroom in5MiB; this is not whole-program worst-case stack certification. All304negative capability mutations are rejected, all3637named protocol-frame physical bindings are unchanged, and the actual packed initializer contains24539objects and exactly eight NC table mappings. Pinned SDK reuse and complete payload reproduction are verified.

After fully powering off and entering bootloader fastboot with Volume Down + Power, run:

```sh
cd /Volumes/untitled/sel4-personal-os &&
"$HOME/Library/Android/sdk/platform-tools/fastboot" -s 02281FQC200058 boot \
  .cache/sunfish-native-wifi/private/native-registry-between-grants-v1/fastboot.img
```

Wait for **REGISTRY BETWEEN GRANTS** and **RESTART WHEN DONE**, then photograph the whole settled panel. This temporarily boots the image; the command does not flash a partition.
