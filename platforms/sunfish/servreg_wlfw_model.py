"""Offline model separating SERVREG mappings from QRTR WLFW discovery.

This is a host-only model of source-derived records and gates. It does not
parse or authenticate the phone's manifests, start a protection domain, send
QRTR/QMI packets, touch shared memory, or grant hardware ownership.
"""

from __future__ import annotations

from dataclasses import dataclass


class DiscoveryError(RuntimeError):
    """A service-discovery transition violates the modeled contract."""


ROOT_PD = "msm/modem/root_pd"
WLAN_PD = "msm/modem/wlan_pd"

# Read-only records captured from modemr.jsn and modemuw.jsn. Their hashes
# match the device inventory in wlan-resources.json. Neither manifest advertises
# tms/pdr_enabled. The two tms/servreg mappings intentionally remain separate:
# upstream pd-mapper aggregates all matching records for GET_DOMAIN_LIST.
CAPTURED_MANIFEST_MAPPINGS = (
    ("tms", "servreg", ROOT_PD, 180),
    ("gps", "gps_service", ROOT_PD, 180),
    ("kernel", "elf_loader", WLAN_PD, 180),
    ("tms", "servreg", WLAN_PD, 180),
    ("wlan", "fw", WLAN_PD, 180),
)

WLFW_SERVICE_ID = 0x45
WLFW_SERVICE_VERSION = 1
WLFW_SERVICE_INSTANCE = 0


@dataclass(frozen=True, order=True)
class DomainRecord:
    """One SERVREG domain-list result as produced from an sr_service entry."""

    domain: str
    instance_id: int


@dataclass(frozen=True, order=True)
class QrtrServiceKey:
    """The service identity matched by a QRTR NEW_LOOKUP/NEW_SERVER pair."""

    service_id: int
    version: int
    instance: int

    def __post_init__(self) -> None:
        if isinstance(self.service_id, bool) or not 0 <= self.service_id <= 0xFFFF_FFFF:
            raise ValueError("QRTR service ID is outside u32")
        if isinstance(self.version, bool) or not 0 <= self.version <= 0xFF:
            raise ValueError("QRTR service version is outside u8")
        if isinstance(self.instance, bool) or not 0 <= self.instance <= 0xFF_FFFF:
            raise ValueError("QRTR service instance is outside u24")


@dataclass(frozen=True)
class QrtrEndpoint:
    """A registered QRTR server endpoint, with its modeled source domain."""

    node: int
    port: int
    source_domain: str


@dataclass(frozen=True)
class NewServerNotice:
    """A NEW_SERVER notification delivered to a matching registered lookup."""

    key: QrtrServiceKey
    endpoint: QrtrEndpoint


WLFW_KEY = QrtrServiceKey(
    WLFW_SERVICE_ID, WLFW_SERVICE_VERSION, WLFW_SERVICE_INSTANCE
)


class PdMapperModel:
    """Model pd-mapper's static service-name to domain/instance table."""

    def __init__(
        self,
        mappings: tuple[tuple[str, str, str, int], ...] = CAPTURED_MANIFEST_MAPPINGS,
    ) -> None:
        self._mappings = tuple(mappings)
        self._published = False
        self._validate_mappings()

    @property
    def published(self) -> bool:
        return self._published

    def publish(self, *, qrtr_name_service_ready: bool) -> None:
        """Make SERVREG queries available after QRTR is ready and maps exist."""
        if self._published:
            raise DiscoveryError("pd-mapper is already published")
        if qrtr_name_service_ready is not True:
            raise DiscoveryError("QRTR name service is not ready")
        if not self._mappings:
            raise DiscoveryError("no protection-domain manifests are loaded")
        self._published = True

    def get_domain_list(self, service_name: str) -> tuple[DomainRecord, ...]:
        """Return all records whose provider/service equals service_name."""
        if not self._published:
            raise DiscoveryError("SERVREG lookup is unavailable before publication")
        if (
            not isinstance(service_name, str)
            or not service_name
            or "\x00" in service_name
        ):
            raise DiscoveryError("invalid SERVREG service name")
        return tuple(
            DomainRecord(domain, instance)
            for provider, service, domain, instance in self._mappings
            if f"{provider}/{service}" == service_name
        )

    def _validate_mappings(self) -> None:
        seen: set[tuple[str, str, str, int]] = set()
        for mapping in self._mappings:
            if not isinstance(mapping, tuple) or len(mapping) != 4:
                raise DiscoveryError("manifest mapping must have four fields")
            provider, service, domain, instance = mapping
            if not all(
                isinstance(value, str) and value
                for value in (provider, service, domain)
            ):
                raise DiscoveryError("manifest provider, service, and domain must be nonempty")
            if "/" in provider or "/" in service or "\x00" in domain:
                raise DiscoveryError("manifest mapping contains an invalid name")
            if (
                isinstance(instance, bool)
                or not isinstance(instance, int)
                or not 0 <= instance <= 0xFFFF_FFFF
            ):
                raise DiscoveryError("manifest QMI instance is outside u32")
            if mapping in seen:
                raise DiscoveryError("duplicate manifest mapping")
            seen.add(mapping)


class QrtrNameServiceModel:
    """Model lookup and server registration as independent QRTR events."""

    def __init__(self) -> None:
        self.ready = False
        self._lookups: set[QrtrServiceKey] = set()
        self._servers: dict[QrtrServiceKey, QrtrEndpoint] = {}

    def start(self) -> None:
        if self.ready:
            raise DiscoveryError("QRTR name service is already ready")
        self.ready = True

    def register_lookup(self, key: QrtrServiceKey) -> NewServerNotice | None:
        """Register a client lookup; this never creates a remote server."""
        self._require_ready()
        if key in self._lookups:
            raise DiscoveryError("QRTR lookup is already registered")
        self._lookups.add(key)
        endpoint = self._servers.get(key)
        if endpoint is None:
            return None
        return NewServerNotice(key, endpoint)

    def advertise_server(
        self,
        key: QrtrServiceKey,
        *,
        node: int,
        port: int,
        source_domain: str,
    ) -> tuple[NewServerNotice, ...]:
        """Register an endpoint and notify only clients looking up that key."""
        self._require_ready()
        if (
            isinstance(node, bool)
            or not isinstance(node, int)
            or isinstance(port, bool)
            or not isinstance(port, int)
            or not 0 <= node <= 0xFFFF_FFFF
            or not 1 <= port < 0xFFFF_FFFE
        ):
            raise DiscoveryError("invalid QRTR endpoint")
        if not isinstance(source_domain, str) or not source_domain:
            raise DiscoveryError("QRTR server source domain is unknown")
        if key in self._servers:
            raise DiscoveryError("QRTR server is already registered")
        if key == WLFW_KEY and source_domain != WLAN_PD:
            raise DiscoveryError("WLFW must be advertised by the modem WLAN domain")
        endpoint = QrtrEndpoint(node, port, source_domain)
        self._servers[key] = endpoint
        if key in self._lookups:
            return (NewServerNotice(key, endpoint),)
        return ()

    def server_for_lookup(self, key: QrtrServiceKey) -> QrtrEndpoint | None:
        """Return an endpoint only after this client registered its lookup."""
        self._require_ready()
        if key not in self._lookups:
            raise DiscoveryError("client has not registered this QRTR lookup")
        return self._servers.get(key)

    def _require_ready(self) -> None:
        if not self.ready:
            raise DiscoveryError("QRTR name service is not ready")
