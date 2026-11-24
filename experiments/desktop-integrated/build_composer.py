"""Build a private Microkit composer with bounded runtime-manager resources."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

REVISION = "ec86afdcd662b5976d11d4994acf1b11a2979882"


def replace(path, before, after):
    text = path.read_text()
    if text.count(before) != 1:
        raise RuntimeError(f"unexpected pinned source in {path}")
    path.write_text(text.replace(before, after))


def build(source, output, pool_bits=24):
    if output.is_relative_to(source.resolve()):
        raise ValueError("composer output must be outside the shared source checkout")
    output.mkdir(parents=True, exist_ok=True)
    checkout = output / "source"
    if checkout.exists():
        raise ValueError("use a fresh composer output directory")
    subprocess.run(["git", "init", "--quiet", str(checkout)], check=True)
    subprocess.run(["git", "-C", str(checkout), "fetch", "--quiet", "--depth=1",
                    str(source.resolve()), REVISION], check=True)
    subprocess.run(["git", "-C", str(checkout), "checkout", "--quiet", "--detach", "FETCH_HEAD"], check=True)
    cspace = checkout / "tool/microkit/src/sdf/cspace.rs"
    replace(cspace, "    VSpace,\n}", "    VSpace,\n    RuntimePool,\n    RuntimeCNode,\n    RuntimeAsid,\n}")
    anchor = '                "cap_vspace" => CapMap::from_xml(CapMapType::VSpace, xml_sdf, &*child)?,'
    replace(cspace, anchor, anchor + """
                "cap_runtime_pool" => CapMap::from_xml(CapMapType::RuntimePool, xml_sdf, &*child)?,
                "cap_runtime_cnode" => CapMap::from_xml(CapMapType::RuntimeCNode, xml_sdf, &*child)?,
                "cap_runtime_asid" => CapMap::from_xml(CapMapType::RuntimeAsid, xml_sdf, &*child)?,""")
    builder = checkout / "tool/microkit/src/capdl/builder.rs"
    anchor = "                CapMapType::VSpace => capdl_util_make_page_table_cap(pd_src_shadow_cspace.vspace),"
    replace(builder, anchor, anchor + """
                CapMapType::RuntimePool => {
                    if cap_map.pd != pd.name { return Err("runtime resources must be local".into()); }
                    let id = spec_container.add_root_object(NamedObject {
                        name: Some(format!("{}_runtime_pool", pd.name)),
                        object: Object::Untyped(object::Untyped { size_bits: 24, paddr: None }),
                    });
                    Cap::Untyped(sel4_capdl_initializer_types::cap::Untyped { object: id })
                },
                CapMapType::RuntimeCNode => {
                    if cap_map.pd != pd.name { return Err("runtime resources must be local".into()); }
                    let mut initial = Vec::new();
                    if let Some(endpoint) = pd_src_shadow_cspace.endpoint {
                        initial.push(capdl_util_make_cte(0,
                            capdl_util_make_endpoint_cap(endpoint, RIGHTS_FAULT_HANDLER_EP, 0)));
                    }
                    let id = capdl_util_make_cnode_obj(&mut spec_container,
                        &format!("{}_runtime", pd.name), 12, initial);
                    // Full-depth alias for CNode invocations, alongside the
                    // root branch used to address newly allocated capabilities.
                    pd_shadow_cspaces[&pd.name].insert_cap_into_microkit_cnode(
                        &mut spec_container, 458, capdl_util_make_cnode_cap(id, 0, 0));
                    capdl_util_make_cnode_cap(id, 0, 46)
                },
                CapMapType::RuntimeAsid => {
                    if cap_map.pd != pd.name { return Err("runtime resources must be local".into()); }
                    let id = spec_container.add_root_object(NamedObject {
                        name: Some(format!("{}_runtime_asid", pd.name)),
                        object: Object::AsidPool(object::AsidPool { high: Word(1) }),
                    });
                    spec_container.spec.asid_slots.push(id);
                    Cap::AsidPool(sel4_capdl_initializer_types::cap::AsidPool { object: id })
                },""")
    replace(builder, "    // Only for aesthetic purposes:",
            """    for pool in spec_container.spec.asid_slots.iter_mut() {
        *pool = obj_old_id_to_new_id[pool];
    }

    // Only for aesthetic purposes:""")
    if pool_bits != 24:
        replace(builder, "size_bits: 24, paddr: None", f"size_bits: {pool_bits}, paddr: None")
    sizes = checkout / "tool/microkit/src/capdl/spec.rs"
    replace(sizes, "        Object::Endpoint => ObjectType::Endpoint.fixed_size_bits(sel4_config).unwrap(),",
            "        Object::Untyped(ut) => ut.size_bits as u64,\n"
            "        Object::Endpoint => ObjectType::Endpoint.fixed_size_bits(sel4_config).unwrap(),")
    subprocess.run(["cargo", "+nightly-2026-09-25", "build", "--locked", "--release",
                    "--manifest-path", str(checkout / "Cargo.toml"), "-p", "microkit-tool",
                    "--target-dir", str(output / "target")], check=True)
    patch = subprocess.check_output(["git", "-C", str(checkout), "diff", "--binary"])
    (output / "composer.patch").write_bytes(patch)
    (output / "provenance.json").write_text(json.dumps({
        "microkit_revision": REVISION,
        "runtime_pool_bits": pool_bits,
        "patch_sha256": hashlib.sha256(patch).hexdigest(),
        "composer_sha256": hashlib.sha256((output / "target/release/microkit").read_bytes()).hexdigest(),
        "scope": "private disposable experiment; bounded resources for the trusted manager",
    }, indent=2) + "\n")
    return output / "target/release/microkit"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--pool-bits", type=int, choices=(24, 26), default=24)
    arguments = parser.parse_args()
    print(build(arguments.source, arguments.output.resolve(), arguments.pool_bits))
