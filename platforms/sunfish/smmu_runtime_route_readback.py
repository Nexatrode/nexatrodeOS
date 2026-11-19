"""Parse read-only LineageOS Apps-SMMU route diagnostics.

This describes Linux routes after the matched driver attached its devices. It
does not describe the earlier ABL handoff or confer SMMU authority on seL4.
"""

from __future__ import annotations

from dataclasses import dataclass


class RouteReadbackError(ValueError):
    """The diagnostic route log is malformed or internally inconsistent."""


@dataclass(frozen=True)
class RouteRecord:
    sid: int
    mask: int
    device: str
    slot: int
    stage: int
    s2cr_type: int
    context_bank: int
    smr: int
    s2cr: int
    cbar: int
    sctlr: int


_FIELDS = (
    "sid", "mask", "dev", "sme", "stage", "type", "cb", "smr",
    "s2cr", "cbar", "sctlr",
)


def parse_route_readback(text: str) -> list[RouteRecord]:
    """Parse all SUNFISH_SMMU rows, retaining repeated attach transitions."""
    records: list[RouteRecord] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        marker = "SUNFISH_SMMU "
        if marker not in line:
            continue
        payload = line.split(marker, 1)[1].strip()
        values: dict[str, str] = {}
        for token in payload.split():
            if "=" not in token:
                raise RouteReadbackError(f"line {line_number}: malformed field {token!r}")
            key, value = token.split("=", 1)
            if key in values:
                raise RouteReadbackError(f"line {line_number}: duplicate field {key}")
            values[key] = value
        if set(values) != set(_FIELDS):
            raise RouteReadbackError(
                f"line {line_number}: fields are {sorted(values)}, expected {sorted(_FIELDS)}"
            )
        try:
            record = RouteRecord(
                sid=int(values["sid"], 0),
                mask=int(values["mask"], 0),
                device=values["dev"],
                slot=int(values["sme"], 0),
                stage=int(values["stage"], 0),
                s2cr_type=int(values["type"], 0),
                context_bank=int(values["cb"], 0),
                smr=int(values["smr"], 0),
                s2cr=int(values["s2cr"], 0),
                cbar=int(values["cbar"], 0),
                sctlr=int(values["sctlr"], 0),
            )
        except ValueError as error:
            raise RouteReadbackError(f"line {line_number}: invalid number") from error
        validate_record(record, line_number)
        records.append(record)
    if not records:
        raise RouteReadbackError("diagnostic log contains no SUNFISH_SMMU records")
    return records


def validate_record(record: RouteRecord, line_number: int = 0) -> None:
    """Check the reported route tuple against the raw SMR/S2CR values."""
    where = f"line {line_number}: " if line_number else ""
    if record.slot < 0 or not 0 <= record.context_bank <= 0xFF:
        raise RouteReadbackError(f"{where}slot or context-bank index is out of range")
    if record.s2cr_type not in (0, 1, 2):
        raise RouteReadbackError(f"{where}S2CR type is invalid")
    if record.smr & (1 << 31) == 0:
        raise RouteReadbackError(f"{where}SMR is not valid")
    raw_sid = record.smr & 0x7FFF
    raw_mask = (record.smr >> 16) & 0x7FFF
    if (raw_sid, raw_mask) != (record.sid, record.mask):
        raise RouteReadbackError(f"{where}reported SID/mask disagree with SMR")
    raw_type = (record.s2cr >> 16) & 0x3
    raw_cb = record.s2cr & 0xFF
    if (raw_type, raw_cb) != (record.s2cr_type, record.context_bank):
        raise RouteReadbackError(f"{where}reported route disagrees with S2CR")


def require_route(
    records: list[RouteRecord],
    *,
    sid: int,
    mask: int,
    device: str,
    slot: int,
    context_bank: int,
    cbar: int,
    sctlr: int,
) -> None:
    """Require one measured route with the supplied complete identity."""
    if not any(
        (
            item.sid,
            item.mask,
            item.device,
            item.slot,
            item.context_bank,
            item.cbar,
            item.sctlr,
        )
        == (sid, mask, device, slot, context_bank, cbar, sctlr)
        for item in records
    ):
        raise RouteReadbackError(
            f"missing runtime route sid={sid:#x} device={device} slot={slot} cb={context_bank}"
        )


def require_usb_context_reassignment(records: list[RouteRecord]) -> None:
    """Keep the observed USB CB1-to-CB23 attach/reassignment history visible."""
    usb_contexts = [
        item.context_bank
        for item in records
        if item.sid == 0x540 and item.mask == 0 and item.device == "a600000.ssusb"
    ]
    if not usb_contexts or usb_contexts[0] != 1 or usb_contexts[-1] != 23:
        raise RouteReadbackError(
            "USB route history must retain initial CB1 and final CB23 observations"
        )
