# Sunfish WMI and Copy Engine integration notes

These notes preserve a comparison of the ath10k path and the existing
QCACLD-derived Copy Engine (CE) path. They describe integration constraints,
not a working Pixel Wi-Fi connection. See the
[ath10k port plan](sunfish-ath10k-port-plan.md) for the broader bring-up.

## Queue and publication ownership

The [ath10k WMI transmit queue](../rust/src/ath10k_wmi_tx.rs) reserves a
software ring slot and HTC credits and returns a `QueuedTransfer`. The
[QCACLD-derived WMI planner](../rust/src/qcom_wmi_ce.rs) produces a separate
`WmiCeTxPlan`, which the [native CE publisher](../rust/src/qcom_ce_v3_native_io.rs)
accepts with its own profile, DMA and ring state. These APIs alone do not
establish that their reservations refer to the same live slot, frame and
transport generation.

Before joining those paths, an integration needs to establish that
correspondence, account for credits and completion, distinguish definite from
uncertain publication, and retain frame and key buffers until they can be
safely retired. Existing host tests and mock compositions are not proof of
live firmware transport.

## Route and host ring comparison

The pinned [ath10k SNOC reference](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/snoc.c)
maps WMI_CONTROL host-to-target to CE3 and target-to-host to CE2, matching
the route checked by `validate_wmi_route`. Its **host** ring table differs
from the two local [QCACLD host profiles](../rust/src/qcom_ce_v3_profile.rs):

| Engine | ath10k SNOC host | QCACLD Mission | QCACLD MissionNoPktlog |
| --- | --- | --- | --- |
| CE1 receive | 512 destination entries | 512 destination entries | 256 destination entries |
| CE2 WMI receive | 64 destination entries | 128 destination entries | 64 destination entries |
| CE3 WMI transmit | 32 source entries, 2048-byte maximum | 32 source entries, 2048-byte maximum | 32 source entries, 2048-byte maximum |
| CE5 | 512 destination entries | 2048 source entries | 2048 source entries |

These are host ring configurations, not target pipe configurations. Matching
the WMI route or a few ring values does not establish profile compatibility,
credit ownership or safe runtime publication; the selected firmware path must
be validated as a whole.

## Descriptor encoding

The [ath10k WCN3990 descriptor](../rust/src/ath10k_ce.rs) places a 12-bit
transfer ID at bit 4 in its 16-bit flags field, consistent with the pinned
[WCN3990 metadata mask and shift](https://github.com/sm7150-mainline/linux/blob/fd78d17954a86ea6ceb0c535e618ea129e09f8dd/drivers/net/wireless/ath/ath10k/hw.c).
The [QCACLD-derived descriptor](../rust/src/qcom_ce_v3.rs) places the 16-bit
ID directly in the corresponding field. With ID 1 and gather and byte-swap
disabled, those fields are `0x0010` and `0x0001`, respectively. Gather and
byte-swap control encoding also differs. An adapter must validate the actual
firmware's descriptor semantics; it cannot copy one encoder's output into the
other publisher merely because the WMI route matches.
