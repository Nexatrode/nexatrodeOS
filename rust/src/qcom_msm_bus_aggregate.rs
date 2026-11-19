//! Bounded, pure MSM bus-node aggregation for a complete route snapshot.
//!
//! The pinned legacy `aggregate_bus_req()` sums average bandwidth and takes
//! the maximum instantaneous bandwidth across every link request on a bus
//! node, independently for ACTIVE and DUAL contexts. This module models that
//! step only. It does not discover the client universe, choose routes, scale
//! to BCM units, submit RPMh commands, or authorize a hardware vote.
#![forbid(unsafe_code)]

use crate::qcom_bcm::Metadata;
use crate::qcom_bcm_vote::{
    Aggregation as BcmAggregation, Bandwidth, Contexts, Error as BcmError, Vectors,
    legacy_update_ordered, scale_node,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidIdentity,
    InvalidGeneration,
    EmptyClientSet,
    EmptyNodeSet,
    DuplicateClient,
    MissingClient,
    UnexpectedClient,
    ClientIdentity,
    ClientGeneration,
    DuplicateRoute,
    UnknownRoute,
    DuplicateClientRoute,
    EmptyRoute,
    DuplicateRouteNode,
    DuplicateSnapshotNode,
    UnknownSnapshotNode,
    MissingRouteNode,
    UnexpectedRouteNode,
    DuplicateRouteNodeVote,
    DuplicateNodeAggregate,
    MissingNodeAggregate,
    DuplicateBcmLink,
    EmptyBcmSet,
    EmptyBcmLinkSet,
    BcmLinkOverflow,
    BcmNodeMismatch,
    RouteCapacity,
    VoteCapacity,
    LedgerFaulted,
    Bcm(BcmError),
    Overflow,
}

/// One ordered source-to-destination route in the pinned bus graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route<'a> {
    pub id: u16,
    pub nodes: &'a [u16],
}

/// One per-route contribution to a bus node. Repeated bus-node IDs are valid
/// when different paths from the same client traverse that node; those link
/// requests are independently aggregated just as Linux's link-node entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteNodeVote {
    pub route_id: u16,
    pub node_id: u16,
    pub contexts: Contexts,
}

/// Complete contribution report for one bus client in one platform epoch.
/// `route_ids` includes every configured route owned by this client, even if
/// its current request is zero. Every node on each listed route must have an
/// explicit `RouteNodeVote` so omission cannot silently understate a vote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientReport<'a> {
    pub client_id: u16,
    pub identity: u64,
    pub generation: u64,
    pub route_ids: &'a [u16],
    pub votes: &'a [RouteNodeVote],
}

/// One immutable, caller-attested complete view of the bus clients and routes.
/// The caller must obtain `expected_clients` and the shared identity/generation
/// from the authoritative platform owner; this value alone is not a lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot<'a> {
    pub identity: u64,
    pub generation: u64,
    pub expected_clients: &'a [u16],
    pub routes: &'a [Route<'a>],
    pub clients: &'a [ClientReport<'a>],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeAggregate {
    pub node_id: u16,
    pub contexts: Contexts,
}

/// DT-derived geometry used by Linux when converting one node aggregate to a
/// BCM link contribution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeGeometry {
    pub node_id: u16,
    pub bus_width: u32,
    pub aggregation_ports: u32,
}

/// One BCM's full, source-ordered list of linked bus-node contributions.
/// `links` must come from the pinned DT topology; this model does not discover
/// the BCM binding set or prove that a supplied list is complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BcmBinding<'a> {
    pub metadata: Metadata,
    pub links: &'a [NodeGeometry],
    pub prior_acv_masks: [u32; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClientState<const MAX_ROUTES: usize, const MAX_VOTES: usize> {
    present: bool,
    route_ids: [u16; MAX_ROUTES],
    route_count: usize,
    votes: [RouteNodeVote; MAX_VOTES],
    vote_count: usize,
}

impl<const MAX_ROUTES: usize, const MAX_VOTES: usize> ClientState<MAX_ROUTES, MAX_VOTES> {
    fn empty() -> Self {
        Self {
            present: false,
            route_ids: [0; MAX_ROUTES],
            route_count: 0,
            votes: [empty_route_node_vote(); MAX_VOTES],
            vote_count: 0,
        }
    }

    fn report(&self, client_id: u16, identity: u64, generation: u64) -> ClientReport<'_> {
        ClientReport {
            client_id,
            identity,
            generation,
            route_ids: &self.route_ids[..self.route_count],
            votes: &self.votes[..self.vote_count],
        }
    }
}

/// Stateful, fixed-capacity ledger for a platform owner's complete MSM-bus
/// client set. The caller must derive `expected_clients` and `routes` from the
/// authoritative system configuration. The ledger requires one current
/// generation-matched report per client before producing aggregate totals; it
/// does not itself grant RPMh or hardware-vote authority.
pub struct MsmBusLedger<'a, const CLIENTS: usize, const MAX_ROUTES: usize, const MAX_VOTES: usize> {
    identity: u64,
    generation: u64,
    expected_clients: [u16; CLIENTS],
    routes: &'a [Route<'a>],
    clients: [ClientState<MAX_ROUTES, MAX_VOTES>; CLIENTS],
    faulted: bool,
}

impl<'a, const CLIENTS: usize, const MAX_ROUTES: usize, const MAX_VOTES: usize>
    MsmBusLedger<'a, CLIENTS, MAX_ROUTES, MAX_VOTES>
{
    pub fn new(
        identity: u64,
        generation: u64,
        expected_clients: [u16; CLIENTS],
        routes: &'a [Route<'a>],
    ) -> Result<Self, Error> {
        if identity == 0 {
            return Err(Error::InvalidIdentity);
        }
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if CLIENTS == 0 {
            return Err(Error::EmptyClientSet);
        }
        for (index, client_id) in expected_clients.iter().enumerate() {
            if expected_clients[..index].contains(client_id) {
                return Err(Error::DuplicateClient);
            }
        }
        for (index, candidate) in routes.iter().enumerate() {
            if routes[..index]
                .iter()
                .any(|previous| previous.id == candidate.id)
            {
                return Err(Error::DuplicateRoute);
            }
            if candidate.nodes.is_empty() {
                return Err(Error::EmptyRoute);
            }
            for (node_index, node_id) in candidate.nodes.iter().enumerate() {
                if candidate.nodes[..node_index].contains(node_id) {
                    return Err(Error::DuplicateRouteNode);
                }
            }
        }
        Ok(Self {
            identity,
            generation,
            expected_clients,
            routes,
            clients: core::array::from_fn(|_| ClientState::empty()),
            faulted: false,
        })
    }

    /// Replace one client's current route report. An invalid identity,
    /// generation, client, or route report permanently faults this ledger for
    /// the current platform generation; callers must establish a new owner
    /// generation before it can issue another aggregate.
    pub fn update(&mut self, report: ClientReport<'_>) -> Result<(), Error> {
        if self.faulted {
            return Err(Error::LedgerFaulted);
        }
        if report.identity != self.identity {
            self.faulted = true;
            return Err(Error::ClientIdentity);
        }
        if report.generation != self.generation {
            self.faulted = true;
            return Err(Error::ClientGeneration);
        }
        let Some(index) = self
            .expected_clients
            .iter()
            .position(|client_id| *client_id == report.client_id)
        else {
            self.faulted = true;
            return Err(Error::UnexpectedClient);
        };
        if report.route_ids.len() > MAX_ROUTES {
            self.faulted = true;
            return Err(Error::RouteCapacity);
        }
        if report.votes.len() > MAX_VOTES {
            self.faulted = true;
            return Err(Error::VoteCapacity);
        }
        validate_report(self.routes, report).inspect_err(|_| self.faulted = true)?;

        let client = &mut self.clients[index];
        client.route_ids.fill(0);
        client.route_ids[..report.route_ids.len()].copy_from_slice(report.route_ids);
        client.route_count = report.route_ids.len();
        client.votes.fill(empty_route_node_vote());
        client.votes[..report.votes.len()].copy_from_slice(report.votes);
        client.vote_count = report.votes.len();
        client.present = true;
        Ok(())
    }

    /// Aggregate only after every expected client has submitted a validated
    /// report. The supplied node IDs must be the complete selected topology
    /// consumed by this platform ledger.
    pub fn aggregate<const N: usize>(
        &self,
        node_ids: [u16; N],
    ) -> Result<[NodeAggregate; N], Error> {
        if self.faulted {
            return Err(Error::LedgerFaulted);
        }
        if self.clients.iter().any(|client| !client.present) {
            return Err(Error::MissingClient);
        }
        let clients: [ClientReport<'_>; CLIENTS] = core::array::from_fn(|index| {
            self.clients[index].report(self.expected_clients[index], self.identity, self.generation)
        });
        aggregate(
            Snapshot {
                identity: self.identity,
                generation: self.generation,
                expected_clients: &self.expected_clients,
                routes: self.routes,
                clients: &clients,
            },
            node_ids,
        )
    }
}

fn validate_report(routes: &[Route<'_>], report: ClientReport<'_>) -> Result<(), Error> {
    let mut expected_votes = 0usize;
    for (route_index, route_id) in report.route_ids.iter().enumerate() {
        if report.route_ids[..route_index].contains(route_id) {
            return Err(Error::DuplicateClientRoute);
        }
        let route = route(routes, *route_id).ok_or(Error::UnknownRoute)?;
        expected_votes = expected_votes
            .checked_add(route.nodes.len())
            .ok_or(Error::Overflow)?;
    }
    for (vote_index, vote) in report.votes.iter().enumerate() {
        if !has(report.route_ids, vote.route_id) {
            return Err(Error::UnexpectedRouteNode);
        }
        let route = route(routes, vote.route_id).ok_or(Error::UnknownRoute)?;
        if !has(route.nodes, vote.node_id) {
            return Err(Error::UnexpectedRouteNode);
        }
        if report.votes[..vote_index]
            .iter()
            .any(|prior| prior.route_id == vote.route_id && prior.node_id == vote.node_id)
        {
            return Err(Error::DuplicateRouteNodeVote);
        }
    }
    if report.votes.len() != expected_votes {
        return Err(if report.votes.len() < expected_votes {
            Error::MissingRouteNode
        } else {
            Error::UnexpectedRouteNode
        });
    }
    for route_id in report.route_ids {
        let route = route(routes, *route_id).ok_or(Error::UnknownRoute)?;
        for node_id in route.nodes {
            if !report
                .votes
                .iter()
                .any(|vote| vote.route_id == *route_id && vote.node_id == *node_id)
            {
                return Err(Error::MissingRouteNode);
            }
        }
    }
    Ok(())
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

fn route<'a>(routes: &'a [Route<'a>], id: u16) -> Option<&'a Route<'a>> {
    routes.iter().find(|route| route.id == id)
}

fn has(values: &[u16], sought: u16) -> bool {
    values.contains(&sought)
}

/// Validate the closed client/route set, then match Linux's per-node
/// `sum_ab`/`max_ib` aggregation for ACTIVE and DUAL contexts.
pub fn aggregate<const N: usize>(
    snapshot: Snapshot<'_>,
    node_ids: [u16; N],
) -> Result<[NodeAggregate; N], Error> {
    if snapshot.identity == 0 {
        return Err(Error::InvalidIdentity);
    }
    if snapshot.generation == 0 {
        return Err(Error::InvalidGeneration);
    }
    if snapshot.expected_clients.is_empty() {
        return Err(Error::EmptyClientSet);
    }
    if N == 0 {
        return Err(Error::EmptyNodeSet);
    }

    for (index, client_id) in snapshot.expected_clients.iter().enumerate() {
        if snapshot.expected_clients[..index].contains(client_id) {
            return Err(Error::DuplicateClient);
        }
    }
    for (index, route) in snapshot.routes.iter().enumerate() {
        if snapshot.routes[..index]
            .iter()
            .any(|prior| prior.id == route.id)
        {
            return Err(Error::DuplicateRoute);
        }
        if route.nodes.is_empty() {
            return Err(Error::EmptyRoute);
        }
        for (node_index, node_id) in route.nodes.iter().enumerate() {
            if route.nodes[..node_index].contains(node_id) {
                return Err(Error::DuplicateRouteNode);
            }
            if !node_ids.contains(node_id) {
                return Err(Error::UnknownSnapshotNode);
            }
        }
    }
    for (index, node_id) in node_ids.iter().enumerate() {
        if node_ids[..index].contains(node_id) {
            return Err(Error::DuplicateSnapshotNode);
        }
    }

    if snapshot.clients.len() != snapshot.expected_clients.len() {
        return Err(
            if snapshot.clients.len() < snapshot.expected_clients.len() {
                Error::MissingClient
            } else {
                Error::UnexpectedClient
            },
        );
    }
    for (index, client) in snapshot.clients.iter().enumerate() {
        if client.identity != snapshot.identity {
            return Err(Error::ClientIdentity);
        }
        if client.generation != snapshot.generation {
            return Err(Error::ClientGeneration);
        }
        if !has(snapshot.expected_clients, client.client_id) {
            return Err(Error::UnexpectedClient);
        }
        if snapshot.clients[..index]
            .iter()
            .any(|prior| prior.client_id == client.client_id)
        {
            return Err(Error::DuplicateClient);
        }
        for (route_index, route_id) in client.route_ids.iter().enumerate() {
            if client.route_ids[..route_index].contains(route_id) {
                return Err(Error::DuplicateClientRoute);
            }
            if route(&snapshot.routes, *route_id).is_none() {
                return Err(Error::UnknownRoute);
            }
        }

        let expected_votes = client
            .route_ids
            .iter()
            .try_fold(0usize, |total, route_id| {
                total
                    .checked_add(route(&snapshot.routes, *route_id).unwrap().nodes.len())
                    .ok_or(Error::Overflow)
            })?;
        for (vote_index, vote) in client.votes.iter().enumerate() {
            if !has(client.route_ids, vote.route_id) {
                return Err(Error::UnexpectedRouteNode);
            }
            let route = route(&snapshot.routes, vote.route_id).ok_or(Error::UnknownRoute)?;
            if !has(route.nodes, vote.node_id) {
                return Err(Error::UnexpectedRouteNode);
            }
            if client.votes[..vote_index]
                .iter()
                .any(|prior| prior.route_id == vote.route_id && prior.node_id == vote.node_id)
            {
                return Err(Error::DuplicateRouteNodeVote);
            }
        }
        if client.votes.len() != expected_votes {
            return Err(if client.votes.len() < expected_votes {
                Error::MissingRouteNode
            } else {
                Error::UnexpectedRouteNode
            });
        }
        for route_id in client.route_ids {
            let route = route(&snapshot.routes, *route_id).ok_or(Error::UnknownRoute)?;
            for node_id in route.nodes {
                if !client
                    .votes
                    .iter()
                    .any(|vote| vote.route_id == *route_id && vote.node_id == *node_id)
                {
                    return Err(Error::MissingRouteNode);
                }
            }
        }
    }
    for client_id in snapshot.expected_clients {
        if !snapshot
            .clients
            .iter()
            .any(|client| client.client_id == *client_id)
        {
            return Err(Error::MissingClient);
        }
    }

    let mut output = [NodeAggregate::default(); N];
    for (output, node_id) in output.iter_mut().zip(node_ids) {
        output.node_id = node_id;
    }
    for client in snapshot.clients {
        for vote in client.votes {
            let index = node_ids
                .iter()
                .position(|node_id| *node_id == vote.node_id)
                .ok_or(Error::UnknownSnapshotNode)?;
            let contexts = &mut output[index].contexts;
            add(&mut contexts.active_only, vote.contexts.active_only)?;
            add(&mut contexts.dual, vote.contexts.dual)?;
        }
    }
    Ok(output)
}

fn add(target: &mut Bandwidth, contribution: Bandwidth) -> Result<(), Error> {
    target.average = target
        .average
        .checked_add(contribution.average)
        .ok_or(Error::Overflow)?;
    target.peak = target.peak.max(contribution.peak);
    Ok(())
}

/// Scale a complete node aggregate into the link contribution stored for one
/// BCM. The BCM's width and unit size come from its validated CMD_DB metadata;
/// node width and aggregation-port count come from the selected DT route.
pub fn scale_for_bcm(
    node: NodeAggregate,
    geometry: NodeGeometry,
    metadata: Metadata,
) -> Result<Contexts, Error> {
    if node.node_id != geometry.node_id {
        return Err(Error::BcmNodeMismatch);
    }
    Ok(Contexts {
        active_only: scale_node(
            &metadata,
            node.contexts.active_only,
            geometry.bus_width,
            geometry.aggregation_ports,
        )
        .map_err(Error::Bcm)?,
        dual: scale_node(
            &metadata,
            node.contexts.dual,
            geometry.bus_width,
            geometry.aggregation_ports,
        )
        .map_err(Error::Bcm)?,
    })
}

/// Scale every already-aggregated node through each BCM link, then reproduce
/// the pinned Linux BCM update order and cross-resource accumulator. The BCM
/// bindings must be passed in their complete source-defined DT order. This is
/// still pure planning: it does not prove that the live client set, binding
/// list, prior ACV state, or RPMh ownership is authoritative.
pub fn aggregate_bcm_links<const BCMS: usize, const MAX_LINKS: usize>(
    nodes: &[NodeAggregate],
    bindings: [BcmBinding<'_>; BCMS],
) -> Result<[Vectors; BCMS], Error> {
    if BCMS == 0 {
        return Err(Error::EmptyBcmSet);
    }
    if MAX_LINKS == 0 {
        return Err(Error::EmptyBcmLinkSet);
    }
    for (index, node) in nodes.iter().enumerate() {
        if nodes[..index]
            .iter()
            .any(|previous| previous.node_id == node.node_id)
        {
            return Err(Error::DuplicateNodeAggregate);
        }
    }

    let mut scaled = [[Contexts::default(); MAX_LINKS]; BCMS];
    let mut lengths = [0usize; BCMS];
    for (binding_index, binding) in bindings.iter().enumerate() {
        if binding.links.is_empty() {
            return Err(Error::EmptyBcmLinkSet);
        }
        if binding.links.len() > MAX_LINKS {
            return Err(Error::BcmLinkOverflow);
        }
        for (link_index, geometry) in binding.links.iter().copied().enumerate() {
            if binding.links[..link_index]
                .iter()
                .any(|previous| previous.node_id == geometry.node_id)
            {
                return Err(Error::DuplicateBcmLink);
            }
            let node = nodes
                .iter()
                .find(|node| node.node_id == geometry.node_id)
                .ok_or(Error::MissingNodeAggregate)?;
            scaled[binding_index][link_index] = scale_for_bcm(*node, geometry, binding.metadata)?;
        }
        lengths[binding_index] = binding.links.len();
    }

    let ordered: [BcmAggregation<'_>; BCMS] = core::array::from_fn(|index| BcmAggregation {
        metadata: bindings[index].metadata,
        nodes: &scaled[index][..lengths[index]],
        prior_acv_masks: bindings[index].prior_acv_masks,
    });
    legacy_update_ordered(ordered).map_err(Error::Bcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: u64 = 0x534d_3731_3530;
    const GENERATION: u64 = 7;

    fn bw(average: u64, peak: u64) -> Bandwidth {
        Bandwidth { average, peak }
    }

    fn contexts(active: Bandwidth, dual: Bandwidth) -> Contexts {
        Contexts {
            active_only: active,
            dual,
        }
    }

    #[test]
    fn aggregate_sums_average_and_maxes_peak_per_node_and_context() {
        let nodes_a = [10, 20];
        let nodes_b = [20, 30];
        let routes = [
            Route {
                id: 1,
                nodes: &nodes_a,
            },
            Route {
                id: 2,
                nodes: &nodes_b,
            },
        ];
        let a_votes = [
            RouteNodeVote {
                route_id: 1,
                node_id: 10,
                contexts: contexts(bw(3, 8), bw(30, 80)),
            },
            RouteNodeVote {
                route_id: 1,
                node_id: 20,
                contexts: contexts(bw(3, 8), bw(30, 80)),
            },
        ];
        let b_votes = [
            RouteNodeVote {
                route_id: 2,
                node_id: 20,
                contexts: contexts(bw(5, 7), bw(50, 70)),
            },
            RouteNodeVote {
                route_id: 2,
                node_id: 30,
                contexts: contexts(bw(5, 7), bw(50, 70)),
            },
        ];
        let expected_clients = [1, 2];
        let clients = [
            ClientReport {
                client_id: 1,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &a_votes,
            },
            ClientReport {
                client_id: 2,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[2],
                votes: &b_votes,
            },
        ];
        let snapshot = Snapshot {
            identity: IDENTITY,
            generation: GENERATION,
            expected_clients: &expected_clients,
            routes: &routes,
            clients: &clients,
        };

        let result = aggregate(snapshot, [10, 20, 30]).unwrap();
        assert_eq!(result[0].contexts, contexts(bw(3, 8), bw(30, 80)));
        assert_eq!(result[1].contexts, contexts(bw(8, 8), bw(80, 80)));
        assert_eq!(result[2].contexts, contexts(bw(5, 7), bw(50, 70)));
    }

    #[test]
    fn zero_vote_client_must_still_be_present_in_complete_set() {
        let node = [10];
        let route = [Route {
            id: 1,
            nodes: &node,
        }];
        let expected = [1, 2];
        let vote = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: Contexts::default(),
        }];
        let clients = [ClientReport {
            client_id: 1,
            identity: IDENTITY,
            generation: GENERATION,
            route_ids: &[1],
            votes: &vote,
        }];
        assert_eq!(
            aggregate(
                Snapshot {
                    identity: IDENTITY,
                    generation: GENERATION,
                    expected_clients: &expected,
                    routes: &route,
                    clients: &clients,
                },
                [10]
            ),
            Err(Error::MissingClient)
        );
    }

    #[test]
    fn omitted_route_node_is_rejected_instead_of_under_voting() {
        let nodes = [10, 20];
        let routes = [Route {
            id: 1,
            nodes: &nodes,
        }];
        let expected = [1];
        let votes = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: Contexts::default(),
        }];
        let clients = [ClientReport {
            client_id: 1,
            identity: IDENTITY,
            generation: GENERATION,
            route_ids: &[1],
            votes: &votes,
        }];
        assert_eq!(
            aggregate(
                Snapshot {
                    identity: IDENTITY,
                    generation: GENERATION,
                    expected_clients: &expected,
                    routes: &routes,
                    clients: &clients,
                },
                [10, 20]
            ),
            Err(Error::MissingRouteNode)
        );
    }

    #[test]
    fn generation_mismatch_and_duplicate_route_node_are_rejected() {
        let nodes = [10];
        let routes = [Route {
            id: 1,
            nodes: &nodes,
        }];
        let expected = [1];
        let votes = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: Contexts::default(),
        }];
        let mut clients = [ClientReport {
            client_id: 1,
            identity: IDENTITY,
            generation: GENERATION + 1,
            route_ids: &[1],
            votes: &votes,
        }];
        fn make_snapshot<'a>(
            expected: &'a [u16],
            routes: &'a [Route<'a>],
            clients: &'a [ClientReport<'a>],
        ) -> Snapshot<'a> {
            Snapshot {
                identity: IDENTITY,
                generation: GENERATION,
                expected_clients: expected,
                routes,
                clients,
            }
        }
        assert_eq!(
            aggregate(make_snapshot(&expected, &routes, &clients), [10]),
            Err(Error::ClientGeneration)
        );
        clients[0].generation = GENERATION;
        let duplicate = [votes[0], votes[0]];
        clients[0].votes = &duplicate;
        assert_eq!(
            aggregate(make_snapshot(&expected, &routes, &clients), [10]),
            Err(Error::DuplicateRouteNodeVote)
        );
    }

    #[test]
    fn average_overflow_fails_closed() {
        let nodes = [10];
        let routes = [Route {
            id: 1,
            nodes: &nodes,
        }];
        let expected = [1, 2];
        let votes_a = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: contexts(bw(u64::MAX, 0), Bandwidth::default()),
        }];
        let votes_b = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: contexts(bw(1, 0), Bandwidth::default()),
        }];
        let clients = [
            ClientReport {
                client_id: 1,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &votes_a,
            },
            ClientReport {
                client_id: 2,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &votes_b,
            },
        ];
        assert_eq!(
            aggregate(
                Snapshot {
                    identity: IDENTITY,
                    generation: GENERATION,
                    expected_clients: &expected,
                    routes: &routes,
                    clients: &clients,
                },
                [10]
            ),
            Err(Error::Overflow)
        );
    }

    #[test]
    fn scales_each_context_into_linux_bcm_link_units() {
        let node = NodeAggregate {
            node_id: 512,
            contexts: contexts(bw(8, 8), bw(16, 4)),
        };
        let geometry = NodeGeometry {
            node_id: 512,
            bus_width: 4,
            aggregation_ports: 2,
        };
        let metadata = Metadata {
            id: *b"MC0\0\0\0\0\0",
            address: 0x50000,
            unit_size: 0x1b0210,
            width: 16,
            clock_domain: 0,
            reserved: 0,
            auxiliary_version: 0x1000,
        };
        assert_eq!(
            scale_for_bcm(node, geometry, metadata),
            Ok(contexts(bw(16, 32), bw(32, 16)))
        );
    }

    #[test]
    fn scales_complete_links_then_uses_linux_bcm_binding_order_and_carry() {
        use crate::qcom_bcm_vote::Vector;

        let mc0 = Metadata {
            id: *b"MC0\0\0\0\0\0",
            address: 0x50000,
            unit_size: 1_770_000,
            width: 16,
            clock_domain: 0,
            reserved: 0,
            auxiliary_version: 0x1000,
        };
        let acv = Metadata {
            id: *b"ACV\0\0\0\0\0",
            address: 0x5000c,
            unit_size: 1,
            width: 0,
            clock_domain: 11,
            reserved: 0,
            auxiliary_version: 0x1000,
        };
        let node = NodeAggregate {
            node_id: 512,
            contexts: contexts(Bandwidth::default(), bw(0, 7_000_000_000)),
        };
        let geometry = [NodeGeometry {
            node_id: 512,
            bus_width: 4,
            aggregation_ports: 2,
        }];
        let result = aggregate_bcm_links::<2, 1>(
            &[node],
            [
                BcmBinding {
                    metadata: mc0,
                    links: &geometry,
                    prior_acv_masks: [0; 2],
                },
                BcmBinding {
                    metadata: acv,
                    links: &geometry,
                    prior_acv_masks: [0x11, 0x20],
                },
            ],
        )
        .unwrap();

        assert_eq!(result[0].active, Vector { x: 0, y: 15_819 });
        assert_eq!(result[0].sleep, Vector { x: 0, y: 15_819 });
        assert_eq!(result[1].active, Vector { x: 0, y: 0x19 });
        assert_eq!(result[1].sleep, Vector { x: 0, y: 0x28 });
    }

    #[test]
    fn bcm_binding_cannot_omit_a_scaled_node_aggregate() {
        let metadata = Metadata {
            id: *b"MC0\0\0\0\0\0",
            address: 0x50000,
            unit_size: 1_770_000,
            width: 16,
            clock_domain: 0,
            reserved: 0,
            auxiliary_version: 0x1000,
        };
        let links = [NodeGeometry {
            node_id: 512,
            bus_width: 4,
            aggregation_ports: 2,
        }];
        assert_eq!(
            aggregate_bcm_links::<1, 1>(
                &[],
                [BcmBinding {
                    metadata,
                    links: &links,
                    prior_acv_masks: [0; 2],
                }],
            ),
            Err(Error::MissingNodeAggregate)
        );
    }

    #[test]
    fn ledger_waits_for_complete_client_set_and_replaces_current_votes() {
        let nodes = [10];
        let routes = [Route {
            id: 1,
            nodes: &nodes,
        }];
        let mut ledger =
            MsmBusLedger::<2, 1, 1>::new(IDENTITY, GENERATION, [1, 2], &routes).unwrap();
        assert_eq!(ledger.aggregate([10]), Err(Error::MissingClient));

        let vote_a = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: contexts(Bandwidth::default(), bw(3, 8)),
        }];
        ledger
            .update(ClientReport {
                client_id: 1,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &vote_a,
            })
            .unwrap();
        assert_eq!(ledger.aggregate([10]), Err(Error::MissingClient));

        let vote_b = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: contexts(Bandwidth::default(), bw(5, 7)),
        }];
        ledger
            .update(ClientReport {
                client_id: 2,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &vote_b,
            })
            .unwrap();
        assert_eq!(
            ledger.aggregate([10]),
            Ok([NodeAggregate {
                node_id: 10,
                contexts: contexts(Bandwidth::default(), bw(8, 8)),
            }])
        );

        let updated_vote_a = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: contexts(Bandwidth::default(), bw(11, 4)),
        }];
        ledger
            .update(ClientReport {
                client_id: 1,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &updated_vote_a,
            })
            .unwrap();
        assert_eq!(
            ledger.aggregate([10]),
            Ok([NodeAggregate {
                node_id: 10,
                contexts: contexts(Bandwidth::default(), bw(16, 7)),
            }])
        );
    }

    #[test]
    fn ledger_faults_on_unknown_client_and_rejects_further_updates() {
        let nodes = [10];
        let routes = [Route {
            id: 1,
            nodes: &nodes,
        }];
        let mut ledger = MsmBusLedger::<1, 1, 1>::new(IDENTITY, GENERATION, [1], &routes).unwrap();
        let votes = [RouteNodeVote {
            route_id: 1,
            node_id: 10,
            contexts: Contexts::default(),
        }];
        assert_eq!(
            ledger.update(ClientReport {
                client_id: 2,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &votes,
            }),
            Err(Error::UnexpectedClient)
        );
        assert_eq!(ledger.aggregate([10]), Err(Error::LedgerFaulted));
        assert_eq!(
            ledger.update(ClientReport {
                client_id: 1,
                identity: IDENTITY,
                generation: GENERATION,
                route_ids: &[1],
                votes: &votes,
            }),
            Err(Error::LedgerFaulted)
        );
    }
}
