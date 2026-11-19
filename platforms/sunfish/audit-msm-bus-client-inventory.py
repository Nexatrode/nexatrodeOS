#!/usr/bin/env python3
"""Audit DT-declared MSM bus clients and their source-selected BCM routes.

This is a static device-tree inventory. It does not claim which clients have
registered at runtime, which usecase is active, or which BCM votes are live.
It never issues a bus vote.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import re
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROUTES_MODULE_PATH = HERE / "validate-bcm-routes.py"
SPEC = importlib.util.spec_from_file_location("validate_bcm_routes", ROUTES_MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load route validator: {ROUTES_MODULE_PATH}")
ROUTES_MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ROUTES_MODULE)
ROOT = ROUTES_MODULE.ROOT
DEFAULT_KERNEL_SOURCE = ROUTES_MODULE.DEFAULT_KERNEL_SOURCE
KNOWN_ROUTES = ROUTES_MODULE.ROUTES
KNOWN_NODES = ROUTES_MODULE.NODES


class DtsTree:
    """Small read-only DTS index, parsed once to avoid per-property fdtget calls."""

    def __init__(self, dtb: Path):
        source = subprocess.check_output(
            ["dtc", "-I", "dtb", "-O", "dts", str(dtb)], text=True,
            stderr=subprocess.DEVNULL,
        )
        self._index(source)

    @classmethod
    def from_text(cls, source: str) -> "DtsTree":
        tree = cls.__new__(cls)
        tree._index(source)
        return tree

    def _index(self, source: str) -> None:
        self.nodes: dict[str, dict[str, str]] = {"/": {}}
        stack = ["/"]
        for line in source.splitlines():
            text = line.strip()
            if not text or text.startswith("//") or text.startswith("/*"):
                continue
            if text == "/ {":
                continue
            if text == "};":
                if len(stack) > 1:
                    stack.pop()
                continue
            # dtc emits every label attached to an overlaid node, e.g.
            # `a: b: c: qcom,device {`. Accept all labels so its closing `};`
            # cannot accidentally pop the parent when the node line is skipped.
            match = re.fullmatch(r"(?:[\w-]+:\s*)*([^\s{]+)\s*\{", text)
            if match:
                parent = stack[-1].rstrip("/")
                path = parent + "/" + match.group(1)
                self.nodes[path] = {}
                stack.append(path)
                continue
            if "=" in text and text.endswith(";"):
                prop = text.split("=", 1)[0].strip()
                self.nodes[stack[-1]][prop] = text.split("=", 1)[1][:-1].strip()
            elif text.endswith(";"):
                self.nodes[stack[-1]][text[:-1].strip()] = ""

    def children(self, node: str) -> list[str]:
        prefix = node.rstrip("/") + "/"
        result = []
        for path in self.nodes:
            if not path.startswith(prefix):
                continue
            suffix = path[len(prefix):]
            if "/" not in suffix:
                result.append(suffix)
        return result

    def raw(self, node: str, prop: str) -> str:
        value = self.nodes[node][prop]
        match = re.search(r'"([^"\\]*(?:\\.[^"\\]*)*)"', value)
        if match:
            return match.group(1)
        return value

    def cells(self, node: str, prop: str) -> list[int]:
        value = self.nodes.get(node, {}).get(prop, "")
        return [int(token, 0) for group in re.findall(r"<([^>]*)>", value)
                for token in group.split()]

    def selected_route(self, source_id: int, destination_id: int):
        names = self.children(ROOT)
        by_handle = {}
        by_id = {}
        for name in names:
            node = ROOT + "/" + name
            handles = self.cells(node, "phandle")
            ids = self.cells(node, "cell-id")
            if len(handles) == 1:
                by_handle[handles[0]] = name
            if len(ids) == 1:
                by_id.setdefault(ids[0], []).append(name)

        def unique(value):
            found = by_id.get(value, [])
            if len(found) != 1:
                raise ValueError(f"bus cell-id {value} is missing or ambiguous")
            return found[0]

        source, destination = unique(source_id), unique(destination_id)
        connections = {}
        blacklists = {}
        for name in names:
            node = ROOT + "/" + name
            connections[name] = []
            for handle in self.cells(node, "qcom,connections"):
                if handle not in by_handle:
                    raise ValueError(f"{name}: unresolved qcom,connections phandle")
                connections[name].append(by_handle[handle])
            blacklists[name] = set()
            for handle in self.cells(node, "qcom,blacklist"):
                if handle not in by_handle:
                    raise ValueError(f"{name}: unresolved qcom,blacklist phandle")
                blacklists[name].add(by_handle[handle])

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


def bcm_names(tree) -> dict[int, str]:
    result = {}
    for child in tree.children(ROOT):
        if not child.startswith("bcm-"):
            continue
        path = ROOT + "/" + child
        handle = tree.cells(path, "phandle")
        name = tree.raw(path, "qcom,bcm-name")
        if len(handle) != 1:
            raise ValueError(f"{path}: missing/non-scalar phandle")
        result[handle[0]] = name
    return result


def devbw_opp_table(tree: DtsTree, node: str, phandle_paths: dict[int, str]) -> dict:
    """Resolve the OPP values used by `devfreq_devbw` for one dynamic client."""
    handles = tree.cells(node, "operating-points-v2")
    if len(handles) != 1:
        raise ValueError(f"{node}: expected one operating-points-v2 phandle")
    table_path = phandle_paths.get(handles[0])
    if table_path is None:
        raise ValueError(f"{node}: unresolved operating-points-v2 phandle")

    entries = []
    for child in tree.children(table_path):
        opp_node = table_path.rstrip("/") + "/" + child
        if "opp-hz" not in tree.nodes[opp_node]:
            continue
        cells = tree.cells(opp_node, "opp-hz")
        if len(cells) == 1:
            encoded_value = cells[0]
        elif len(cells) == 2:
            encoded_value = (cells[0] << 32) | cells[1]
        else:
            raise ValueError(f"{opp_node}: malformed opp-hz value")
        entries.append({
            "node": child,
            "opp_hz": encoded_value,
            # devfreq_devbw passes the selected OPP value directly as new_ib,
            # then multiplies it by 1 MiB in set_bw(). Sunfish's BW_OPP_ENTRY
            # macro encodes this field as bandwidth in MiB/s, despite the
            # generic OPP property name.
            "request_ib_mibps": encoded_value,
            "status": tree.raw(opp_node, "status") if "status" in tree.nodes[opp_node] else "okay (default)",
        })
    if not entries:
        raise ValueError(f"{table_path}: no opp-hz entries")
    return {"node": table_path, "entries": entries}


def validate_devbw_value_semantics(kernel_source: Path) -> dict:
    """Pin the OPP-to-bandwidth interpretation to the selected kernel source."""
    driver_path = kernel_source / "drivers/devfreq/devfreq_devbw.c"
    dts_path = kernel_source / "arch/arm64/boot/dts/qcom/atoll.dtsi"
    performance_path = kernel_source / "drivers/devfreq/governor_performance.c"
    powersave_path = kernel_source / "drivers/devfreq/governor_powersave.c"
    vbif_path = kernel_source / "drivers/devfreq/governor_bw_vbif.c"
    driver = driver_path.read_text()
    dts = dts_path.read_text()
    performance = performance_path.read_text()
    powersave = powersave_path.read_text()
    vbif = vbif_path.read_text()
    gpubw_path = kernel_source / "drivers/devfreq/governor_gpubw_mon.c"
    kgsl_pwrctrl_path = kernel_source / "drivers/gpu/msm/kgsl_pwrctrl.c"
    kgsl_pwrscale_path = kernel_source / "drivers/gpu/msm/kgsl_pwrscale.c"
    gpubw = gpubw_path.read_text()
    kgsl_pwrctrl = kgsl_pwrctrl_path.read_text()
    kgsl_pwrscale = kgsl_pwrscale_path.read_text()
    required_driver = (
        "#define MBYTE (1ULL << 20)",
        "d = devm_kzalloc(dev, sizeof(*d), GFP_KERNEL);",
        "d->bw_levels[i].vectors[0].ib = new_ib * MBYTE;",
        "return set_bw(dev, *freq, d->gov_ab);",
        "stat->private_data = &d->gov_ab;",
    )
    required_dts = (
        "#define MHZ_TO_MBPS(mhz, w) ((mhz * 1000000 * w) / (1024 * 1024))",
        "#define BW_OPP_ENTRY(mhz, w) opp-mhz {opp-hz = /bits/ 64 <MHZ_TO_MBPS(mhz, w)>;}",
    )
    if any(fragment not in driver for fragment in required_driver):
        raise ValueError(f"{driver_path}: devbw bandwidth semantics changed")
    if any(fragment not in dts for fragment in required_dts):
        raise ValueError(f"{dts_path}: BW_OPP_ENTRY semantics changed")
    if "*freq = df->max_freq;" not in performance or "private_data" in performance:
        raise ValueError(f"{performance_path}: devbw average-vote behavior changed")
    if "*freq = df->min_freq;" not in powersave or "private_data" in powersave:
        raise ValueError(f"{powersave_path}: devbw average-vote behavior changed")
    if "dev_ab = stat.private_data;" not in vbif or "*dev_ab = ab;" not in vbif:
        raise ValueError(f"{vbif_path}: dynamic devbw average-vote behavior changed")
    required_gpubw = (
        "#define BW_STEP                 160",
        "norm_ab =  (unsigned int)priv->bus.ram_time /",
        "(unsigned int) priv->bus.total_time;",
        "ab_mbytes = (norm_ab * priv->bus.width * 1000000ULL) >> 20;",
        "bus_profile->ab_mbytes = roundup(ab_mbytes, BW_STEP);",
    )
    if any(fragment not in gpubw for fragment in required_gpubw):
        raise ValueError(f"{gpubw_path}: GPU average-bandwidth formula changed")
    required_kgsl = (
        "data->bus.width = pwr->bus_width;",
        'of_property_read_u32(pdev->dev.of_node, "qcom,bus-width",',
        "*ab = pwr->bus_ab_mbytes;",
        "devfreq_vbif_update_bw(ib_votes[last_vote_buslevel], ab);",
        "kgsl_pwrctrl_vbif_update(ab);",
    )
    if any(fragment not in kgsl_pwrscale + kgsl_pwrctrl for fragment in required_kgsl):
        raise ValueError("KGSL bus-width/AB handoff to bw_vbif changed")
    set_bw_log = "dev_dbg(dev, \"BW MBps: AB: %d IB: %d\\n\", new_ab, new_ib);"
    if set_bw_log not in driver:
        raise ValueError(f"{driver_path}: devbw runtime AB/IB trace changed")
    return {
        "driver": "drivers/devfreq/devfreq_devbw.c",
        "dts": "arch/arm64/boot/dts/qcom/atoll.dtsi",
        "selected_opp_argument": "new_ib",
        "ib_units": "MiB/s (multiplied by 1 MiB before bus submission)",
        "zero_average_governors": ["performance", "powersave"],
        "zero_average_source": "devm_kzalloc initializes gov_ab to zero; these governors do not write private_data",
        "dynamic_average_governors": ["bw_vbif"],
        "dynamic_average_source": "governor_bw_vbif.c updates gov_ab through private_data; value is not exposed by sampled sysfs attributes",
        "dynamic_average_model": {
            "producer": "drivers/devfreq/governor_gpubw_mon.c",
            "inputs": "VBIF ram_time and total_time counters plus KGSL qcom,bus-width",
            "formula": "roundup(((u32(ram_time) / u32(total_time)) * bus_width_bytes * 1000000) >> 20, 160) MiB/s",
            "handoff": "KGSL bus_ab_mbytes -> devfreq_vbif_update_bw(ib, ab) -> devbw gov_ab",
            "exact_runtime_trace": "devbw <device-name>: BW MBps: AB: <new_ab> IB: <new_ib>, emitted only when the request pair changes",
            "sysfs_limitation": "the sampled devfreq attributes expose selected IB state, not VBIF counters or the computed average request",
        },
    }


def devbw_inventory(tree: DtsTree, buses: dict[int, str]) -> tuple[list[dict], list[dict]]:
    """Resolve qcom,devbw dynamic clients and return rows plus route failures."""
    clients = []
    unresolved = []
    phandle_paths: dict[int, str] = {}
    for path, props in tree.nodes.items():
        handles = tree.cells(path, "phandle")
        if len(handles) > 1:
            raise ValueError(f"{path}: multiple phandles")
        if handles:
            if handles[0] in phandle_paths:
                raise ValueError(f"duplicate phandle 0x{handles[0]:x}")
            phandle_paths[handles[0]] = path

    for node, props in tree.nodes.items():
        compatible = re.findall(r'"([^"\\]*(?:\\.[^"\\]*)*)"', props.get("compatible", ""))
        if "qcom,devbw" not in compatible:
            continue
        ports = tree.cells(node, "qcom,src-dst-ports")
        if not ports or len(ports) % 2:
            raise ValueError(f"{node}: malformed qcom,src-dst-ports")

        # devfreq_devbw.c uses dev_name(dev) as its msm-bus client name. OF's
        # platform naming algorithm prefixes each address-less path component;
        # leave it unknown where a translated `reg` address could change it.
        path_parts = node.strip("/").split("/")
        ancestors = ["/" + "/".join(path_parts[:index]) for index in range(1, len(path_parts) + 1)]
        runtime_name = None
        if not any("reg" in tree.nodes.get(path, {}) for path in ancestors):
            runtime_name = ":".join(path_parts)

        paths = []
        for path_index in range(len(ports) // 2):
            src_id, dst_id = ports[path_index * 2 : path_index * 2 + 2]
            row = {"path_index": path_index, "source_id": src_id, "destination_id": dst_id}
            try:
                route = tree.selected_route(src_id, dst_id)
                if route is None:
                    raise ValueError("no route in the pinned arbiter search")
                route_bcms = []
                for bus_node in route:
                    for handle in tree.cells(ROOT + "/" + bus_node, "qcom,bcms"):
                        if handle not in buses:
                            raise ValueError(f"{bus_node}: unknown BCM phandle 0x{handle:x}")
                        if buses[handle] not in route_bcms:
                            route_bcms.append(buses[handle])
                row["route"] = route
                row["bcms"] = route_bcms
            except ValueError as error:
                row["route_error"] = str(error)
                unresolved.append({"node": node, **row})
            paths.append(row)
        clients.append({
            "node": node,
            "runtime_name": runtime_name,
            "runtime_name_basis": "of_device_make_bus_id, address-less path" if runtime_name else "address-based name unresolved",
            "governor": tree.raw(node, "governor") if "governor" in props else "performance",
            "governor_source": "device tree" if "governor" in props else "devfreq_devbw.c default",
            "active_only": "qcom,active-only" in props,
            "status": tree.raw(node, "status") if "status" in props else "okay (default)",
            "operating_points_v2": devbw_opp_table(tree, node, phandle_paths),
            "paths": paths,
        })
    clients.sort(key=lambda row: row["node"])
    return clients, unresolved


def fabric_topology(tree: DtsTree, clients: list[dict], devbw_clients: list[dict]) -> dict:
    """Export the DT order and geometry used by the pinned RPMh bus driver."""
    prefix = ROOT.rstrip("/") + "/"
    children = [
        (path, props)
        for path, props in tree.nodes.items()
        if path.startswith(prefix) and "/" not in path[len(prefix):]
    ]
    phandle_paths: dict[int, str] = {}
    for path, props in tree.nodes.items():
        handles = tree.cells(path, "phandle")
        if len(handles) > 1:
            raise ValueError(f"{path}: multiple phandles")
        if handles:
            if handles[0] in phandle_paths:
                raise ValueError(f"duplicate phandle 0x{handles[0]:x}")
            phandle_paths[handles[0]] = path

    def cell_id(path: str) -> int | None:
        values = tree.cells(path, "cell-id")
        if len(values) > 1:
            raise ValueError(f"{path}: multiple cell-id values")
        return values[0] if values else None

    def resolved_node(handle: int, prop: str) -> dict:
        path = phandle_paths.get(handle)
        if path is None:
            raise ValueError(f"unresolved {prop} phandle 0x{handle:x}")
        return {
            "phandle": handle,
            "node": path.rsplit("/", 1)[-1],
            "path": path,
            "cell_id": cell_id(path),
        }

    bcm_by_handle: dict[int, dict] = {}
    bcm_devices = []
    rsc_handles: list[int] = []
    for order, (path, props) in enumerate(children):
        if "qcom,bcm-name" not in props:
            continue
        handles = tree.cells(path, "phandle")
        ids = tree.cells(path, "cell-id")
        if len(handles) != 1 or len(ids) != 1:
            raise ValueError(f"{path}: BCM node needs one phandle and cell-id")
        rscs = [resolved_node(handle, "qcom,rscs") for handle in tree.cells(path, "qcom,rscs")]
        for rsc in rscs:
            if rsc["phandle"] not in rsc_handles:
                rsc_handles.append(rsc["phandle"])
        row = {
            "device_order": order,
            "node": path.rsplit("/", 1)[-1],
            "path": path,
            "phandle": handles[0],
            "cell_id": ids[0],
            "cmd_db_name": tree.raw(path, "qcom,bcm-name"),
            "rscs": rscs,
            "commit_rsc": rscs[0] if rscs else None,
        }
        bcm_by_handle[handles[0]] = row
        bcm_devices.append(row)

    route_nodes: set[str] = set()
    parent_paths: set[str] = set()
    for path, _props in children:
        for handle in tree.cells(path, "qcom,bus-dev"):
            parent_paths.add(resolved_node(handle, "qcom,bus-dev")["path"])
    for client in clients:
        for case in client.get("cases", []):
            for path_row in case.get("paths", []):
                route_nodes.update(path_row.get("route", []))
    for client in devbw_clients:
        for path_row in client.get("paths", []):
            route_nodes.update(path_row.get("route", []))

    bus_nodes = []
    by_name: dict[str, dict] = {}
    for order, (path, props) in enumerate(children):
        if "qcom,bcm-name" in props or any(
            bcm["path"] == path for bcm in bcm_devices
        ):
            continue
        node_name = path.rsplit("/", 1)[-1]
        # RSC and non-bus helper children are not route nodes. Keep route
        # members and every node that binds to a BCM; the latter set defines
        # the possible BCM link list even when a current client never uses it.
        if node_name not in route_nodes and "qcom,bcms" not in props and path not in parent_paths:
            continue
        ids = tree.cells(path, "cell-id")
        if len(ids) != 1:
            raise ValueError(f"{path}: route/BCM node needs one cell-id")
        qports = tree.cells(path, "qcom,qport")
        widths = tree.cells(path, "qcom,buswidth")
        agg_ports = tree.cells(path, "qcom,agg-ports")
        if len(widths) > 1 or len(agg_ports) > 1:
            raise ValueError(f"{path}: non-scalar aggregation geometry")
        buswidth = widths[0] if widths else 8
        num_aggports = agg_ports[0] if agg_ports else len(qports)
        if "qcom,bcms" in props and (buswidth == 0 or num_aggports == 0):
            raise ValueError(f"{path}: linked BCM node has zero aggregation geometry")

        bus_dev_handles = tree.cells(path, "qcom,bus-dev")
        if len(bus_dev_handles) > 1:
            raise ValueError(f"{path}: multiple qcom,bus-dev entries")
        parent = resolved_node(bus_dev_handles[0], "qcom,bus-dev") if bus_dev_handles else None
        bindings = []
        for index, handle in enumerate(tree.cells(path, "qcom,bcms")):
            resource = bcm_by_handle.get(handle)
            if resource is None:
                raise ValueError(f"{path}: unknown qcom,bcms phandle 0x{handle:x}")
            bindings.append({
                "binding_index": index,
                "phandle": handle,
                "node": resource["node"],
                "cell_id": resource["cell_id"],
                "cmd_db_name": resource["cmd_db_name"],
                "commit_rsc": resource["commit_rsc"],
            })
        row = {
            "device_order": order,
            "node": node_name,
            "path": path,
            "cell_id": ids[0],
            "buswidth": buswidth,
            "buswidth_source": "qcom,buswidth" if widths else "msm_bus_of_rpmh.c default 8",
            "num_qports": len(qports),
            "aggregation_ports": num_aggports,
            "aggregation_ports_source": "qcom,agg-ports" if agg_ports else "num_qports",
            "bus_device": parent,
            "bcm_bindings": bindings,
            "client_route_member": node_name in route_nodes,
        }
        bus_nodes.append(row)
        by_name[node_name] = row

    missing_route_nodes = route_nodes - by_name.keys()
    if missing_route_nodes:
        raise ValueError(f"route nodes missing from exported fabric topology: {sorted(missing_route_nodes)}")

    routed_resources = []
    routed_handles: set[int] = set()
    for node_name in (node for node, _props in children if node.rsplit("/", 1)[-1] in route_nodes):
        row = by_name.get(node_name.rsplit("/", 1)[-1])
        if row is None:
            continue
        for binding in row["bcm_bindings"]:
            if binding["phandle"] not in routed_handles:
                routed_handles.add(binding["phandle"])
                routed_resources.append(binding)

    rsc_devices = [resolved_node(handle, "qcom,rscs") for handle in rsc_handles]
    return {
        "scope": (
            "Pinned DT bus geometry and BCM/RSC binding topology. It does not contain "
            "request values, runtime BCM metadata bytes, live TCS ownership, or votes."
        ),
        "source_order_basis": {
            "child_order": "msm_bus_of_rpmh.c:768 for_each_child_of_node; msm_bus_fabric_rpmh.c:1846 indexed registration loop",
            "bcm_binding_order": "qcom,bcms property order consumed by msm_bus_setup_dev_conn()",
            "bcm_rsc": "msm_bus_fabric_rpmh.c:bcm_clist_add uses qcom,rscs[0]",
            "bus_device_parents": "all qcom,bus-dev target nodes are included, even when not route members and without BCM bindings",
            "buswidth_default": "msm_bus_of_rpmh.c:get_agg_params defaults qcom,buswidth to 8",
            "aggregation_ports_default": "msm_bus_of_rpmh.c:get_agg_params defaults qcom,agg-ports to num_qports",
        },
        "summary": {
            "topology_bus_nodes": len(bus_nodes),
            "bcm_linked_bus_nodes": sum(bool(row["bcm_bindings"]) for row in bus_nodes),
            "bcm_bindings": sum(len(row["bcm_bindings"]) for row in bus_nodes),
            "bcm_device_nodes": len(bcm_devices),
            "unique_cmd_db_names": len({row["cmd_db_name"] for row in bcm_devices}),
            "rsc_device_nodes": len(rsc_devices),
            "client_route_nodes": len(route_nodes),
            "client_routed_bcm_device_nodes": len(routed_resources),
            "client_routed_cmd_db_names": len({row["cmd_db_name"] for row in routed_resources}),
        },
        "rsc_devices": rsc_devices,
        "bcm_devices": bcm_devices,
        "bus_nodes": bus_nodes,
        "client_routed_bcm_devices": routed_resources,
    }


def inventory(tree, dtb: Path, kernel_commit: str) -> dict:
    buses = bcm_names(tree)
    known_routes = {}
    for label, route in KNOWN_ROUTES.items():
        selected = tree.selected_route(
            KNOWN_NODES[route[0]][0], KNOWN_NODES[route[-1]][0]
        )
        if selected != route:
            raise ValueError(f"{label}: selected route changed to {selected}")
        known_routes[label] = selected

    clients = []
    clients_by_bcm: dict[str, set[str]] = {}
    resolved_paths = 0
    unresolved_paths = []

    for node in tree.nodes:
        props = tree.nodes[node]
        if "qcom,msm-bus,name" not in props:
            continue

        client_name = tree.raw(node, "qcom,msm-bus,name")
        counts = tree.cells(node, "qcom,msm-bus,num-cases")
        paths = tree.cells(node, "qcom,msm-bus,num-paths")
        if len(counts) != 1 or len(paths) != 1 or not counts[0] or not paths[0]:
            raise ValueError(f"{node} ({client_name}): invalid case/path counts")
        if "qcom,msm-bus,vectors-KBps" in props:
            vector_prop = "qcom,msm-bus,vectors-KBps"
        elif "qcom,msm-bus,vectors" in props:
            vector_prop = "qcom,msm-bus,vectors"
        else:
            raise ValueError(f"{node} ({client_name}): missing bus vector property")

        cells = tree.cells(node, vector_prop)
        expected_cells = counts[0] * paths[0] * 4
        if len(cells) != expected_cells:
            raise ValueError(
                f"{node} ({client_name}): {vector_prop} has {len(cells)} cells, "
                f"expected {expected_cells}"
            )

        case_names = []
        if "qcom,bus-vector-names" in props:
            value = props["qcom,bus-vector-names"]
            case_names = re.findall(r'"([^"\\]*(?:\\.[^"\\]*)*)"', value)
            if len(case_names) != counts[0]:
                raise ValueError(f"{node} ({client_name}): vector-name count mismatch")
        else:
            case_names = [f"case{index}" for index in range(counts[0])]

        client_bcms: set[str] = set()
        cases = []
        for case_index, case_name in enumerate(case_names):
            path_rows = []
            for path_index in range(paths[0]):
                offset = (case_index * paths[0] + path_index) * 4
                src_id, dst_id, average, instantaneous = cells[offset : offset + 4]
                row = {
                    "path_index": path_index,
                    "source_id": src_id,
                    "destination_id": dst_id,
                    "average": average,
                    "instantaneous": instantaneous,
                }
                try:
                    route = tree.selected_route(src_id, dst_id)
                    if route is None:
                        raise ValueError("no route in the pinned arbiter search")
                    route_bcms = []
                    for bus_node in route:
                        bus_path = ROOT + "/" + bus_node
                        for handle in tree.cells(bus_path, "qcom,bcms"):
                            if handle not in buses:
                                raise ValueError(
                                    f"{bus_node}: unknown BCM phandle 0x{handle:x}"
                                )
                            if buses[handle] not in route_bcms:
                                route_bcms.append(buses[handle])
                    row["route"] = route
                    row["bcms"] = route_bcms
                    resolved_paths += 1
                    client_bcms.update(route_bcms)
                    for bcm in route_bcms:
                        clients_by_bcm.setdefault(bcm, set()).add(client_name)
                except ValueError as error:
                    row["route_error"] = str(error)
                    unresolved_paths.append(
                        {"client": client_name, "node": node, **row}
                    )
                path_rows.append(row)
            cases.append({"name": case_name, "paths": path_rows})

        clients.append({
            "name": client_name,
            "node": node,
            "vector_property": vector_prop,
            "active_only": "qcom,msm-bus,active-only" in props,
            "context_mode": (
                "active-only" if "qcom,msm-bus,active-only" in props else "dual-context-default"
            ),
            "cases": cases,
            "route_bcms": sorted(client_bcms),
        })

    clients.sort(key=lambda client: (client["name"], client["node"]))
    devbw_clients, devbw_unresolved = devbw_inventory(tree, buses)
    topology = fabric_topology(tree, clients, devbw_clients)
    return {
        "kernel_commit": kernel_commit,
        "dtb_sha256": hashlib.sha256(dtb.read_bytes()).hexdigest(),
        "known_route_checks": known_routes,
        "scope": (
            "Device-tree MSM bus vectors plus qcom,devbw route metadata only. "
            "Not runtime registration, active usecases, or live requests; no bus votes issued."
        ),
        "summary": {
            "declared_clients": len(clients),
            "declared_paths": sum(len(case["paths"]) for c in clients for case in c["cases"]),
            "devbw_clients": len(devbw_clients),
            "devbw_paths": sum(len(client["paths"]) for client in devbw_clients),
            "devbw_unresolved_route_paths": len(devbw_unresolved),
            "resolved_route_paths": resolved_paths,
            "unresolved_route_paths": len(unresolved_paths),
        },
        "clients_by_bcm": {
            bcm: sorted(names) for bcm, names in sorted(clients_by_bcm.items())
        },
        "unresolved_paths": unresolved_paths,
        "devbw_clients": devbw_clients,
        "devbw_unresolved_paths": devbw_unresolved,
        "fabric_topology": topology,
        "clients": clients,
    }


def runtime_registration_snapshot(tree, dmesg: Path, dt_declared_clients: list[dict]) -> dict:
    """Resolve register_client_adhoc path logs from one saved Linux boot."""
    text = dmesg.read_text(errors="replace")
    pending = []
    registrations = []
    failed_paths = []
    orphan_paths = []
    find_re = re.compile(r"register_client_adhoc:find path\.src\s+(-?\d+) dest\s+(-?\d+)")
    fail_re = re.compile(r"register_client_adhoc:Failed to find path\.src\s+(-?\d+) dest\s+(-?\d+)")
    handle_re = re.compile(r"register_client_adhoc:Client handle\s+(\d+)\s+(\S+)")

    def resolve(src_id: int, dst_id: int) -> dict:
        row = {"source_id": src_id, "destination_id": dst_id}
        try:
            route = tree.selected_route(src_id, dst_id)
            if route is None:
                raise ValueError("no route in the pinned arbiter search")
            route_bcms = []
            bcm_by_handle = bcm_names(tree)
            for name in route:
                for handle in tree.cells(ROOT + "/" + name, "qcom,bcms"):
                    if handle not in bcm_by_handle:
                        raise ValueError(f"{name}: unknown BCM phandle 0x{handle:x}")
                    if bcm_by_handle[handle] not in route_bcms:
                        route_bcms.append(bcm_by_handle[handle])
            row["route"] = route
            row["bcms"] = route_bcms
        except ValueError as error:
            row["route_error"] = str(error)
        return row

    for line in text.splitlines():
        match = find_re.search(line)
        if match:
            pending.append((int(match.group(1)), int(match.group(2))))
            continue
        match = fail_re.search(line)
        if match:
            failure = resolve(int(match.group(1)), int(match.group(2)))
            failure["log_line"] = line.strip()
            failed_paths.append(failure)
            if pending and pending[-1] == (failure["source_id"], failure["destination_id"]):
                pending.pop()
            continue
        match = handle_re.search(line)
        if match:
            routes = [resolve(src, dst) for src, dst in pending]
            registrations.append({
                "handle": int(match.group(1)),
                "name": match.group(2),
                "paths": routes,
            })
            pending = []
    orphan_paths = [resolve(src, dst) for src, dst in pending]

    dt_names = {client["name"] for client in dt_declared_clients}
    runtime_names = {row["name"] for row in registrations}
    unresolved_routes = sum(
        "route_error" in path
        for row in registrations
        for path in row["paths"]
    )
    return {
        "dmesg_sha256": hashlib.sha256(dmesg.read_bytes()).hexdigest(),
        "scope": (
            "Route endpoints logged when clients registered in this captured Linux boot. "
            "Does not report current usecases, requested values, or live BCM votes."
        ),
        "summary": {
            "registered_handles": len(registrations),
            "unique_debug_names": len(runtime_names),
            "logged_paths": sum(len(row["paths"]) for row in registrations),
            "resolved_paths": sum(
                "route" in path for row in registrations for path in row["paths"]
            ),
            "unresolved_paths": unresolved_routes,
            "failed_path_logs": len(failed_paths),
            "orphan_path_logs": len(orphan_paths),
        },
        "registration_names_not_in_dt": sorted(runtime_names - dt_names),
        "dt_names_without_registration_log": sorted(dt_names - runtime_names),
        "failed_paths": failed_paths,
        "orphan_paths": orphan_paths,
        "registrations": registrations,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dtb", type=Path)
    parser.add_argument("--kernel-source", type=Path, default=DEFAULT_KERNEL_SOURCE)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--dmesg", type=Path)
    args = parser.parse_args()
    commit = ROUTES_MODULE.validate_kernel_source(args.kernel_source)
    value_semantics = validate_devbw_value_semantics(args.kernel_source)
    result = inventory(DtsTree(args.dtb), args.dtb, commit)
    result["devbw_value_semantics"] = value_semantics
    if args.dmesg:
        result["runtime_registration_snapshot"] = runtime_registration_snapshot(
            DtsTree(args.dtb), args.dmesg, result["clients"]
        )
    rendered = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
