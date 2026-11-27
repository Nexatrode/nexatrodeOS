# IMG_9808: ELF ready loan did not run; credits excluded the remaining windows

Settled **ELF READY GRANT TURN**, source `26a529c2f7737e6d03b1c6ceaee7cea5277da99a`, image SHA256 `5e88b61f8472037a1ed3a8b3544578ebe99be39998955efe063faa3585127692`. All95 delivered artifact hashes were reverified unchanged. Raw photo, full/readable PNG, manual rows, decoder and hashes are separately retained under `device-result/`. Error/detail0, close stage6, no pending close. No Pixel commands ran during analysis.

## No new callback attempt

ELF publication calls2E=46, mask24AC, transaction5/88bytes. Bits19/20 are absent: no new ready DATA callback attempt or commit occurred. The title only identifies the enabled policy. It does not prove the private binding state or an eligible window, neither of which is directly exported here.

The first-priority gates sum46: TFTP5, pending intent ACK32, ready-credit exclusion8, eventual selection1; all cooldown, own-return, no-candidate and different-registry gates0. Independent borrowed-grant gates also sum46: no ACK14, negative ACK8, missing backing12, credit12; all missing-yield, cooldown, TFTP/WRQ/own-return, used-opportunity, missing-candidate and selected gates0. These are two alternative first-gate classifications, not additive packet counts. Missing backing is the actual guard result; it must not be equated universally with an advertisement already read by the modem.

In the remaining positive-grant candidate checks, the ready credit condition prevented the DATA loan. This observation motivates replaying a bounded credit→ELF opportunity, but does not prove the binding flag from the screenshot or identify each credit's route. The current converted-credit helper accepts only transaction4; it cannot supply that credit opportunity to the exact ELF transaction5.

## Timing and retained records

ELF acceptance/queue/publication/Owner completion `014A0475 / 0159AAE1 / 01A4B486 / 01A974DA`:1,025.644ms prequeue, **4,917.669ms queue→publication**,311.380ms publication→completion,6,254.693ms total. Queue→publication is1,901.214ms shorter than IMG_9807's6,818.883ms, but no new loan ran. Do not attribute that difference to the loan or call it a controlled improvement. Prequeue increased952.478ms, so the whole accepted→completed interval improved681.357ms, not1.901seconds.

46 calls are timed; finished1/active0/regressed0. First entry015A0A43, last end01A4B479. Completed Core writer spans total1,674.028ms, leaving3,243.641ms outside those spans within queue→publication; maximum inter-call gap219.556ms. The remainder includes interleaved work and instrumentation, not demonstrated idle time. Eight first4/latest4 records remain,38 omitted. Retained callbacks all returned1 and had fitting buffers at their end checks: RX_DONE45, advertisement, positive ACK, RX_DONE50; later advertisement, positive ACK, QRTR RESUME, exact ELF DATA. This is not continuous availability proof. No blocked/uncertain retained callback is shown.

TX3 Q/P/Core `0130AC10 / 0134A815 / 016D3FA6`; TX4 Q/P/Owner `0145C08F / 0148624E / 016EDA98`. Both older replies completed before ELF publication, but these scalar fields do not allocate its writer wait.

## Successful digest replies and unchanged WLAN

Digest peers4E/4F, reservations21/22, flagsBF, result0, actual ERROR5/code1/19bytes from ports5009/500A, intent0. Both matching completions preceded observed close:88.738ms and175.620ms. These first2 do not establish every later outcome: total12/omitted10. Fourteen close cleanups committed. The response gap fixed in IMG_9807 remains closed for the captured records.

Live IPA flags1F/no pending message; exact ELF host-credit mask7. Original final ACK00D993B8 completed. PD7FFFFFFF, **WLAN RRQ0/WLFW0**; read count22/kind mask2E. No WLAN startup milestone moved. Next work is a finite, normally checked credit→ELF pair with its own spent/committed state, preserving every other ready credit, grant advertisement-before-ACK, blocked-write fallback, request identity and exact completion. No repeat of this unchanged image is required.
