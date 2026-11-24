# IMG_9718: first-slot ELF continuation works; publication remains delayed

Settled **ELF FIRST SLOT TURN**, source `6f12a709`, image SHA-256 `3e7d2c28e4dd5dae7b02c6bac593740360a89f4c422ffd50e648ae19b4ebde8c`. The raw HEIC and decoded rows are saved privately in the delivered profile's `device-result/`; verification manifest unchanged. Runtime error0.

## The intended hardware ordering is established

Both earlier packets are fully published and retain distinct remote intents: transaction3/intent4 and transaction4/intent10, state3B. Seven normal same-peer admission failures precede the eighth, successful attempt. Both first/latest failed snapshots have count2/gates1.

| Event | Clock | Observation sequence | Earlier owners remaining |
| --- | --- | ---: | ---: |
| Exact transaction3 receipt | 0170CB6D | 8 | 1 |
| Successful normal ELF admission | 0172608B | 9 | 1 |
| Exact transaction4 receipt | 0172DC01 | 10 | 0 |
| Exact ELF transaction5 receipt | 01B1EF75 | 11 | 0 |

**SLOT TURN=1**. ELF admission occurs103.710ms after the first receipt and31.606ms before the second, retaining the original transaction4 ownership. This validates the specific change: the first useful completion gives ordinary service dispatch a turn instead of waiting through both receipts. It does not enlarge the two-reply window, retire the other reply or bypass normal queue/credit checks. IMG9717 admitted after both receipts; its first-receipt-to-admission interval was264.662ms. Different physical boots are not controlled timing benchmarks.

## Overall ELF performance did not improve in this boot

ELF receive/queue/publication/Owner completion: `0149C39B / 01726091 / 01B18AC9 / 01B236EC`.

| Interval | Current boot |
| --- | ---: |
| Acceptance → queue | 2,661.622ms |
| Queue → recorded full publication | **4,139.576ms** |
| Publication → matching Owner completion | 44.067ms |
| Acceptance → matching completion | **6,845.265ms** |

IMG9717's corresponding total was2,995.709ms and queue-to-publication379.822ms. The correct conclusion is a verified scheduling improvement at the first receipt, **not an overall startup speedup**.

Queued-handoff flags0F mean entered/returned without exact ELF publication by its end. Entry/end `01726094 / 0177721F` span332.171ms. Full publication is3,807.402ms after that turn ends. The panel does not identify which writer choice, grant/intent condition, service work or observation boundary accounts for this larger post-queue interval. Do not assign it all to CPU slowness or remote waiting without the corresponding evidence. The short44.067ms publication-to-completion interval is a separate observation.

Transaction3 was consumed6.114ms after publication. Its exact receipt was first visible3,318.889ms after sampled consumption, with160bytes ahead, and processed906.280ms after visibility. The last-absent/first-visible bracket is18.849ms. These are validated FIFO bounds, not exact remote emission times or identities for the160 intervening bytes. No history evictions were reported. Owner completion clocks are later observation sites than the Core receipt journal; their differences are not duplicate or mismatched completions.

## Startup result and next offline question

**IPA COMPLETE1; PD state7FFFFFFF; WLAN RRQ0; WLFW0.** MCFG remains one completed write /one committed byte /zero full reads. This boot's bounded post-matching-ELF MCFG target is absent (stage0 and unknown peer/result). That does not establish there was no earlier MCFG traffic, a missing file or a rejected request.

The useful next target is the newly dominant4.139576s queue-to-publication interval: compare the retained ordinary writer, queued-handoff and service-loop paths with the earlier boots where ELF publication followed queueing promptly. The first-completion scheduling question is now answered on hardware and should not be reprobed as if unresolved. No rerun of this image is needed to establish that result or the unchanged WLAN outcome. No device command was executed while decoding the photo.
