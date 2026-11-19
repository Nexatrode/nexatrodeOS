"""Offline fail-closed policy model for Sunfish modem lifecycle authority.

This is a host-testable transition model only. It has no SCM/SMC, AOP, MMIO,
firmware I/O, allocator, shared-memory, or GLINK access. Production callers
must obtain each evidence item, including AP-side startup-service readiness,
from the named trusted service, not from a client-provided boolean.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum, auto


class ModemLifecycleError(RuntimeError):
    """An operation violates the modem lifecycle authority policy."""


class ModemState(Enum):
    OFF = auto()
    PREPARED = auto()
    BOOTING = auto()
    AUTHENTICATED = auto()
    PEER_READY = auto()
    WLAN_READY = auto()
    STOPPING = auto()
    QUARANTINED = auto()


@dataclass(frozen=True)
class PeerGeneration:
    """Opaque-in-production proof that a modem peer belongs to one boot."""

    value: int


class ModemLifecycleModel:
    """Model the narrow authority boundary around modem and WLAN startup."""

    def __init__(self) -> None:
        self.state = ModemState.OFF
        self.generation = 0
        self._peer: PeerGeneration | None = None

    @property
    def peer_generation(self) -> PeerGeneration | None:
        """Return the current generation only while its peer remains valid."""
        if self.state in (ModemState.PEER_READY, ModemState.WLAN_READY):
            return self._peer
        return None

    @property
    def modem_reservation_excluded(self) -> bool:
        """The full modem image pool is never returned to general allocation."""
        return True

    def prepare_boot(
        self,
        *,
        firmware_bundle_identified: bool,
        image_layout_valid: bool,
        memory_excluded: bool,
        scm_operation_allowlisted: bool,
        ipa_host_ready: bool,
        servreg_mapper_ready: bool,
        remote_storage_ready: bool,
        heap_memory_ready: bool,
        tftp_provider_ready: bool,
    ) -> None:
        self._require(ModemState.OFF)
        missing_services = [
            name
            for name, ready in (
                ("IPA host provider", ipa_host_ready),
                ("SERVREG mapper", servreg_mapper_ready),
                ("remote storage", remote_storage_ready),
                ("heap-memory sharing", heap_memory_ready),
                ("TFTP provider", tftp_provider_ready),
            )
            if ready is not True
        ]
        if missing_services:
            raise ModemLifecycleError(
                "host startup services are not ready: " + ", ".join(missing_services)
            )
        if not all((firmware_bundle_identified, image_layout_valid, memory_excluded,
                    scm_operation_allowlisted)):
            raise ModemLifecycleError("firmware, image, memory, or secure-operation policy is unverified")
        self.state = ModemState.PREPARED

    def power_sequence_complete(self, *, aop_sequence_verified: bool) -> None:
        self._require(ModemState.PREPARED)
        if aop_sequence_verified is not True:
            self._quarantine()
            raise ModemLifecycleError("named AOP power sequence is unverified")
        self.state = ModemState.BOOTING

    def secure_boot_complete(self, *, pas_authentication_succeeded: bool) -> None:
        self._require(ModemState.BOOTING)
        if pas_authentication_succeeded is not True:
            self._quarantine()
            raise ModemLifecycleError("secure processor authentication failed")
        self.state = ModemState.AUTHENTICATED

    def modem_ready(
        self,
        *,
        after_powerup_verified: bool,
        fresh_reset_verified: bool,
        exclusive_transport_verified: bool,
    ) -> PeerGeneration:
        self._require(ModemState.AUTHENTICATED)
        if not all((after_powerup_verified, fresh_reset_verified,
                    exclusive_transport_verified)):
            self._quarantine()
            raise ModemLifecycleError("fresh, exclusively owned modem peer is unverified")
        self.generation += 1
        self._peer = PeerGeneration(self.generation)
        self.state = ModemState.PEER_READY
        return self._peer

    def enable_wlan(
        self,
        peer: PeerGeneration,
        *,
        msa_owner_verified: bool,
        smmu_isolated: bool,
    ) -> None:
        self._require(ModemState.PEER_READY)
        if peer != self._peer:
            raise ModemLifecycleError("stale or foreign modem peer generation")
        if not (msa_owner_verified and smmu_isolated):
            raise ModemLifecycleError("MSA ownership or SMMU isolation is unverified")
        self.state = ModemState.WLAN_READY

    def request_stop(self) -> None:
        if self.state not in (ModemState.PEER_READY, ModemState.WLAN_READY):
            raise ModemLifecycleError("modem stop requested outside an active peer")
        self._invalidate_peer()
        self.state = ModemState.STOPPING

    def shutdown_complete(
        self,
        *,
        stop_ack_verified: bool,
        pas_shutdown_succeeded: bool,
        power_down_verified: bool,
        transport_dma_contained: bool,
    ) -> None:
        self._require(ModemState.STOPPING)
        if not all((stop_ack_verified, pas_shutdown_succeeded,
                    power_down_verified, transport_dma_contained)):
            self._quarantine()
            raise ModemLifecycleError(
                "stop acknowledgement, PAS shutdown, power-down, or DMA containment is unverified"
            )
        self.state = ModemState.OFF

    def fault(self) -> None:
        """Watchdog, crash, ambiguous reset, or transport corruption."""
        if self.state is ModemState.OFF:
            raise ModemLifecycleError("fault reported while modem is already off")
        self._quarantine()

    def confirm_fault_stopped(
        self,
        *,
        reset_verified: bool,
        pas_shutdown_succeeded: bool,
        power_down_verified: bool,
        transport_dma_contained: bool,
    ) -> None:
        self._require(ModemState.QUARANTINED)
        if not all((reset_verified, pas_shutdown_succeeded, power_down_verified,
                    transport_dma_contained)):
            raise ModemLifecycleError("faulted modem remains quarantined")
        self.state = ModemState.OFF

    def _invalidate_peer(self) -> None:
        self._peer = None
        self.generation += 1

    def _quarantine(self) -> None:
        self._invalidate_peer()
        self.state = ModemState.QUARANTINED

    def _require(self, expected: ModemState) -> None:
        if self.state is not expected:
            raise ModemLifecycleError(
                f"operation requires {expected.name}, current state is {self.state.name}"
            )
