#!/usr/bin/env python3
"""Cross-compile SQLCipher's Haskell bindings and SimpleX VFS open probe."""

import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / ".cache"
SOURCES = CACHE / "simplex-chat-upstream/dist-newstyle/src"
GHCTREE = CACHE / "ghc-port/ghc-9.6.6"
GHC = GHCTREE / "_build/stage0/bin/aarch64-unknown-none-ghc"
PACKAGE_DB = GHCTREE / "_build/stage1/lib/package.conf.d"
GUEST_DB = CACHE / "sel4-tls-deps/guest-package.conf.d"
HSC2HS = (CACHE / "simplex-toolchain/cabal/store/ghc-9.6.3/hsc2hs-0.68.10-9931a881"
          / "bin/hsc2hs")
OUT = ROOT / "experiments/ghc-microkit/build/guest-sqlcipher-bindings"
PROBE = ROOT / "experiments/ghc-microkit/SqlCipherVfsCompileProbe.hs"
LLVM_AR = Path("/opt/homebrew/opt/llvm/bin/llvm-ar")
CROSS_PKG = GHCTREE / "_build/stage0/bin/aarch64-unknown-none-ghc-pkg"


def unique(pattern: str) -> Path:
    matches = list(SOURCES.glob(pattern))
    if len(matches) != 1:
        raise RuntimeError(f"expected one source for {pattern}, got {matches}")
    return matches[0]


def run(*args: str) -> None:
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True)


def package_ids() -> dict[str, str]:
    result = {}
    for db in (PACKAGE_DB, GUEST_DB):
        for conf in db.glob("*.conf"):
            fields = {}
            for line in conf.read_text().splitlines():
                if ":" in line and not line[:1].isspace():
                    key, value = line.split(":", 1)
                    fields[key.strip()] = value.strip()
            if fields.get("name") and fields.get("id"):
                result[fields["name"]] = fields["id"]
    return result


def register_package(name: str, version: str, modules: list[str],
                     objects: list[Path], dependencies: list[str], ids: dict[str, str],
                     import_dir: Path) -> None:
    archive_name = f"HS{name}-{version}-inplace"
    archive = OUT / "lib" / f"lib{archive_name}.a"
    archive.parent.mkdir(parents=True, exist_ok=True)
    if not objects:
        raise RuntimeError(f"no object files found for {name}")
    run(LLVM_AR, "crs", archive, *objects)
    missing = [dep for dep in dependencies if dep not in ids]
    if missing:
        raise RuntimeError(f"target package dependencies unavailable for {name}: {missing}")
    conf = OUT / "packagedb" / f"{name}-{version}-inplace.conf"
    conf.parent.mkdir(parents=True, exist_ok=True)
    conf.write_text(
        f"name: {name}\nversion: {version}\nid: {name}-{version}-inplace\n"
        f"key: {name}-{version}-inplace\nexposed: True\n"
        f"exposed-modules: {' '.join(modules)}\n"
        f"import-dirs: {import_dir}\nlibrary-dirs: {OUT / 'lib'}\n"
        f"hs-libraries: {archive_name}\n"
        f"depends: {' '.join(ids[dep] for dep in dependencies)}\n"
    )
    run(CROSS_PKG, "--no-user-package-db", f"--package-db={PACKAGE_DB}",
        f"--package-db={GUEST_DB}", "update", "--force", conf)


def compile_modules(sources: list[Path], source_dirs: list[Path],
                    packages: list[str], unit_id: str, output_dir: Path) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    ghc_args = [
        GHC, "--make", "-no-link", "-fforce-recomp", "-v0",
        "-no-global-package-db", "-package-env=-",
        "-package-db", PACKAGE_DB, "-package-db", GUEST_DB,
        *(f"-i{source_dir}" for source_dir in source_dirs),
        "-odir", output_dir, "-hidir", output_dir,
        f"-this-unit-id={unit_id}",
        "-XDoAndIfThenElse", "-XOverloadedStrings", "-XBangPatterns",
        "-XViewPatterns", "-XTypeOperators", "-XNamedFieldPuns",
        "-XRank2Types", "-XRecordWildCards",
        *(f"-package={package}" for package in packages),
        "-optc-target", "-optcaarch64-none-elf",
        f"-optc-I{ROOT / 'experiments/ghc-microkit/freestanding-headers'}",
        f"-optc-I{GHCTREE / 'rts/include'}", f"-optc-I{GHCTREE / 'rts'}",
        *sources,
    ]
    run(*ghc_args)


def main() -> None:
    direct_hsc = unique("direct-sq_*/Database/SQLite3/Bindings/Types.hsc")
    direct_root = direct_hsc.parents[3]
    simple_main = unique("sqlcipher_*/Database/SQLite/Simple.hs")
    simple_root = simple_main.parents[2]
    types_hs = OUT / "hs/Database/SQLite3/Bindings/Types.hs"
    OUT.mkdir(parents=True, exist_ok=True)
    types_hs.parent.mkdir(parents=True, exist_ok=True)
    run(HSC2HS, f"-I{direct_root}", direct_hsc, "-o", types_hs)

    direct_objects_dir = OUT / "obj/direct-sqlcipher"
    direct_sources = [
        types_hs,
        direct_root / "Database/SQLite3/Bindings.hs",
        direct_root / "Database/SQLite3/Direct.hs",
        direct_root / "Database/SQLite3.hs",
    ]
    compile_modules(direct_sources, [OUT / "hs", direct_root],
                    ["bytestring", "text"], "direct-sqlcipher-2.3.28-inplace",
                    direct_objects_dir)
    sqlite3_objects = list(direct_objects_dir.rglob("*.o"))
    ids = package_ids()
    register_package(
        "direct-sqlcipher", "2.3.28",
        ["Database.SQLite3", "Database.SQLite3.Bindings",
         "Database.SQLite3.Bindings.Types", "Database.SQLite3.Direct"],
        sqlite3_objects, ["base", "bytestring", "text"], ids, direct_objects_dir)
    ids["direct-sqlcipher"] = "direct-sqlcipher-2.3.28-inplace"
    simple_objects_dir = OUT / "obj/sqlcipher-simple"
    simple_sources = [simple_main, *sorted((simple_root / "Database/SQLite/Simple").rglob("*.hs"))]
    compile_modules(
        simple_sources, [direct_objects_dir, simple_root],
        ["attoparsec", "blaze-builder", "blaze-textual", "bytestring",
         "containers", "direct-sqlcipher", "exceptions", "Only",
         "template-haskell", "text", "time", "transformers"],
        "sqlcipher-simple-0.4.18.1-inplace", simple_objects_dir)
    simple_objects = list(simple_objects_dir.rglob("*.o"))
    register_package(
        "sqlcipher-simple", "0.4.18.1",
        ["Database.SQLite.Simple", "Database.SQLite.Simple.Ok",
         "Database.SQLite.Simple.FromField", "Database.SQLite.Simple.FromRow",
         "Database.SQLite.Simple.Internal", "Database.SQLite.Simple.QQ",
         "Database.SQLite.Simple.ToField", "Database.SQLite.Simple.ToRow",
         "Database.SQLite.Simple.Types", "Database.SQLite.Simple.Function",
         "Database.SQLite.Simple.Time",
         "Database.SQLite.Simple.Time.Implementation"],
        simple_objects,
        ["base", "attoparsec", "blaze-builder", "blaze-textual", "bytestring",
         "containers", "direct-sqlcipher", "exceptions", "Only", "template-haskell",
         "text", "time", "transformers"], ids, simple_objects_dir)
    probe_dir = OUT / "obj/probe"
    compile_modules([PROBE], [OUT / "hs", direct_objects_dir, simple_objects_dir],
                    ["direct-sqlcipher", "sqlcipher-simple"],
                    "sqlcipher-vfs-compile-probe", probe_dir)
    print(f"SQLCIPHER HASKELL BINDINGS|AARCH64 OBJECT BUILD PASS|{len(list((OUT / 'obj').rglob('*.o')))} objects")
    print("SQLCIPHER HASKELL PACKAGES|REGISTERED IN TARGET DB|2")
    print("SQLCIPHER CUSTOM VFS OPEN|TYPECHECK PASS")


if __name__ == "__main__":
    main()
