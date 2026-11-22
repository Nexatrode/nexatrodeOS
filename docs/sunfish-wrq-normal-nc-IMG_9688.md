# WRQ TABLE NORMAL NC cold boot — IMG_9688

The user supplied a settled matching panel after the temporary cold boot. Runtime error is zero. The experiment reduced comparable table-validation time, but did not produce the first DATA or move WLAN/WLFW startup.

| Measurement | Device baseline IMG_9687 | Normal-NC IMG_9688 |
| --- | ---: | ---: |
| Table comparisons before WRQ return, 52 checks in each | 374.681 ms | 175.608 ms |
| Table time per check before return | 7.205 ms | 3.377 ms |
| WRQ visibility → buffer return | 889.469 ms | 594.554 ms |
| Buffer return → OACK publication | 665.875 ms | 837.496 ms |
| Checks after buffer return | 40 | 77 |
| Table time per check after return | 7.189 ms | 3.396 ms |
| WRQ visibility → modem OACK read | 1,594.323 ms | 1,458.303 ms |
| OACK publication after close first visible | 42.169 ms | 218.576 ms |
| Modem OACK read after close first visible | 81.147 ms | 244.828 ms |

Comparable table work improved by about **53%**. The measured first stage improved by 294.915 ms. The post-return stage grew by 171.621 ms and contains 77 checks instead of 40; its aggregate duration is not a like-for-like per-check performance comparison. Total OACK-read latency improved by 136.020 ms, while the close also became visible sooner relative to WRQ. Therefore the OACK still missed this boot's observed close window, by more than in the baseline. These host FIFO markers are not an independent measurement of the proprietary modem timer origin.

The first DATA marker remains `FFFFFFFF`. IPA COMPLETE is `1`, WLAN RRQ RX is `0`, and WLFW SERVERS is `0`. History loss and missing native/category sets are zero. IPA completion was already observed before this experiment; it is not a new WLAN milestone.

The result supports the table-memory performance hypothesis on this handset. It does not establish equivalent transient ordering/fault behavior for every Normal-memory interleaving, nor does it show working Wi-Fi. The next useful offline comparison is why the post-return window performed 37 additional checks and what work occurred before registration/OACK publication. Preserve the checks and grant/FIFO ordering while identifying the repeated work; another identical memory-type probe is unnecessary.

Evidence is archived privately under `.cache/sunfish-native-wifi/private/native-wrq-table-normal-nc-v1/device-result`: original HEIC, converted full view, marker/cost crops, `decoded-IMG_9688.json` and `result-manifest.json`. Both phase clocks, RX/TX/other sums and category completeness were checked. Independent elapsed rows and absolute marker subtraction can differ by 1 μs; category sums by a few μs. The build manifest remains the pre-boot artifact record; the separate result manifest records the subsequent user-reported physical boot. The photo is not an independent image-hash readback from the phone.
