# Sunfish table memory-type review

Status: user explicitly approved preparing the isolated experiment on2026-11-21. Implementation and verification follow the corresponding plan; no device action is authorized to the agent. Source work remains directly on main.

IMG_9687 measured662.252ms in table comparisons and164.247ms in route scanning from original WRQ visibility through OACK publication. The modem-read marker was1,594.323ms after first WRQ visibility. Reaching the conditional1-second experiment target by optimizing table time alone would require about89.7% of that measured table cost to disappear. This is a budget, not a prediction or proof of the proprietary modem timer origin. Small private planner bookkeeping reductions cannot be represented as the required saving.

## Established source and mapping facts

The pinned Sunfish kernel is seL46e7c3b733d296cfd88d5fbf635c96e447a882374, Hypervisor support OFF. Its makeUserPagePTE maps cached=false to DEVICE_nGnRnE, not NORMAL_NC. XML sets all eight table RAM frames cached=false. The MAIR slot for Normal-NC already exists; CPU TCR fields describe CPU page-table walks and must not be confused with the SMMU's separate table-walk TCR.

Actual packaged CapDL contains eight4KiB table frames with empty init fill lists, exactly eight final mapping caps, all uncached/NX, in the native-startup owner's page table. This does not by itself prove the absence of every initialization/kernel alias. Any candidate must also trace frame creation/clearing, temporary initializer mappings and subsequent kernel accesses. The baseline kernel direct map and cache maintenance must be accounted for.

The shipped AArch64 verifier already emits scalar64-bit loads and per-word mismatch branches in an unrolled32-word comparison sequence. There is one DSB before the table scan and one after; there is no per-word DSB to remove. The private expectation and grouped-check changes are already enabled. Earlier expected-zero-group host trials are not a demonstrated Pixel deadline fix. Full live reads and early mismatch behavior remain required under the current policy.

## Reusable existing work

The repository already contains explicit Normal-NC support prepared for the iPhone framebuffer: patches/sel4-iphone7plus-explicit-normal-nc.patch, rust-sel4-iphone7plus-explicit-normal-nc.patch, microkit-iphone7plus-explicit-normal-nc.patch and platforms/iphone7plus/build-explicit-normal-nc.sh. Notes record a physical iPhone boot with display/touch behavior preserved. This proves an existing implementation path, not Sunfish table-coherency safety or a speedup.

The explicit path adds VM attribute0x08, carries normal_nc through CapDL/initializer and parser, and selects existing MAIR Normal-NC only on requested uncached AArch64 EL1 maps. A separately rebuilt Sunfish kernel, initializer, tool and SDK would be needed. Globally redefining cached=false is excluded because it would also change MMIO semantics. Cached=true and a new cached alias are excluded from this proposal.

## Contract distinction requiring a decision

Normal-NC keeps data caching disabled and can retain all4096 scalar volatile loads/comparisons, read widths at the instruction level, outer validation/barrier calls and terminal quarantine. Arm Normal-memory semantics nevertheless permit speculation, merging, repetition and reordering of external transactions. It is not an exact Device-nGnRnE transaction replacement. Thus it cannot be certified under a strict requirement to preserve every former live observation/order or every changing/transient corruption interleaving.

A small offline ordering model demonstrates the distinction: a permitted early Normal read of a later word can precede an external change that an ordered Device read would observe. The model is an existence example, not a claim about this Pixel's actual pipeline. The existing sequential baseline is not an atomic whole-table snapshot either. Native persistent-corruption tests and successful iPhone display tests cannot prove all interleavings equivalent.

## Concrete optional experiment

If this memory-model change is explicitly accepted, prepare an isolated, opt-in SDK/image that sets normal_nc=true only on the eight allocated table RAM frames. Keep register/SMEM/ring fixed physical mappings, all mapping rights, SMMU TCR, firmware, grants/credits, transport schedule, every table comparison and quarantine behavior unchanged. Resolve shareability using the CPU/SMMU memory contract rather than copying iPhone assumptions. Leave the baseline SDK and verified Device image intact.

Implementation clarification: the original proposal said all physical allocation would stay unchanged. Rebuilding the kernel/initializer changes the boot allocation boundary. The generated `smmu_data` and `ipa_receive_data` empty-fill RAM pools consequently move by 8 KiB; the driver receives their actual addresses through its existing injected address cells. Their VAs, rights, Device memory type, sizes and ownership remain unchanged. SMMU leaf output addresses follow those generated PAs. All eight table PAs and other fixed MMIO/SMEM/ring mappings remain unchanged. This necessary generated-layout difference is explicitly measured in the implementation evidence; the candidate is not described as an identical physical-buffer layout.

Before recommending a manual boot: audit exclusive runtime ownership and initialization aliases; validate ABI and parser/serializer/initializer round trips; reject cached+normal_nc and unsupported architectures/hypervisors; add exact final-map allowlisting and negative mutations for Normal-NC on MMIO, other RAM or another PD; preserve the full existing capability suite; rerun native corruption/fault and protocol I/O parity; inspect scalar load/comparison code and actual packaged mappings; verify the rebuilt kernel/initializer provenance, stack, loader, payload and boot layout. Hardware timing and transient/fault behavior remain experimental. Report that changed contract plainly before a user chooses to cold boot.

## Evidence

Private evidence: .cache/sunfish-native-wifi/private/offline-wrq-table-access-v1 (pinned input hashes, actual verifier disassembly, eight-frame/cap audit, ordering model and budget arithmetic). No source firmware payload is exposed.

Primary references:
- seL416.0 AArch64 mappings: https://github.com/seL4/seL4/blob/16.0.0/src/arch/arm/64/kernel/vspace.c
- Arm memory-system guide, Normal versus Device semantics: https://documentation-service.arm.com/static/62d6777531ea212bb6627683
- Linux LPAE allocation/synchronization and coherent_walk handling: https://github.com/torvalds/linux/blob/master/drivers/iommu/io-pgtable-arm.c
- Existing local iPhone evidence: docs/iphone7plus-sel4-bringup.md, explicit Normal-NC section.
