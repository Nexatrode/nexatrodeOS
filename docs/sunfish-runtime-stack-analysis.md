# Sunfish MPSS runtime stack analysis

The one-shot constructor diagnostic stopped at `TABLE SETUP / SETUP STEP
0000000A` in IMG_9220. That checkpoint was flushed immediately before the
receive-mapping constructor. The next checkpoint, `0xC`, was compiled after
the constructor's stack probes and did not appear. The screen also reported
zero PAS attempts and no authentication receipt.

For the ELF from `native-mpss-runtime-constructor-checkpoints-20261029-v1`,
the AArch64 prologues and direct call sites give these simultaneously live
frames on that path:

| Frame | Bytes |
| --- | ---: |
| Microkit `run_main` closure | 371,392 |
| Startup `init` | 386,352 |
| `start_runtime_after_gate` | 247,344 |
| `service` | 480 |
| `retained_quiet_with` | 112 |
| `retained_receive_domain_with` | 224 |
| `verify_receive_tables_with` | 25,856 |
| `receive_mapping_inner_with` | 13,456 |
| Mapping constructor | 26,160 |
| **Total** | **1,071,376** |

The auth entry tail-calls `start_runtime_after_gate`, so its own frame is
not included. The sum is a lower bound: it excludes runtime-entry overhead
and does not constitute a device fault record. The TCB initial SP is
`0x7fffffffe000`, and the original 256 stack frames span the 1 MiB below it.
Even without outer overhead, the constructor's first 4 KiB stack probe
crosses that range. This is consistent with the observed last checkpoint.

The runtime system now declares a 2 MiB stack. Its runtime capability audit
checks the size, exact 4 KiB frame identities and count, TCB page-table root,
reachable contiguous virtual range below SP, and cached RW/non-executable
frame mappings. A fresh build and a one-shot device test are required to
establish whether execution advances; neither Wi-Fi nor a complete stack
high-water mark is established by this analysis.

In the 2 MiB rebuild's generated CapDL, the initial SP and reachable page
tables cover 512 contiguous cached RW/non-executable 4 KiB stack frames below
SP. An offline inspection of its ELF shows the same major `run_main`,
`init`, post-gate, and constructor frame sizes. The full capability and
boot-layout audits and the device run remain separate checks.
