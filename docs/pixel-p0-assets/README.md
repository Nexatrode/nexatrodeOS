# Pixel P0 local assets

Host-side materials for the [P0 boot/debug checklist](../pixel-p0-boot-feasibility.md).
Large binaries are gitignored; keep checksums and notes in the repo.

## Layout

| Path | Purpose |
| --- | --- |
| `sm7150-google-sunfish.dts`, `sm7150.dtsi` | Mainline-style DT from `sm7150-mainline/linux` (v6.19 snapshot) |
| `lineage-ref/YYYYMMDD/` | Matching Lineage `boot.img`, `dtbo.img`, `vbmeta.img`, signed ZIP, SHA256 |
| `lineage-ref/YYYYMMDD/unpacked/` | Extracted kernel, ramdisk, DTB from `boot.img` |

Related desk scaffold (not flashed): [`platforms/sunfish/`](../../platforms/sunfish/README.md)
holds a minimal overlay DTS and GENI UART putchar stub derived from these DT
sources.

Do not commit phone serial numbers or userdata dumps here.
