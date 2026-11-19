#!/usr/bin/env python3
"""Host-only checks for the Sunfish MSM-bus DT inventory parser."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "platforms" / "sunfish" / "audit-msm-bus-client-inventory.py"
SPEC = importlib.util.spec_from_file_location("audit_msm_bus_client_inventory", MODULE_PATH)
assert SPEC and SPEC.loader
INVENTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INVENTORY)


class MsmBusInventoryParserTests(unittest.TestCase):
    def test_devbw_opp_unit_interpretation_is_pinned_to_driver_and_dts(self) -> None:
        semantics = INVENTORY.validate_devbw_value_semantics(
            INVENTORY.DEFAULT_KERNEL_SOURCE
        )
        self.assertEqual(semantics["selected_opp_argument"], "new_ib")
        self.assertIn("MiB/s", semantics["ib_units"])
        self.assertEqual(
            semantics["zero_average_governors"], ["performance", "powersave"]
        )
        self.assertEqual(semantics["dynamic_average_governors"], ["bw_vbif"])
        gpu_ab = semantics["dynamic_average_model"]
        self.assertIn("ram_time", gpu_ab["inputs"])
        self.assertIn("total_time", gpu_ab["inputs"])
        self.assertIn("bus_width_bytes", gpu_ab["formula"])
        self.assertIn(", 160)", gpu_ab["formula"])
        self.assertIn("devfreq_vbif_update_bw", gpu_ab["handoff"])
        self.assertIn("BW MBps: AB:", gpu_ab["exact_runtime_trace"])

    def test_devbw_rows_retain_governor_and_source_encoded_opp_bandwidths(self) -> None:
        tree = INVENTORY.DtsTree.from_text("""\
/dts-v1/;
/ {
    soc {
        ad-hoc-bus {
            source {
                phandle = <1>;
                cell-id = <1>;
                qcom,connections = <2>;
            };
            destination {
                phandle = <2>;
                cell-id = <2>;
            };
        };
        bw-opp-table {
            phandle = <100>;
            opp-0 {
                opp-hz = /bits/ 64 <0 0>;
            };
            opp-300 {
                opp-hz = /bits/ 64 <0 1144>;
            };
        };
        qcom,cpu-llcc-ddr-bw {
            compatible = "qcom,devbw";
            qcom,src-dst-ports = <1 2>;
            operating-points-v2 = <100>;
        };
    };
};
""")

        clients, unresolved = INVENTORY.devbw_inventory(tree, {})

        self.assertEqual(unresolved, [])
        self.assertEqual(len(clients), 1)
        client = clients[0]
        self.assertEqual(client["governor"], "performance")
        self.assertEqual(client["governor_source"], "devfreq_devbw.c default")
        self.assertEqual(
            [entry["request_ib_mibps"] for entry in client["operating_points_v2"]["entries"]],
            [0, 1144],
        )
        self.assertEqual(client["operating_points_v2"]["entries"][1]["opp_hz"], 1144)
        self.assertEqual(client["paths"][0]["route"], ["source", "destination"])

    def test_devbw_inventory_rejects_unresolved_opp_table(self) -> None:
        tree = INVENTORY.DtsTree.from_text("""\
/dts-v1/;
/ {
    soc {
        ad-hoc-bus {
            source {
                phandle = <1>;
                cell-id = <1>;
                qcom,connections = <2>;
            };
            destination {
                phandle = <2>;
                cell-id = <2>;
            };
        };
        qcom,missing-opp-table {
            compatible = "qcom,devbw";
            qcom,src-dst-ports = <1 2>;
            operating-points-v2 = <100>;
        };
    };
};
""")

        with self.assertRaisesRegex(ValueError, "unresolved operating-points-v2"):
            INVENTORY.devbw_inventory(tree, {})

    def test_indexes_nodes_with_multiple_dtc_labels_under_their_real_parent(self) -> None:
        tree = INVENTORY.DtsTree.from_text("""\
/dts-v1/;
/ {
    soc: soc {
        devbw_alias: bw_alias: qcom,cpu-cpu-llcc-bw {
            compatible = "qcom,devbw";
            qcom,src-dst-ports = <1 770>;
        };
    };
};
""")
        self.assertIn("/soc/qcom,cpu-cpu-llcc-bw", tree.nodes)
        self.assertNotIn("/qcom,cpu-cpu-llcc-bw", tree.nodes)
        self.assertEqual(
            tree.raw("/soc/qcom,cpu-cpu-llcc-bw", "compatible"),
            "qcom,devbw",
        )
        self.assertEqual(
            tree.cells("/soc/qcom,cpu-cpu-llcc-bw", "qcom,src-dst-ports"),
            [1, 770],
        )

    def test_exports_ordered_bcm_bindings_with_rsc_identity_and_source_defaults(self) -> None:
        tree = INVENTORY.DtsTree.from_text("""\
/dts-v1/;
/ {
    soc: soc {
        ad_hoc_bus: ad-hoc-bus {
            apps_rsc: rsc-apps {
                phandle = <100>;
                cell-id = <8000>;
            };
            display_rsc: rsc-display {
                phandle = <101>;
                cell-id = <8001>;
            };
            apps_mc0: bcm-mc0 {
                phandle = <200>;
                cell-id = <7000>;
                qcom,bcm-name = "MC0";
                qcom,rscs = <100>;
            };
            display_mc0: bcm-mc0-display {
                phandle = <201>;
                cell-id = <27000>;
                qcom,bcm-name = "MC0";
                qcom,rscs = <101>;
            };
            fabric: fab-system {
                phandle = <300>;
                cell-id = <6000>;
            };
            parent_only: fab-parent-only {
                phandle = <301>;
                cell-id = <6001>;
            };
            source: source-node {
                phandle = <400>;
                cell-id = <1>;
                qcom,qport = <1 2>;
                qcom,bus-dev = <301>;
                qcom,bcms = <201 200>;
            };
            destination: destination-node {
                phandle = <500>;
                cell-id = <2>;
                qcom,bus-dev = <301>;
            };
        };
    };
};
""")

        topology = INVENTORY.fabric_topology(
            tree,
            [{"cases": [{"paths": [{"route": ["source-node", "fab-system", "destination-node"]}]}]}],
            [],
        )

        self.assertEqual(topology["summary"]["bcm_device_nodes"], 2)
        self.assertEqual(topology["summary"]["unique_cmd_db_names"], 1)
        self.assertEqual(topology["summary"]["client_routed_cmd_db_names"], 1)
        self.assertEqual(
            [row["node"] for row in topology["bcm_devices"]],
            ["bcm-mc0", "bcm-mc0-display"],
        )
        source = next(row for row in topology["bus_nodes"] if row["node"] == "source-node")
        parent_only = next(row for row in topology["bus_nodes"]
                           if row["node"] == "fab-parent-only")
        self.assertEqual(parent_only["bcm_bindings"], [])
        self.assertEqual(source["buswidth"], 8)
        self.assertEqual(source["buswidth_source"], "msm_bus_of_rpmh.c default 8")
        self.assertEqual(source["num_qports"], 2)
        self.assertEqual(source["aggregation_ports"], 2)
        self.assertEqual(
            [(row["node"], row["commit_rsc"]["node"]) for row in source["bcm_bindings"]],
            [("bcm-mc0-display", "rsc-display"), ("bcm-mc0", "rsc-apps")],
        )


if __name__ == "__main__":
    unittest.main()
