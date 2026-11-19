//! Source-derived Sunfish IPA v4.2 interconnect vote profile.
//!
//! The pinned Atoll DTS declares `qcom,bandwidth-vote-for-ipa`; Linux submits
//! one of these four-path `msm_bus` usecases before enabling IPA clocks. This
//! module can compose the exact IPA contribution with a caller-provided
//! complete MSM-bus snapshot into a pure BCM/TCS plan. It does not submit that
//! plan, enable a clock, or establish that the IPA aperture is powered.
#![forbid(unsafe_code)]

use crate::qcom_bcm_vote::{
    Bandwidth, BcmRequest, Contexts, Error as BcmVoteError, RpmhRequestState, TcsPlan, plan_tcs,
};
use crate::qcom_msm_bus_aggregate::{
    self as msm_bus, BcmBinding, ClientReport, Error as MsmBusError, Route, RouteNodeVote,
    Snapshot as BusSnapshot,
};
use crate::qcom_rpmh_handoff::Snapshot as RscSnapshot;
use crate::qcom_rpmh_vrm_tcs::{self as rsc_tcs, Plan as RscTcsPlan};

pub const SUNFISH_IPA_HW_V4_2: u32 = 16;
pub const USE_CASE_COUNT: usize = 5;
pub const PATH_COUNT: usize = 4;
pub const MAX_PATH_NODE_VOTES: usize = 20;
/// atoll.dtsi omits qcom,msm-bus,active-only for IPA. The pinned parser
/// defaults such clients to dual-context requests (active plus sleep).
pub const DUAL_CONTEXT: bool = true;

pub const RATE_SVS2_HZ: u32 = 50_000_000;
pub const RATE_SVS_HZ: u32 = 100_000_000;
pub const RATE_NOMINAL_HZ: u32 = 201_000_000;
pub const RATE_TURBO_HZ: u32 = 240_000_000;

pub const MASTER_AMPSS_M0: u16 = 1;
pub const MASTER_IPA: u16 = 90;
pub const MASTER_IPA_CORE: u16 = 143;
pub const SLAVE_EBI_CH0: u16 = 512;
pub const SLAVE_OCIMEM: u16 = 585;
pub const SLAVE_IPA_CFG: u16 = 676;
pub const SLAVE_IPA_CORE: u16 = 777;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    WrongHardwareType,
    WrongUseCaseCount { expected: usize, actual: usize },
    UnsupportedClockRate(u32),
    PathEndpoint,
    DuplicateRoute,
    EmptyRoute,
    DuplicateRouteNode,
    VoteCapacity,
    InvalidIdentity,
    InvalidGeneration,
    CompleteBusSet,
    IpaClientMissing,
    IpaClientAmbiguous,
    BusAggregate(MsmBusError),
    BcmAggregate(MsmBusError),
    RpmhPlan(BcmVoteError),
    TcsBatchIndex,
    TcsBatchLayout,
    RscPlan(rsc_tcs::Error),
}

/// Pure end-to-end initial/usecase vote result. `bcm_requests` and `tcs` are
/// diagnostic plans; submitting them still requires a trusted platform owner,
/// complete runtime client state, live RSC lease, and source-matched executor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedVote<const BCMS: usize> {
    pub use_case_index: u8,
    pub use_case_name: &'static str,
    pub bcm_requests: [BcmRequest; BCMS],
    pub tcs: TcsPlan<BCMS>,
}

/// Replace only the IPA client's contribution inside a complete, current bus
/// snapshot, aggregate every configured client, scale the ordered BCM links,
/// and plan active/wake/sleep TCS commands. The caller must supply the entire
/// owning-RSC commit list and authentic per-client starting reports. This is
/// pure planning: it acquires no lease and performs no controller I/O.
pub fn plan_with_complete_bus_snapshot<
    const CLIENTS: usize,
    const NODE_COUNT: usize,
    const BCMS: usize,
    const MAX_LINKS: usize,
>(
    snapshot: BusSnapshot<'_>,
    ipa_client_id: u16,
    use_case: &'static UseCase,
    path_routes: [PathRoute<'_>; PATH_COUNT],
    node_ids: [u16; NODE_COUNT],
    bcm_bindings: [BcmBinding<'_>; BCMS],
    init_time: bool,
    request_state: RpmhRequestState,
) -> Result<PreparedVote<BCMS>, Error> {
    if CLIENTS == 0
        || snapshot.expected_clients.len() != CLIENTS
        || snapshot.clients.len() != CLIENTS
    {
        return Err(Error::CompleteBusSet);
    }

    let mut ipa_index = None;
    for (index, client) in snapshot.clients.iter().enumerate() {
        if client.client_id == ipa_client_id {
            if ipa_index.replace(index).is_some() {
                return Err(Error::IpaClientAmbiguous);
            }
        }
    }
    let ipa_index = ipa_index.ok_or(Error::IpaClientMissing)?;

    let contribution = client_contribution::<MAX_PATH_NODE_VOTES>(use_case, path_routes)?;
    let ipa_report = contribution.report(ipa_client_id, snapshot.identity, snapshot.generation)?;
    let mut clients = [snapshot.clients[0]; CLIENTS];
    clients.copy_from_slice(snapshot.clients);
    clients[ipa_index] = ipa_report;
    let complete_snapshot = BusSnapshot {
        identity: snapshot.identity,
        generation: snapshot.generation,
        expected_clients: snapshot.expected_clients,
        routes: snapshot.routes,
        clients: &clients,
    };

    let node_totals =
        msm_bus::aggregate(complete_snapshot, node_ids).map_err(Error::BusAggregate)?;
    let bcm_vectors = msm_bus::aggregate_bcm_links::<BCMS, MAX_LINKS>(&node_totals, bcm_bindings)
        .map_err(Error::BcmAggregate)?;

    let bcm_requests = core::array::from_fn(|index| BcmRequest {
        metadata: bcm_bindings[index].metadata,
        vectors: bcm_vectors[index],
    });
    let tcs = plan_tcs(bcm_requests, init_time, request_state).map_err(Error::RpmhPlan)?;
    Ok(PreparedVote {
        use_case_index: use_case.index,
        use_case_name: use_case.name,
        bcm_requests,
        tcs,
    })
}

/// Convert one complete active-phase batch into the existing guarded Sunfish
/// Apps-RSC TCS0 register plan. The snapshot and owner identity/generation are
/// supplied by the caller; this helper does not claim that they are live,
/// exclusive, or safe to execute. Deferred wake/sleep batches remain owner
/// cache work and are not submitted through active TCS0.
pub fn plan_active_rsc_tcs_batch<const BCMS: usize>(
    prepared: &PreparedVote<BCMS>,
    snapshot: RscSnapshot,
    identity: u64,
    generation: u64,
    ticket: u64,
    batch_index: usize,
) -> Result<RscTcsPlan, Error> {
    let phase = prepared.tcs.active;
    if phase.len > BCMS || phase.batches.count > BCMS || batch_index >= phase.batches.count {
        return Err(Error::TcsBatchIndex);
    }

    let mut offset = 0usize;
    let mut selected = None;
    for index in 0..phase.batches.count {
        let length = phase.batches.lengths[index];
        if length == 0 || length > rsc_tcs::MAX_TCS_COMMANDS {
            return Err(Error::TcsBatchLayout);
        }
        let end = offset.checked_add(length).ok_or(Error::TcsBatchLayout)?;
        if end > phase.len {
            return Err(Error::TcsBatchLayout);
        }
        if index == batch_index {
            selected = Some((offset, end));
        }
        offset = end;
    }
    if offset != phase.len {
        return Err(Error::TcsBatchLayout);
    }
    let (start, end) = selected.ok_or(Error::TcsBatchIndex)?;
    rsc_tcs::build_bcm_command_plan(
        snapshot,
        identity,
        generation,
        ticket,
        &phase.commands[start..end],
    )
    .map_err(Error::RscPlan)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathVote {
    pub master: u16,
    pub slave: u16,
    pub average_kbps: u32,
    pub instantaneous_kbps: u32,
}

impl PathVote {
    /// Convert the DTS KB/s vector to the byte/s dual-context request consumed
    /// by msm_bus_scale_client_update_request. No RPMh word is produced.
    pub fn contexts(self) -> Contexts {
        Contexts {
            active_only: Bandwidth::default(),
            dual: Bandwidth {
                average: u64::from(self.average_kbps) * 1_000,
                peak: u64::from(self.instantaneous_kbps) * 1_000,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UseCase {
    pub index: u8,
    pub name: &'static str,
    pub paths: [PathVote; PATH_COUNT],
}

/// A source-selected MSM-bus route for one IPA path. The route must come from
/// the validated Sunfish DT topology; endpoint matching prevents accidentally
/// applying one path's bandwidth to another path's node list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathRoute<'a> {
    pub master: u16,
    pub slave: u16,
    pub route: Route<'a>,
}

/// Node IDs on the four ordered routes selected by the pinned Sunfish msm_bus
/// arbiter for the IPA v4.2 vectors. `validate-bcm-routes.py` checks these
/// sequences against the live-derived Sunfish DT and pinned route algorithm.
pub const SUNFISH_IPA_EBI_ROUTE_NODES: [u16; 8] = [90, 10_065, 10_064, 10_073, 135, 770, 129, 512];
pub const SUNFISH_IPA_OCIMEM_ROUTE_NODES: [u16; 4] = [90, 10_065, 10_064, 585];
pub const SUNFISH_IPA_CFG_ROUTE_NODES: [u16; 6] = [1, 10_071, 161, 10_036, 10_035, 676];
pub const SUNFISH_IPA_CORE_ROUTE_NODES: [u16; 2] = [143, 777];

/// Construct the source-selected IPA path set using caller-assigned unique
/// route IDs from the complete platform bus graph.
pub const fn sunfish_path_routes(route_ids: [u16; PATH_COUNT]) -> [PathRoute<'static>; PATH_COUNT] {
    [
        PathRoute {
            master: MASTER_IPA,
            slave: SLAVE_EBI_CH0,
            route: Route {
                id: route_ids[0],
                nodes: &SUNFISH_IPA_EBI_ROUTE_NODES,
            },
        },
        PathRoute {
            master: MASTER_IPA,
            slave: SLAVE_OCIMEM,
            route: Route {
                id: route_ids[1],
                nodes: &SUNFISH_IPA_OCIMEM_ROUTE_NODES,
            },
        },
        PathRoute {
            master: MASTER_AMPSS_M0,
            slave: SLAVE_IPA_CFG,
            route: Route {
                id: route_ids[2],
                nodes: &SUNFISH_IPA_CFG_ROUTE_NODES,
            },
        },
        PathRoute {
            master: MASTER_IPA_CORE,
            slave: SLAVE_IPA_CORE,
            route: Route {
                id: route_ids[3],
                nodes: &SUNFISH_IPA_CORE_ROUTE_NODES,
            },
        },
    ]
}

/// Return the node-route table for `sunfish_path_routes()` using the same IDs.
pub const fn sunfish_routes(route_ids: [u16; PATH_COUNT]) -> [Route<'static>; PATH_COUNT] {
    let paths = sunfish_path_routes(route_ids);
    [
        paths[0].route,
        paths[1].route,
        paths[2].route,
        paths[3].route,
    ]
}

/// Fixed-capacity route-node contribution for one IPA clock usecase. This
/// contains only the IPA client's contribution. The final MSM-bus snapshot
/// must still include every expected client before shared BCM totals are
/// complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientContribution<const MAX_VOTES: usize> {
    route_ids: [u16; PATH_COUNT],
    votes: [RouteNodeVote; MAX_VOTES],
    vote_count: usize,
}

impl<const MAX_VOTES: usize> ClientContribution<MAX_VOTES> {
    /// Borrow this contribution in the form consumed by the complete
    /// source-validated MSM-bus client aggregator.
    pub fn report(
        &self,
        client_id: u16,
        identity: u64,
        generation: u64,
    ) -> Result<ClientReport<'_>, Error> {
        if identity == 0 {
            return Err(Error::InvalidIdentity);
        }
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        Ok(ClientReport {
            client_id,
            identity,
            generation,
            route_ids: &self.route_ids,
            votes: &self.votes[..self.vote_count],
        })
    }
}

impl UseCase {
    /// Per-path values after the Linux KBTOB conversion and its dual-context
    /// default. Route-node/client aggregation still has to be performed.
    pub fn contexts(&self) -> [Contexts; PATH_COUNT] {
        self.paths.map(PathVote::contexts)
    }
}

/// Convert all four selected IPA path vectors into complete route-node votes.
/// `path_routes` must be in the Sunfish `ipa3_enable_clks()` usecase order and
/// each route must be the source-selected path from the master to the slave.
/// Repeated node IDs across different routes are retained because Linux
/// aggregates each route's link contribution separately.
pub fn client_contribution<const MAX_VOTES: usize>(
    use_case: &UseCase,
    path_routes: [PathRoute<'_>; PATH_COUNT],
) -> Result<ClientContribution<MAX_VOTES>, Error> {
    let mut route_ids = [0; PATH_COUNT];
    let mut votes = [empty_route_node_vote(); MAX_VOTES];
    let mut vote_count = 0usize;

    for (path_index, (path, binding)) in use_case.paths.iter().zip(path_routes).enumerate() {
        if path.master != binding.master || path.slave != binding.slave {
            return Err(Error::PathEndpoint);
        }
        if route_ids[..path_index].contains(&binding.route.id) {
            return Err(Error::DuplicateRoute);
        }
        if binding.route.nodes.is_empty() {
            return Err(Error::EmptyRoute);
        }
        for (node_index, node_id) in binding.route.nodes.iter().copied().enumerate() {
            if binding.route.nodes[..node_index].contains(&node_id) {
                return Err(Error::DuplicateRouteNode);
            }
            if vote_count == MAX_VOTES {
                return Err(Error::VoteCapacity);
            }
            votes[vote_count] = RouteNodeVote {
                route_id: binding.route.id,
                node_id,
                contexts: path.contexts(),
            };
            vote_count += 1;
        }
        route_ids[path_index] = binding.route.id;
    }

    Ok(ClientContribution {
        route_ids,
        votes,
        vote_count,
    })
}

const fn empty_route_node_vote() -> RouteNodeVote {
    RouteNodeVote {
        route_id: 0,
        node_id: 0,
        contexts: Contexts {
            active_only: Bandwidth {
                average: 0,
                peak: 0,
            },
            dual: Bandwidth {
                average: 0,
                peak: 0,
            },
        },
    }
}

const fn path(master: u16, slave: u16, average_kbps: u32, instantaneous_kbps: u32) -> PathVote {
    PathVote {
        master,
        slave,
        average_kbps,
        instantaneous_kbps,
    }
}

const USE_CASES: [UseCase; USE_CASE_COUNT] = [
    UseCase {
        index: 0,
        name: "MIN",
        paths: [
            path(MASTER_IPA, SLAVE_EBI_CH0, 0, 0),
            path(MASTER_IPA, SLAVE_OCIMEM, 0, 0),
            path(MASTER_AMPSS_M0, SLAVE_IPA_CFG, 0, 0),
            path(MASTER_IPA_CORE, SLAVE_IPA_CORE, 0, 0),
        ],
    },
    UseCase {
        index: 1,
        name: "SVS2",
        paths: [
            path(MASTER_IPA, SLAVE_EBI_CH0, 80_000, 465_000),
            path(MASTER_IPA, SLAVE_OCIMEM, 80_000, 68_570),
            path(MASTER_AMPSS_M0, SLAVE_IPA_CFG, 80_000, 30),
            path(MASTER_IPA_CORE, SLAVE_IPA_CORE, 0, 30),
        ],
    },
    UseCase {
        index: 2,
        name: "SVS",
        paths: [
            path(MASTER_IPA, SLAVE_EBI_CH0, 80_000, 2_000_000),
            path(MASTER_IPA, SLAVE_OCIMEM, 80_000, 267_461),
            path(MASTER_AMPSS_M0, SLAVE_IPA_CFG, 80_000, 109_890),
            path(MASTER_IPA_CORE, SLAVE_IPA_CORE, 0, 109),
        ],
    },
    UseCase {
        index: 3,
        name: "NOMINAL",
        paths: [
            path(MASTER_IPA, SLAVE_EBI_CH0, 206_000, 4_000_000),
            path(MASTER_IPA, SLAVE_OCIMEM, 206_000, 712_961),
            path(MASTER_AMPSS_M0, SLAVE_IPA_CFG, 206_000, 491_520),
            path(MASTER_IPA_CORE, SLAVE_IPA_CORE, 0, 491),
        ],
    },
    UseCase {
        index: 4,
        name: "TURBO",
        paths: [
            path(MASTER_IPA, SLAVE_EBI_CH0, 206_000, 5_598_900),
            path(MASTER_IPA, SLAVE_OCIMEM, 206_000, 1_436_481),
            path(MASTER_AMPSS_M0, SLAVE_IPA_CFG, 206_000, 491_520),
            path(MASTER_IPA_CORE, SLAVE_IPA_CORE, 0, 491),
        ],
    },
];

/// Select the exact DTS `msm_bus` usecase for a source-derived IPA v4.2 rate.
/// An unsupported rate or usecase table is rejected; Linux's warning-and-SVS2
/// fallback is intentionally not treated as a verified power vote.
pub fn use_case_for_clock_rate(
    hardware_type: u32,
    clock_rate_hz: u32,
    use_case_count: usize,
) -> Result<&'static UseCase, Error> {
    if hardware_type != SUNFISH_IPA_HW_V4_2 {
        return Err(Error::WrongHardwareType);
    }
    if use_case_count != USE_CASE_COUNT {
        return Err(Error::WrongUseCaseCount {
            expected: USE_CASE_COUNT,
            actual: use_case_count,
        });
    }
    let index = match clock_rate_hz {
        RATE_SVS2_HZ => 1,
        RATE_SVS_HZ => 2,
        RATE_NOMINAL_HZ => 3,
        RATE_TURBO_HZ => USE_CASE_COUNT - 1,
        other => return Err(Error::UnsupportedClockRate(other)),
    };
    Ok(&USE_CASES[index])
}

/// The usecase Linux submits when it disables the IPA clock vote.
pub const fn disabled_use_case() -> &'static UseCase {
    &USE_CASES[0]
}

/// The boot rate set by the pinned Linux probe before its first IPA register
/// access. This selects usecase 1 (`SVS2`); it is not itself a power vote.
pub const fn boot_clock_rate_hz() -> u32 {
    RATE_SVS2_HZ
}
