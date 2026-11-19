# Pixel service and application authority

The Pixel prototype uses seL4 and Microkit. We do not need to write a new
process scheduler or a general `fork`/`exec` model for its fixed app set. Each
Microkit protection domain has its own seL4 thread, address space (VSpace),
capability space (CSpace), and scheduling context. The system description
specifies its image, mapped pages, channels, priority, and CPU budget. seL4
stores and enforces the underlying thread, mapping, capability, and scheduling
objects. The current design loads a fixed set of domains at boot; demand
paging, arbitrary app installation, and dynamic process creation are not
implemented.

The intended boundary is a service call, not a pointer into another app's
memory. A client receives an endpoint capability for each service it may call.
The service uses the receiving channel identity assigned by Microkit to select
the caller's authority. A request may specify an operation and data, but cannot
choose its own caller identity or storage namespace. The service validates the
label, exact message length, and data before changing state. A passive service
uses the caller's scheduling context, so its work consumes the caller's CPU
budget. Drivers alone should receive device MMIO and IRQ capabilities; service
clients receive narrow calls rather than hardware mappings.

## Current state and target boundaries

| Domain or view | Pixel state now | Target authority |
| --- | --- | --- |
| Slint shell and eight app views | Slint renders all views and handles seven workspaces locally; Notes edits now call a separate domain, and Slint maps its bounded view read-only | Split the remaining private apps and narrow the shell's input authority |
| Touch service | Separate domain with touch-related MMIO and writable event queue | Keep hardware access here; publish validated input events to a trusted router |
| Notes, Contacts, Calendar, Messages | Notes has private editable state and a vault call endpoint; the other three remain interactive in-memory views in Slint | Separate the remaining app identities and give each a fixed vault namespace endpoint |
| Calculator | Interactive in-memory view in the Slint domain | No storage or device capability unless a feature requires one |
| Clock and Settings | Interactive in-memory views in the Slint domain | Clock reads time from a time service; Settings requests specific changes through privileged brokers |
| Vault and storage driver | Notes has a private **volatile** vault domain on Pixel; QEMU/UTM has disk-backed namespaces, but no Pixel block service is integrated | Vault owns durable encrypted records and calls a block service; apps cannot map storage or each other's records |

The QEMU/UTM vault already binds Notes, Contacts, Calendar, and Messages
endpoints to distinct fixed namespaces. Its protocol is defined in
[`rust/src/vault_protocol.rs`](../rust/src/vault_protocol.rs), and its present
limits are described in [`security-model.md`](security-model.md). The Pixel
policy probe uses the same read, write, OK, bad-request, and denied labels;
it holds only one volatile word and is not the encrypted vault.

## Pixel evidence: bounded policy probe

[`policy-probe.system`](../platforms/sunfish/policy-probe.system) grants two
different protected-call capabilities to the passive `policy_vault`. The
writer's vault-side channel 0 permits read and write. The reader's vault-side
channel 1 permits read; a write returns `POLICY_DENIED` without changing the
word. The intruder has no vault endpoint capability in its CSpace. Only the
vault and witness map the test-state page. A lower-priority witness checks the
final state and requests PSCI reset. The builder refuses to package an SDK
whose generated kernel timer IRQ remains unresolved.

On the Pixel 4a temporary boot of 2026-10-07, UART reported `WRITER PASS`,
`READER PASS`, and `INTRUDER DENIED NO CAP`. The witness reported the original
`NOTE` test word (decimal `1313821765`), one accepted write, three accepted
reads, and one denied write. The probe reset itself; ADB confirmed LineageOS
23.2 boot complete on slot `a`. No partition was flashed. This demonstrates
operation-level authorization for two named channels and kernel rejection of
one missing capability. It does not demonstrate persistent storage, secure
user authentication, or app-level isolation in the present Pixel Slint shell.

## Pixel Notes split

[`slint-display.system`](../platforms/sunfish/slint-display.system) now maps a
single 4 KiB `notes_view` page read-only into Slint and read-write into
`notes_service`. The view contains a revision, length, status, and at most 128
bytes of text; the service publishes it with an odd/even revision, and the
shell checks the revision and UTF-8 before displaying it. Notes holds the
editable text in its private memory. Slint sends one validated keyboard code
or SAVE request over its Notes protected-call endpoint. Only Notes has the
vault protected-call endpoint. The vault validates exact request length,
bounded text length, and allowed bytes before copying the record into its own
private memory. Notes checks the vault's length and content hash reply before
showing `SAVED IN VOLATILE VAULT`. No Pixel storage or encryption is involved;
the record disappears on reset. Slint still sees the projected cleartext while
rendering, so this split is an app/service boundary, not confidentiality from
the display domain.

The Microkit report confirms the Slint CSpace contains an endpoint to Notes
and no endpoint to the vault. The image built and booted temporarily on the
Pixel on 2026-10-07; UART showed both services ready and the Slint first frame.
An optional boot-time smoke image sent `A` to Notes, checked the view, called
SAVE, observed `VOLATILE VAULT WRITE` and `NOTES VAULT ACCEPTED`, then deleted
the test character; it printed `NOTES IPC SMOKE PASS`. This verifies the
protected-call chain and view on hardware. A manual Notes keyboard/SAVE test
of the split image has not yet been observed.

## Next integration boundary

Repeat the negative checks from the QEMU system on Pixel: Notes cannot name or
read Contacts data, malformed request lengths change no record, and a fault in
Notes leaves the vault and display alive. Then add a Pixel block service and
key handling before calling Notes private or durable.
