#!/usr/bin/env python3
"""Generate RTS platform headers with GHC 9.6.6's Hadrian rules.

This mirrors hadrian/src/Rules/Generate.hs (generateGhcAutoconfH and
generateGhcPlatformH). It consumes configure's real mk/config.h and Hadrian's
configured system.config; it does not invent target feature tests.
"""

from pathlib import Path
import re
import subprocess
import sys


def parse_config(path: Path) -> dict[str, str]:
    values = {}
    for line in path.read_text().splitlines():
        match = re.match(r"^([a-z][a-z0-9-]*)\s*=\s*(.*?)\s*$", line)
        if match:
            values[match.group(1)] = match.group(2)
    return values


def cppify(value: str) -> str:
    return value.replace("-", "_").replace(".", "_")


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} GHC_SOURCE OUTPUT_DIR", file=sys.stderr)
        return 2

    ghc = Path(sys.argv[1]).resolve()
    output = Path(sys.argv[2]).resolve()
    config_h = ghc / "mk/config.h"
    system_config = ghc / "hadrian/cfg/system.config"
    if not config_h.is_file() or not system_config.is_file():
        raise SystemExit(f"not a configured GHC tree: {ghc}")

    cfg = parse_config(system_config)
    required = ("build-platform", "target-platform", "build-arch", "target-arch",
                "build-os", "target-os", "build-vendor", "target-vendor")
    missing = [key for key in required if key not in cfg]
    if missing:
        raise SystemExit(f"missing Hadrian settings: {', '.join(missing)}")
    if cfg["target-os"] != "sel4":
        raise SystemExit(f"expected configured target-os=sel4, got {cfg['target-os']}")

    # Hadrian's non-stage0 RTS headers describe the compiler target as host.
    build_platform = cfg["build-platform"]
    target_platform = cfg["target-platform"]
    build_arch = cfg["build-arch"]
    target_arch = cfg["target-arch"]
    build_os = cfg["build-os"]
    target_os = cfg["target-os"]
    build_vendor = cfg["build-vendor"]
    target_vendor = cfg["target-vendor"]

    platform_lines = [
        "#if !defined(__GHCPLATFORM_H__)",
        "#define __GHCPLATFORM_H__",
        "",
        f"#define BuildPlatform_TYPE  {cppify(build_platform)}",
        f"#define HostPlatform_TYPE   {cppify(target_platform)}",
        "",
        f"#define {cppify(build_platform)}_BUILD 1",
        f"#define {cppify(target_platform)}_HOST 1",
        "",
        f"#define {build_arch}_BUILD_ARCH 1",
        f"#define {target_arch}_HOST_ARCH 1",
        f'#define BUILD_ARCH "{build_arch}"',
        f'#define HOST_ARCH "{target_arch}"',
        "",
        f"#define {build_os}_BUILD_OS 1",
        f"#define {target_os}_HOST_OS 1",
        f'#define BUILD_OS "{build_os}"',
        f'#define HOST_OS "{target_os}"',
        "",
        f"#define {build_vendor}_BUILD_VENDOR 1",
        f"#define {target_vendor}_HOST_VENDOR 1",
        f'#define BUILD_VENDOR "{build_vendor}"',
        f'#define HOST_VENDOR "{target_vendor}"',
        "",
    ]
    if cfg.get("ghc-unregisterised") == "YES":
        platform_lines.append("#define UnregisterisedCompiler 1")
    platform_lines += ["", "#endif /* __GHCPLATFORM_H__ */", ""]

    # mk/config.h comes from the macOS machine building the cross-compiler.
    # Keep target feature tests from claiming host process, signal, or wall
    # clock APIs exist in a freestanding Microkit protection domain.
    unavailable_on_sel4 = {
        "HAVE_CLOCK_GETTIME",
        "HAVE_CTIME_R",
        "HAVE_FORK",
        "HAVE_LIBPTHREAD",
        "HAVE_PTHREAD_H",
        "HAVE_PTHREAD_SETNAME_NP_DARWIN",
        "HAVE_SCHED_H",
        "HAVE_SYS_MMAN_H",
        "HAVE_SYS_SELECT_H",
        "HAVE_TERMIOS_H",
        "HAVE_SYSCONF",
        "HAVE_GETEUID",
        "HAVE_GETEGID",
        "HAVE_GETGID",
        "HAVE_GETPID",
        "HAVE_GETUID",
        "HAVE_GETTIMEOFDAY",
        "HAVE_RAISE",
        "HAVE_SETITIMER",
        "HAVE_SIGINTERRUPT",
        "HAVE_SIGNAL_H",
        "HAVE_SYS_TIMEB_H",
        "HAVE_SYS_TIMES_H",
        "HAVE_SYS_TIME_H",
        "HAVE_TIME_H",
        "HAVE_TIMES",
        "HAVE_UTIME_H",
        "HAVE_VFORK",
        "HAVE_WORKING_FORK",
        "HAVE_WORKING_VFORK",
    }
    autoconf_lines = ["#if !defined(__GHCAUTOCONF_H__)", "#define __GHCAUTOCONF_H__"]
    for line in config_h.read_text().splitlines():
        if line.startswith("#define ") and line.split(maxsplit=2)[1] in unavailable_on_sel4:
            ident = line.split(maxsplit=2)[1]
            autoconf_lines.append(f"/* #undef {ident} */")
        elif line.startswith("#define PACKAGE_"):
            ident = line.split(maxsplit=2)[1]
            autoconf_lines.append(f"/* #undef {ident} */")
        elif line.startswith("#define __GLASGOW_HASKELL") or line == "/* REMOVE ME */":
            continue
        else:
            autoconf_lines.append(line)
    autoconf_lines += ["#endif /* __GHCAUTOCONF_H__ */", ""]

    output.mkdir(parents=True, exist_ok=True)
    (output / "ghcplatform.h").write_text("\n".join(platform_lines))
    (output / "ghcautoconf.h").write_text("\n".join(autoconf_lines))
    event_headers = output / "rts"
    event_headers.mkdir(parents=True, exist_ok=True)
    generator = ghc / "rts/gen_event_types.py"
    subprocess.run([
        sys.executable, str(generator),
        "--event-types-defines", str(event_headers / "EventLogConstants.h"),
        "--event-types-array", str(event_headers / "EventTypes.h"),
    ], check=True)
    print(f"Generated GHC 9.6.6 RTS headers from {ghc}")
    print(f"  target: {target_platform} ({target_os})")
    print(f"  output: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
