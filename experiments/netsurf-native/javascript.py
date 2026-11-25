# SPDX-License-Identifier: BSD-2-Clause
"""Host-only WebIDL binding generator; Duktape itself is pinned inside NetSurf."""
import os
from pathlib import Path


def build_generator(base, run):
    prefix=base/'host-tools'
    bison=Path('/opt/homebrew/opt/bison/bin/bison')
    run(['make','-C',base/'deps/nsgenbind','-j4','install',
         'NSSHARED='+str(base/'deps/buildsystem'),'PREFIX='+str(prefix),
         'BUILD=aarch64-apple-darwin','HOST=aarch64-apple-darwin','CC=cc',
         'VARIANT=release','BISON='+str(bison if bison.exists() else '/usr/bin/bison')],base/'nsgenbind.log')
    assert (prefix/'bin/nsgenbind').is_file()
    return str(prefix/'bin')+':'+os.environ['PATH']
