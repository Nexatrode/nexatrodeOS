#!/usr/bin/env python3
"""Validate Sunfish bus routes/metadata from DT; never issue votes."""
from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

ROOT = "/soc/ad-hoc-bus"
PINNED_KERNEL_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_KERNEL_SOURCE = Path(__file__).resolve().parents[2] / ".cache/sunfish-lineage-kernel"
# IDs and widths are from the final live-derived DT, not common-DTS defaults.
NODES = {
    # IPA v4.2's four `msm_bus` vectors, resolved through the final live DT.
    # Expected routes are selected with the pinned arbiter's ordered BFS from
    # the final live DT. This validates route selection, not bus voting.
    "mas-qxm-ipa": (90, 8, 1, []),
    "slv-qns-a2noc-snoc": (10065, 16, 1, ["SN11"]),
    "mas-qnm-aggre2-noc": (10064, 16, 1, ["SN11"]),
    "slv-qns-gemnoc-sf": (10073, 16, 1, ["SN0"]),
    "mas-qnm-snoc-sf": (135, 16, 1, []),
    "slv-qns-llcc": (770, 16, 2, ["SH0"]),
    "slv-qxs-imem": (585, 8, 1, ["SN1"]),
    "mas-acm-apps": (1, 16, 1, ["SH5"]),
    "slv-qns-gem-noc-snoc": (10071, 8, 1, ["SH2"]),
    "mas-qnm-gemnoc": (161, 8, 1, ["SN15"]),
    "slv-qns-cnoc": (10036, 8, 1, []),
    "mas-qnm-snoc": (10035, 8, 1, ["CN0"]),
    "slv-qhs-ipa": (676, 4, 1, ["CN0"]),
    "mas-ipa-core-master": (143, 8, 1, []),
    "slv-ipa-core-slave": (777, 8, 1, ["IP0"]),
    "mas-llcc-mc": (129, 4, 2, []),
    "slv-ebi": (512, 4, 2, ["MC0", "ACV"]),
    "mas-qxm-crypto": (125, 8, 1, ["CE0"]),
}
ROUTES = {
    "ipa_ebi": ["mas-qxm-ipa", "slv-qns-a2noc-snoc", "mas-qnm-aggre2-noc",
                "slv-qns-gemnoc-sf", "mas-qnm-snoc-sf", "slv-qns-llcc",
                "mas-llcc-mc", "slv-ebi"],
    "ipa_ocimem": ["mas-qxm-ipa", "slv-qns-a2noc-snoc", "mas-qnm-aggre2-noc",
                   "slv-qxs-imem"],
    "ipa_cfg": ["mas-acm-apps", "slv-qns-gem-noc-snoc", "mas-qnm-gemnoc",
                "slv-qns-cnoc", "mas-qnm-snoc", "slv-qhs-ipa"],
    "ipa_core": ["mas-ipa-core-master", "slv-ipa-core-slave"],
    "modem_proxy": ["mas-llcc-mc", "slv-ebi"],
    "pas_crypto": ["mas-qxm-crypto", "slv-qns-a2noc-snoc", "mas-qnm-aggre2-noc",
                   "slv-qns-gemnoc-sf", "mas-qnm-snoc-sf", "slv-qns-llcc",
                   "mas-llcc-mc", "slv-ebi"],
}


class Tree:
    def __init__(self, dtb: Path):
        self.dtb = dtb
        self._bus_graph = None

    def raw(self, node, prop=None, kind="x"):
        args = ["fdtget", "-t", kind, str(self.dtb), node, prop] if prop else ["fdtget", "-p", str(self.dtb), node]
        return subprocess.check_output(args, text=True).strip()

    def cells(self, node, prop):
        if prop not in self.raw(node).split():
            return []
        return [int(value, 16) for value in self.raw(node, prop).split()]

    def optional_cells(self, node, prop):
        result = subprocess.run(
            ["fdtget", "-t", "x", str(self.dtb), node, prop],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode:
            return []
        return [int(value, 16) for value in result.stdout.split()]

    def children(self, node):
        return subprocess.check_output(
            ["fdtget", "-l", str(self.dtb), node], text=True
        ).split()

    def bus_graph(self):
        if self._bus_graph is not None:
            return self._bus_graph
        names = self.children(ROOT)
        by_handle = {}
        by_id = {}
        for name in names:
            node = ROOT + "/" + name
            handles = self.optional_cells(node, "phandle")
            node_ids = self.optional_cells(node, "cell-id")
            if len(handles) == 1:
                by_handle[handles[0]] = name
            if len(node_ids) == 1:
                by_id.setdefault(node_ids[0], []).append(name)
        connections = {}
        blacklists = {}
        for name in names:
            node = ROOT + "/" + name
            connections[name] = []
            blacklists[name] = set()
            for prop, destination_list in (
                ("qcom,connections", connections[name]),
                ("qcom,blacklist", blacklists[name]),
            ):
                for handle in self.optional_cells(node, prop):
                    if handle not in by_handle:
                        raise ValueError(f"{name}: unresolved {prop} phandle")
                    if isinstance(destination_list, list):
                        destination_list.append(by_handle[handle])
                    else:
                        destination_list.add(by_handle[handle])
        self._bus_graph = (by_id, connections, blacklists)
        return self._bus_graph

    def selected_route(self, source_id, destination_id):
        """Mirror the pinned arbiter's ordered breadth-first getpath search."""
        by_id, connections, blacklists = self.bus_graph()

        def unique_id(value):
            matches = by_id.get(value, [])
            if len(matches) != 1:
                raise ValueError(f"bus cell-id {value} is missing or ambiguous")
            return matches[0]

        source = unique_id(source_id)
        destination = unique_id(destination_id)

        frontier = [source]
        parent = {}
        visited = {source}
        blocked = set()
        while frontier:
            if destination in frontier:
                path = [destination]
                while path[-1] != source:
                    path.append(parent[path[-1]])
                return list(reversed(path))
            # setup_bl_list() appends each frontier's entries to one list that
            # stays live until prune_path(), so earlier-depth blacklists remain
            # active as the BFS expands.
            for name in frontier:
                blocked.update(blacklists[name])
            next_frontier = []
            for name in frontier:
                for connected in connections[name]:
                    if connected in blocked:
                        continue
                    if connected in visited:
                        if connections[connected]:
                            raise ValueError(
                                f"msm_bus getpath hits circular route through {connected}"
                            )
                        continue
                    visited.add(connected)
                    parent[connected] = name
                    next_frontier.append(connected)
            frontier = next_frontier
        return None


def validate_kernel_source(source):
    if not source.is_dir():
        raise ValueError(f"pinned msm_bus source tree not found: {source}")
    commit = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    if commit != PINNED_KERNEL_COMMIT:
        raise ValueError(f"expected msm_bus source {PINNED_KERNEL_COMMIT}, got {commit}")
    for relative in (
        "drivers/soc/qcom/msm_bus/msm_bus_arb_rpmh.c",
        "drivers/soc/qcom/msm_bus/msm_bus_fabric_rpmh.c",
    ):
        result = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", relative],
            check=False,
        )
        if result.returncode:
            raise ValueError(f"pinned msm_bus source has local edits: {relative}")
    return commit


def validate(tree, kernel_commit):
    handles = {name: tree.cells(ROOT + "/" + name, "phandle") for name in NODES}
    if any(len(value) != 1 for value in handles.values()):
        raise ValueError("missing/non-scalar path phandle")
    app_handle = tree.cells(ROOT + "/rsc-apps", "phandle")
    if len(app_handle) != 1:
        raise ValueError("missing Apps RSC phandle")
    result = {}
    for name, (node_id, width, ports, bcms) in NODES.items():
        node = ROOT + "/" + name
        for prop, expected in [("cell-id", node_id), ("qcom,buswidth", width), ("qcom,agg-ports", ports)]:
            if tree.cells(node, prop) != [expected]:
                raise ValueError(f"{name}: changed {prop}")
        if tree.cells(node, "qcom,blacklist"):
            raise ValueError(f"{name}: unreviewed blacklist")
        bcm_handles = []
        for bcm in bcms:
            bcm_node = ROOT + "/bcm-" + bcm.lower()
            if tree.raw(bcm_node, "qcom,bcm-name", "s") != bcm:
                raise ValueError(f"{bcm}: changed command DB name")
            if tree.cells(bcm_node, "qcom,rscs") != app_handle:
                raise ValueError(f"{bcm}: unexpected RSC owner")
            handle = tree.cells(bcm_node, "phandle")
            if len(handle) != 1:
                raise ValueError(f"{bcm}: missing phandle")
            bcm_handles += handle
        if tree.cells(node, "qcom,bcms") != bcm_handles:
            raise ValueError(f"{name}: changed BCM bindings")
    for label, route in ROUTES.items():
        for source, dest in zip(route, route[1:]):
            if handles[dest][0] not in tree.cells(ROOT + "/" + source, "qcom,connections"):
                raise ValueError(f"{label}: missing {source}->{dest}")
        selected = tree.selected_route(NODES[route[0]][0], NODES[route[-1]][0])
        if selected != route:
            raise ValueError(f"{label}: kernel-selected route changed to {selected}")
        result[label] = [{"node": name, "id": NODES[name][0], "buswidth": NODES[name][1],
                          "aggregation_ports": NODES[name][2], "bcms": NODES[name][3]} for name in route]
    vectors = tree.cells("/soc/qcom,mss@4080000", "qcom,msm-bus,vectors-KBps")
    if vectors != [129, 512, 0, 0, 129, 512, 0, 7000000]:
        raise ValueError("changed final modem bandwidth vectors")
    override = tree.cells("/soc/qcom,mss@4080000", "qcom,mas-crypto")
    if override and override != handles["mas-qxm-crypto"]:
        raise ValueError("unreviewed PAS crypto master override")
    return {"kernel_commit": kernel_commit,
            "scope": "Source-matched route selection and DT metadata only; no QoS, aggregation, votes, or ownership implemented",
            "modem_vectors_kbps": [vectors[:4], vectors[4:]], "routes": result,
            "metadata_names": ["MC0", "ACV", "CE0", "SN11", "SN0", "SH0",
                               "SN1", "SH5", "SH2", "SN15", "CN0", "IP0"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dtb", type=Path)
    parser.add_argument("--kernel-source", type=Path, default=DEFAULT_KERNEL_SOURCE)
    args = parser.parse_args()
    commit = validate_kernel_source(args.kernel_source)
    print(json.dumps(validate(Tree(args.dtb), commit), indent=2))


if __name__ == "__main__":
    main()
