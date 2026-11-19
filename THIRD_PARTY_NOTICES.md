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
