#!/usr/bin/env python3
"""Apply the one target-specific SQLCipher entropy hook to the pinned amalgamation."""

from __future__ import annotations

import argparse
from pathlib import Path


SEED_OLD = """#ifndef SQLCIPHER_TEST
  sqlite3_randomness(FORTUNA_MAX_SZ, random_buffer);
#endif"""
SEED_NEW = """#ifndef SQLCIPHER_TEST
  if (sqlite3_secure_randomness(FORTUNA_MAX_SZ, random_buffer) != SQLITE_OK) {
    sqlcipher_memset(random_buffer, 0, FORTUNA_MAX_SZ);
    if (--ltc_ref_count == 0) {
      fortuna_done(&prng);
      sqlcipher_memset((void *)&prng, 0, sizeof(prng));
      ltc_init = 0;
    }
    sqlite3_mutex_leave(sqlcipher_mutex(SQLCIPHER_MUTEX_PROVIDER_ACTIVATE));
    return SQLITE_ERROR;
  }
#endif"""


def replace_once(source: str, old: str, new: str, description: str) -> str:
    matches = source.count(old)
    if matches != 1:
        raise SystemExit(f"expected one {description}, found {matches}")
    return source.replace(old, new, 1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    source = args.source.read_text()
    source = replace_once(source, SEED_OLD, SEED_NEW, "pinned SQLCipher Fortuna seed site")

    for call in (
        "register_prng(&fortuna_desc)",
        "register_cipher(&rijndael_desc)",
        "register_hash(&sha512_desc)",
        "register_hash(&sha256_desc)",
        "register_hash(&sha1_desc)",
    ):
        old = f"    if({call} < 0) return SQLITE_ERROR;"
        new = (
            f"    if({call} < 0) {{\n"
            "      sqlite3_mutex_leave(sqlcipher_mutex(SQLCIPHER_MUTEX_PROVIDER_ACTIVATE));\n"
            "      return SQLITE_ERROR;\n"
            "    }"
        )
        source = replace_once(source, old, new, f"{call} registration site")

    source = replace_once(
        source,
        """    if(fortuna_start(&prng) != CRYPT_OK) {
      return SQLITE_ERROR;
    }""",
        """    if(fortuna_start(&prng) != CRYPT_OK) {
      sqlite3_mutex_leave(sqlcipher_mutex(SQLCIPHER_MUTEX_PROVIDER_ACTIVATE));
      return SQLITE_ERROR;
    }""",
        "Fortuna startup error path",
    )

    source = replace_once(
        source,
        """  if(sqlcipher_ltc_add_random(ctx, random_buffer, FORTUNA_MAX_SZ) != SQLITE_OK) {
    return SQLITE_ERROR;
  }""",
        """  if(sqlcipher_ltc_add_random(ctx, random_buffer, FORTUNA_MAX_SZ) != SQLITE_OK) {
    sqlcipher_memset(random_buffer, 0, FORTUNA_MAX_SZ);
    if (--ltc_ref_count == 0) {
      fortuna_done(&prng);
      sqlcipher_memset((void *)&prng, 0, sizeof(prng));
      ltc_init = 0;
    }
    sqlite3_mutex_leave(sqlcipher_mutex(SQLCIPHER_MUTEX_PROVIDER_ACTIVATE));
    return SQLITE_ERROR;
  }""",
        "Fortuna entropy-add error path",
    )

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(source)
    print(f"SQLCIPHER_ENTROPY_HOOK|PASS|{args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
