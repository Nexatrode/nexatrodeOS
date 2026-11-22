# Normal-NC table pair reads

IMG_9691 showed the metadata batch working, with runtime error0 and complete
caller data. Post-return samples fell77→72, their measured check time fell
618→563ms, and WRQ return→OACK publication fell838→800ms. Publication and peer
read were still203/230ms after exact close visibility. IPA completed but first
DATA, WLAN RRQ and WLFW remained absent. These are single boots, not a repeated
statistical benchmark.

The earlier complete Normal-NC breakdown measured261.502ms in full table scans
after WRQ return. The next opt-in change targets the cost per scan. It retains
all4096 expected-word comparisons at every domain boundary, reading two words
at a time from each of the eight exclusively owned Normal-NC RAM pages.
It does not retain a successful validation across operations or remove a FIFO
sample. The outer fault/ASID/route/register validation, table entry/exit barriers,
metadata batch, receive/consume/transmit checks and grant/credit ordering remain
unchanged.

`sunfish-smmu-normal-nc-table-pairs` depends on the existing Normal-NC,
metadata-batch and stack-split feature chain. Its private helper rejects null
and misaligned pointers before access. For a valid pointer,256 volatileu128
reads cover exactly4096 bytes; each compares both expected native-endianu64
values. The caller must retain a readable page,16-byte alignment, Normal-NC
mapping and no concurrent CPU/device writer for the retained lifetime.
All eight frames, including Native::new's four original frames, carry that
explicit public unsafe staging obligation. The actual SDK and packed-map audit
prove it for this image; a Cargo feature alone cannot prove a memory mapping.

The candidate AArch64 helper uses general-purpose register `ldp` for both the
expected and actual word pair, compares both values, decrements a4096-byte
counter by16 and advances both pointers by16. It has null/alignment guards,
no calls, and no new SIMD/FPU requirement. Volatileu128 source does not promise
one atomic hardware access or stable instruction selection. Rust documents
that larger volatile loads may split, so the exact candidate ELF and helper
disassembly are hash-bound and reviewed. This is table RAM only; device MMIO
accessors are unchanged. See [Rust volatile-load semantics](https://doc.rust-lang.org/std/ptr/fn.read_volatile.html)
and [Arm Normal memory](https://support.arm.com/documentation/102376/latest/Normal-memory).

The primitive test flips every bit of every word:32,768 corruptions are rejected,
and a restored page succeeds. Actual Native integration separately flips every
one of4096 words across the eight real fixture table bindings and rejects16
register/route/context/fault mutations, in both candidate and baseline profiles.
Native pre/post-domain fault quarantine and ten existing transport fault controls
pass. The existing metadata tests remain active. Two source-contract tests pin
the all-eight-frame mapping obligation and feature dependency.

Independent review found one important documentation gap: the older public
staging contract admitted generic uncached memory. That contract was tightened
to Normal-NC when pair reads are enabled, with its regression observed RED→GREEN.
The corrected source rebuilt to a byte-identical candidate ELF. No critical
issue or deferred blocking finding remains. Hardware speed remains unmeasured.

Offline checks include564 owner tests versus563 in the prior profile,212 link
tests, nine handoff/parser tests,31 runtime-source tests,19 source guards and
the focused Normal-NC/source-pin suites. All24 paired replay schedules match
full FIFO I/O digests and peer-event timing. Fake replay does not execute the
native table reader; the actual Native corruption tests cover that path.
Selected stack demand remains4,290,304 bytes, leaving952,576 bytes in the same
5MiB mapping. This is a selected-frame budget, not whole-program stack proof.
The prior unrelated SMMU model78-versus128 fixture failure remains recorded in
the metadata-batch notes and is not claimed passing or fixed here.

The image passed304 capability mutation tests. The pinned SDK, eight packed
Normal-NC maps, complete loader/CapDL/report reproduction, boot envelopes,
caller source map and payload hashes are separate delivery gates. No SDK,
MMIO authority, stack mapping or allocated protocol RAM policy change is added.
Prior images and handset evidence remain intact.

The private profile is
`.cache/sunfish-native-wifi/private/native-wrq-table-pairs-v1`, with panel title
`WRQ TABLE PAIR READS`. Its temporary boot image is68,911,104 bytes, SHA-256
`01a00d37db989e179a3b7dd127dae2bbe883979f0057f7a7c03ba347728b08e8`.
The next handset test should measure lower time per retained check, rather than
a predetermined reduction in sample count. DATA, WLAN firmware loading and
working Wi-Fi remain outcomes to demonstrate; no speedup is claimed yet.
