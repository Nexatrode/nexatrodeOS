# Third-party notices for the SimpleX messaging guest

## SimpleXMQ

SimpleXMQ 7.0.0.6, copyright 2020-2022 simplex.chat. The guest uses a modified
version of SimpleXMQ under the GNU Affero General Public License, version 3 only
(`AGPL-3.0-only`). It comes without warranty. [Full license](LICENSES/AGPL-3.0-only.txt)
· [Upstream source at efaad8e73436d60f5052f07dda6b71151ad5039b](https://github.com/simplex-chat/simplexmq/tree/efaad8e73436d60f5052f07dda6b71151ad5039b)

**NexatrodeOS modifications as of November 19, 2026.** The seL4/Microkit port
adapts the client TLS transport to the guest networking service and the agent's
SQLite store to a guest VFS. It extracts protocol and authentication code for
the freestanding client. The agent integration connects the upstream messaging
and file-transfer services to the guest over IPC.

The port source and build instructions are public. To inspect the source for
a particular build, select its NexatrodeOS revision and use the pinned upstream
source above together with these files from the same revision.

- [TLS transport patch](patches/simplexmq-microkit-tls-backend.patch) and
  [SQLite VFS patch](patches/simplexmq-microkit-sqlite-vfs.patch).
- [Client shims](experiments/ghc-microkit/guest-client-shims),
  [protocol shims](experiments/ghc-microkit/upstream-shims), and
  [guest agent source and build scripts](experiments/ghc-microkit).
- [Guest build targets](experiments/ghc-microkit/Makefile) and
  [port and interoperability notes](docs/simplex-port-feasibility.md).

When distributing a guest binary, include these notices and license texts
and provide the corresponding source for that exact build, including the
modifications and build scripts. [NexatrodeOS source repository](https://github.com/Nexatrode/nexatrodeOS)

## Zstandard

The guest uses the Zstandard C decoder bundled with the pinned Hackage
`zstd-0.1.3.0` source archive to read compressed SimpleX messages. It uses
the BSD license option. Zstandard is copyright 2016-present, Facebook, Inc.
The bundled entropy decoder and xxHash code also credit Yann Collet and
Facebook, Inc. [Full BSD notice and disclaimer](LICENSES/Zstandard-BSD.txt)
· [Decoder build and archive checksum](experiments/ghc-microkit/build_simplex_zstd_decoder.py)
· [Source archive](https://hackage.haskell.org/package/zstd-0.1.3.0/zstd-0.1.3.0.tar.gz)

## Optional native NetSurf guest

The separate `experiments/netsurf-native` build uses NetSurf at
`170d89462442c286f80c0604f30e4c7f2a801265`, under GPL version 2, with a new
NexatrodeOS frontend and small platform/build changes. Its local frontend files
are marked `GPL-2.0-only`. NetSurf is copyright its respective contributors;
its original source headers and license are preserved in the downloaded tree.

The complete revision/URL/SHA-256 inventory is
[sources.json](experiments/netsurf-native/sources.json). It also pins NetSurf's
parser/CSS/DOM/bitmap libraries and build system, Picolibc 1.8.12, zlib 1.3.1,
and stb_truetype. The Roboto font and its OFL notice come from the existing
[font assets](experiments/slint-notes/assets/fonts). MacSurf and Ladybird code
are not used by this experiment.

The builder copies the pinned dependency license/copyright files and Roboto's
OFL into the guest staging directory's `licenses/` folder. Keep those notices
with a distributed build, and provide the corresponding NetSurf source,
frontend, build scripts and modifications for that exact version. See the
[experiment instructions and limitations](experiments/netsurf-native/README.md).


The opt-in HTTPS broker uses the existing `nexatrode-guest-https` Rust adapter,
rustls 0.23.45, ring 0.17.14, webpki-roots 1.0.11, getrandom and the seL4 Rust
runtime/allocator. Its complete dependency versions and source revisions are
in [Cargo.lock](experiments/netsurf-native/https-worker/Cargo.lock). The builder
copies each available dependency license/copyright notice to the stage's
`licenses/` directory and records package license expressions and source URLs
in `licenses/https-crates.json`. Keep that inventory and the copied notices
with the optional broker binary. The browser's GPLv2 source obligations still
apply to the browser frontend and engine.


PNG and JPEG support in the optional browser uses libpng 1.6.59 and
libjpeg-turbo 3.2.1. Their upstream licenses and contributor notices are
preserved in the pinned archives and copied as `libpng-LICENSE` and
`libjpeg-turbo-LICENSE.md` in the staging directory. The latter describes the
IJG, modified BSD and zlib license terms for its constituent code. Versioned
upstream URLs and archive SHA-256 values are in the same browser `sources.json`.
The existing zlib notice still applies. Synthetic test patterns are generated
locally; live website reference images stay in ignored test output.

The optional JavaScript build links Duktape 2.7.0 (MIT), bundled inside the
pinned NetSurf source. Its original license and author notices are preserved;
the builder also exports the embedded license and author list into `licenses/`.
The WebIDL generator nsgenbind 0.9 is pinned in `sources.json`, is MIT-licensed
(copyright Vincent Sanders), and runs only on the build host. The local DOM
traversal, form repaint, clock and execution-budget changes are reproduced by
the experiment build scripts. Enabling JavaScript does not change the browser's
GPLv2 distribution obligations.

The native DOM reflow extension rebuilds NetSurf's render tree from its live DOM.
Its GPL-2.0-only implementation and the exact reproducible upstream patches are
in `experiments/netsurf-native/dom_reflow.inc` and `reflow.py`. The latter also
patches libDOM's class-token cache and adds the CharacterData JavaScript value
binding. Original upstream notices remain preserved in the staged sources.
The follow-up allocation-guard, rollback, main-document status and Reload/Stop
integration is in `alloc_guard.c`, `page.c`, `browser.c` and `hardening.py`, under
the same stated GPL-2.0-only frontend license. The optional disposable test CA
is generated locally by the host's OpenSSL tool; no OpenSSL implementation is
linked into the guest, and normal images do not trust that fixture CA.
The bounded fragment follow-up is reproduced by `dom_work.py` and
`fragment_limits.py`. The libDOM guard in `fragment_limits.inc` retains an MIT
license; the frontend helpers declare GPL-2.0-only. Upstream libDOM and NetSurf
copyright/license headers remain intact in the patched sources.
