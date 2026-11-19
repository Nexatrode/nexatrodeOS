#![forbid(unsafe_code)]
// The test imports source modules to exercise pure vote arithmetic; ancillary
// decoder/controller entry points outside that test surface are intentionally unused.
#![allow(dead_code)]

#[path = "../rust/src/sunfish_ipa_bus_vote.rs"]
mod sunfish_ipa_bus_vote;

use sunfish_ipa_bus_vote::{
    boot_clock_rate_hz, client_contribution, disabled_use_case, sunfish_path_routes,
    sunfish_routes, use_case_for_clock_rate, Error, PathRoute, DUAL_CONTEXT, MASTER_AMPSS_M0,
    MASTER_IPA, MASTER_IPA_CORE, RATE_NOMINAL_HZ, RATE_SVS2_HZ, RATE_SVS_HZ, RATE_TURBO_HZ,
    SLAVE_EBI_CH0, SLAVE_IPA_CFG, SLAVE_IPA_CORE, SLAVE_OCIMEM, SUNFISH_IPA_HW_V4_2,
    USE_CASE_COUNT,
};

#[path = "../rust/src/qcom_bcm_vote.rs"]
mod qcom_bcm_vote;

#[path = "../rust/src/qcom_bcm.rs"]
mod qcom_bcm;

#[path = "../rust/src/qcom_cmd_db.rs"]
mod qcom_cmd_db;

#[path = "../rust/src/qcom_rpmh_read.rs"]
mod qcom_rpmh_read;

#[path = "../rust/src/qcom_msm_bus_aggregate.rs"]
mod qcom_msm_bus_aggregate;

#[path = "../rust/src/qcom_rpmh_handoff.rs"]
mod qcom_rpmh_handoff;

#[path = "../rust/src/qcom_rpmh_vote_aggregate.rs"]
mod qcom_rpmh_vote_aggregate;

#[path = "../rust/src/qcom_rpmh_vote_broker.rs"]
mod qcom_rpmh_vote_broker;

#[path = "../rust/src/qcom_rpmh_vrm_command.rs"]
mod qcom_rpmh_vrm_command;

#[path = "../rust/src/qcom_rpmh_vrm_tcs.rs"]
mod qcom_rpmh_vrm_tcs;

#[test]
fn boot_clock_rate_selects_the_exact_svs2_bus_vote() {
    let vote =
        use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, boot_clock_rate_hz(), USE_CASE_COUNT).unwrap();
    assert_eq!((vote.index, vote.name), (1, "SVS2"));
    assert_eq!(
        vote.paths.map(|path| (path.master, path.slave)),
        [
            (MASTER_IPA, SLAVE_EBI_CH0),
            (MASTER_IPA, SLAVE_OCIMEM),
            (MASTER_AMPSS_M0, SLAVE_IPA_CFG),
            (MASTER_IPA_CORE, SLAVE_IPA_CORE),
        ]
    );
    assert_eq!(
        vote.paths
            .map(|path| (path.average_kbps, path.instantaneous_kbps)),
        [(80_000, 465_000), (80_000, 68_570), (80_000, 30), (0, 30)]
    );
}

#[test]
fn supported_clock_rates_map_to_the_source_usecase_indices() {
    for (rate, index, name, bandwidths) in [
        (
            RATE_SVS2_HZ,
            1,
            "SVS2",
            [(80_000, 465_000), (80_000, 68_570), (80_000, 30), (0, 30)],
        ),
        (
            RATE_SVS_HZ,
            2,
            "SVS",
            [
                (80_000, 2_000_000),
                (80_000, 267_461),
                (80_000, 109_890),
                (0, 109),
            ],
        ),
        (
            RATE_NOMINAL_HZ,
            3,
            "NOMINAL",
            [
                (206_000, 4_000_000),
                (206_000, 712_961),
                (206_000, 491_520),
                (0, 491),
            ],
        ),
        (
            RATE_TURBO_HZ,
            4,
            "TURBO",
            [
                (206_000, 5_598_900),
                (206_000, 1_436_481),
                (206_000, 491_520),
                (0, 491),
            ],
        ),
    ] {
        let vote = use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, rate, USE_CASE_COUNT).unwrap();
        assert_eq!((vote.index, vote.name), (index, name));
        assert_eq!(
            vote.paths
                .map(|path| (path.average_kbps, path.instantaneous_kbps)),
            bandwidths
        );
    }
}

#[test]
fn unsupported_platform_rate_and_dts_shape_fail_closed() {
    assert_eq!(
        use_case_for_clock_rate(15, RATE_SVS2_HZ, USE_CASE_COUNT),
        Err(Error::WrongHardwareType)
    );
    assert_eq!(
        use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, 77_000_000, USE_CASE_COUNT),
        Err(Error::UnsupportedClockRate(77_000_000))
    );
    assert_eq!(
        use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, RATE_SVS2_HZ, 4),
        Err(Error::WrongUseCaseCount {
            expected: 5,
            actual: 4,
        })
    );
}

#[test]
fn disable_selects_the_zero_bandwidth_minimum_case() {
    let vote = disabled_use_case();
    assert_eq!((vote.index, vote.name), (0, "MIN"));
    assert!(vote
        .paths
        .iter()
        .all(|path| path.average_kbps == 0 && path.instantaneous_kbps == 0));
}

#[test]
fn ipa_vectors_use_the_source_default_dual_context_and_bytes_per_second() {
    assert!(DUAL_CONTEXT);
    let vote =
        use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, boot_clock_rate_hz(), USE_CASE_COUNT).unwrap();
    let contexts = vote.contexts();
    assert_eq!(
        contexts,
        [
            qcom_bcm_vote::Contexts {
                active_only: qcom_bcm_vote::Bandwidth::default(),
                dual: qcom_bcm_vote::Bandwidth {
                    average: 80_000_000,
                    peak: 465_000_000,
                },
            },
            qcom_bcm_vote::Contexts {
                active_only: qcom_bcm_vote::Bandwidth::default(),
                dual: qcom_bcm_vote::Bandwidth {
                    average: 80_000_000,
                    peak: 68_570_000,
                },
            },
            qcom_bcm_vote::Contexts {
                active_only: qcom_bcm_vote::Bandwidth::default(),
                dual: qcom_bcm_vote::Bandwidth {
                    average: 80_000_000,
                    peak: 30_000,
                },
            },
            qcom_bcm_vote::Contexts {
                active_only: qcom_bcm_vote::Bandwidth::default(),
                dual: qcom_bcm_vote::Bandwidth {
                    average: 0,
                    peak: 30_000,
                },
            },
        ]
    );
}

#[test]
fn ipa_usecase_expands_to_route_nodes_and_aggregates_with_other_clients() {
    use qcom_bcm_vote::{Bandwidth, Contexts};
    use qcom_msm_bus_aggregate::{ClientReport, MsmBusLedger, RouteNodeVote};

    let route_ids = [10, 11, 12, 13];
    let routes = sunfish_routes(route_ids);
    let path_routes = sunfish_path_routes(route_ids);
    let ipa_use_case =
        use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, boot_clock_rate_hz(), USE_CASE_COUNT).unwrap();
    let ipa_contribution = client_contribution::<20>(&ipa_use_case, path_routes).unwrap();
    let ipa_client = ipa_contribution.report(90, 0x5355_4e46, 7).unwrap();
    assert_eq!(ipa_client.route_ids, route_ids);
    assert_eq!(ipa_client.votes.len(), 20);

    let other_contexts = Contexts {
        active_only: Bandwidth::default(),
        dual: Bandwidth {
            average: 10_000_000,
            peak: 400_000_000,
        },
    };
    let other_route_ids = [10];
    let other_votes: [RouteNodeVote; 8] = core::array::from_fn(|index| RouteNodeVote {
        route_id: 10,
        node_id: routes[0].nodes[index],
        contexts: other_contexts,
    });
    let other_client = ClientReport {
        client_id: 91,
        identity: 0x5355_4e46,
        generation: 7,
        route_ids: &other_route_ids,
        votes: &other_votes,
    };
    let mut ledger = MsmBusLedger::<2, 4, 20>::new(0x5355_4e46, 7, [90, 91], &routes).unwrap();
    assert_eq!(
        ledger.aggregate([
            90, 10_065, 10_064, 10_073, 135, 770, 129, 512, 585, 1, 10_071, 161, 10_036, 10_035,
            676, 143, 777,
        ]),
        Err(qcom_msm_bus_aggregate::Error::MissingClient)
    );
    ledger.update(ipa_client).unwrap();
    ledger.update(other_client).unwrap();
    let node_totals = ledger
        .aggregate([
            90, 10_065, 10_064, 10_073, 135, 770, 129, 512, 585, 1, 10_071, 161, 10_036, 10_035,
            676, 143, 777,
        ])
        .unwrap();

    assert_eq!(
        node_totals[1].contexts,
        Contexts {
            active_only: Bandwidth::default(),
            dual: Bandwidth {
                average: 170_000_000,
                peak: 465_000_000,
            },
        }
    );
}

#[test]
fn complete_client_snapshot_composes_ipa_svs2_through_bcm_and_tcs_planning() {
    use qcom_bcm::Metadata;
    use qcom_bcm_vote::{RpmhRequestState, Vector, Vectors};
    use qcom_msm_bus_aggregate::{
        BcmBinding, ClientReport, NodeGeometry, Route, RouteNodeVote, Snapshot,
    };
    use qcom_rpmh_handoff::{Bank, Snapshot as RscSnapshot, OBSERVED_CONFIG};

    const IDENTITY: u64 = 0x5355_4e46;
    const GENERATION: u64 = 17;

    let route_ids = [10, 11, 12, 13];
    let ipa_routes = sunfish_path_routes(route_ids);
    let other_nodes = [SLAVE_EBI_CH0];
    let routes = [
        ipa_routes[0].route,
        ipa_routes[1].route,
        ipa_routes[2].route,
        ipa_routes[3].route,
        Route {
            id: 20,
            nodes: &other_nodes,
        },
    ];
    let old_ipa_contribution = client_contribution::<20>(disabled_use_case(), ipa_routes).unwrap();
    let old_ipa = old_ipa_contribution
        .report(10, IDENTITY, GENERATION)
        .unwrap();
    let other_contexts = qcom_bcm_vote::Contexts {
        active_only: qcom_bcm_vote::Bandwidth::default(),
        dual: qcom_bcm_vote::Bandwidth {
            average: 10_000_000,
            peak: 400_000_000,
        },
    };
    let other_votes = [RouteNodeVote {
        route_id: 20,
        node_id: SLAVE_EBI_CH0,
        contexts: other_contexts,
    }];
    let clients = [
        old_ipa,
        ClientReport {
            client_id: 91,
            identity: IDENTITY,
            generation: GENERATION,
            route_ids: &[20],
            votes: &other_votes,
        },
    ];
    // Handle 10 is the IPA handle observed in the matched Lineage capture.
    let expected_clients = [10, 91];
    let snapshot = Snapshot {
        identity: IDENTITY,
        generation: GENERATION,
        expected_clients: &expected_clients,
        routes: &routes,
        clients: &clients,
    };

    let node_ids = [
        90, 10_065, 10_064, 10_073, 135, 770, 129, 512, 585, 1, 10_071, 161, 10_036, 10_035, 676,
        143, 777,
    ];
    let metadata = Metadata {
        id: *b"MC0\0\0\0\0\0",
        address: 0x50000,
        unit_size: 1_770_000,
        width: 16,
        clock_domain: 0,
        reserved: 0,
        auxiliary_version: 0x1000,
    };
    let mc0_links = [NodeGeometry {
        node_id: SLAVE_EBI_CH0,
        bus_width: 4,
        aggregation_ports: 2,
    }];
    let binding = BcmBinding {
        metadata,
        links: &mc0_links,
        prior_acv_masks: [0, 0],
    };
    let svs2 = use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, RATE_SVS2_HZ, USE_CASE_COUNT).unwrap();
    let prepared = sunfish_ipa_bus_vote::plan_with_complete_bus_snapshot::<2, 17, 1, 1>(
        snapshot,
        10,
        svs2,
        ipa_routes,
        node_ids,
        [binding],
        false,
        RpmhRequestState::ActiveOnly,
    )
    .unwrap();

    assert_eq!(
        (prepared.use_case_index, prepared.use_case_name),
        (1, "SVS2")
    );
    assert_eq!(
        prepared.bcm_requests[0].vectors,
        Vectors {
            active: Vector { x: 101, y: 1_050 },
            sleep: Vector { x: 101, y: 1_050 },
        }
    );
    assert_eq!(prepared.tcs.active.len, 1);
    assert_eq!(prepared.tcs.active.commands[0].address, 0x50000);
    assert!(prepared.tcs.active.commands[0].complete);
    assert_eq!(prepared.tcs.wake.len, 0);
    assert_eq!(prepared.tcs.sleep.len, 0);

    let mut rsc_snapshot = RscSnapshot {
        config: OBSERVED_CONFIG,
        irq_enable: 1,
        ..RscSnapshot::default()
    };
    rsc_snapshot.banks[0] = Bank {
        idle: 1,
        ..Bank::default()
    };
    rsc_snapshot.banks[1] = Bank {
        idle: 1,
        ..Bank::default()
    };
    let active_tcs = sunfish_ipa_bus_vote::plan_active_rsc_tcs_batch(
        &prepared,
        rsc_snapshot,
        IDENTITY,
        GENERATION,
        41,
        0,
    )
    .unwrap();
    assert_eq!(active_tcs.identity(), IDENTITY);
    assert_eq!(active_tcs.generation(), GENERATION);
    assert_eq!(active_tcs.ticket(), 41);
    assert_eq!(active_tcs.command_count(), 1);
    assert_eq!(
        active_tcs.write(1).unwrap().register(),
        qcom_rpmh_vrm_tcs::Register::CommandAddress(0)
    );
    assert_eq!(active_tcs.write(1).unwrap().value(), 0x50000);
    assert_eq!(
        sunfish_ipa_bus_vote::plan_active_rsc_tcs_batch(
            &prepared,
            rsc_snapshot,
            IDENTITY,
            GENERATION,
            42,
            1,
        ),
        Err(Error::TcsBatchIndex)
    );
    let mut malformed_plan = prepared;
    malformed_plan.tcs.active.batches.lengths[0] = 2;
    assert_eq!(
        sunfish_ipa_bus_vote::plan_active_rsc_tcs_batch(
            &malformed_plan,
            rsc_snapshot,
            IDENTITY,
            GENERATION,
            43,
            0,
        ),
        Err(Error::TcsBatchLayout)
    );

    let incomplete = Snapshot {
        clients: &clients[..1],
        ..snapshot
    };
    assert_eq!(
        sunfish_ipa_bus_vote::plan_with_complete_bus_snapshot::<2, 17, 1, 1>(
            incomplete,
            10,
            svs2,
            ipa_routes,
            node_ids,
            [binding],
            false,
            RpmhRequestState::ActiveOnly,
        ),
        Err(Error::CompleteBusSet)
    );
}

#[test]
fn ipa_route_contribution_rejects_mismatched_endpoints_and_capacity() {
    use qcom_msm_bus_aggregate::Route;

    let nodes = [1, 2];
    let routes = [
        Route {
            id: 10,
            nodes: &nodes,
        },
        Route {
            id: 11,
            nodes: &nodes,
        },
        Route {
            id: 12,
            nodes: &nodes,
        },
        Route {
            id: 13,
            nodes: &nodes,
        },
    ];
    let path_routes = [
        PathRoute {
            master: MASTER_IPA,
            slave: SLAVE_EBI_CH0,
            route: routes[0],
        },
        PathRoute {
            master: MASTER_IPA,
            slave: SLAVE_OCIMEM,
            route: routes[1],
        },
        PathRoute {
            master: MASTER_AMPSS_M0,
            slave: SLAVE_IPA_CFG,
            route: routes[2],
        },
        PathRoute {
            master: MASTER_IPA_CORE,
            slave: SLAVE_IPA_CORE,
            route: routes[3],
        },
    ];
    let use_case =
        use_case_for_clock_rate(SUNFISH_IPA_HW_V4_2, boot_clock_rate_hz(), USE_CASE_COUNT).unwrap();

    let mut wrong_endpoint = path_routes;
    wrong_endpoint[2].slave = SLAVE_OCIMEM;
    assert_eq!(
        client_contribution::<8>(&use_case, wrong_endpoint),
        Err(Error::PathEndpoint)
    );
    assert_eq!(
        client_contribution::<7>(&use_case, path_routes),
        Err(Error::VoteCapacity)
    );
}
