# Guest HTTPS transport prototype

This `no_std` TLS client adapter authenticates the
hostname and certificate chain against the WebPKI root set, uses the supplied
Unix time for certificate validation, limits buffered TLS records and the
response, and accepts a caller-provided TCP transport. NexaWal's experimental
`wallet-https` feature connects `Session` to the wallet's TCP, entropy and clock
services. The test configuration is constructed with
`experiments/ghc-microkit/enable_wallet_https_system.py`. The separate scan
worker still uses its legacy node and is not started for other configured
nodes. This feature must be explicitly enabled and is not yet a default build.
Ports other than 443 continue to use the existing HTTP transport; port 443
uses TLS without a plaintext fallback. The wallet's configured node name is
used for hostname validation. Do not disable certificate verification.

When the merged UTM system includes the HVF timer poller, the configuration
tool gives it a 100 µs budget every 1,000 µs. The wallet uses this timer to
poll an empty TCP socket. During a binary range scan it can process additional
available data and blocks within the same wake before yielding.

For a timing run, build the wallet with `wallet-range-profile`. Its
`RANGE|PROFILE` records give cumulative microseconds spent in transport,
binary decoding, scanning and committing, and checkpoint persistence. The
difference between `wall_us` and `work_us` measures elapsed time outside the
wallet's processing steps. It includes timer and host scheduling delays.

The `rust` workspace uses its vendored ring copy to enable the upstream ELF
AArch64 assembly for the seL4 `target_os = "none"` target. A guest build must
compile that C/assembly with the project's freestanding headers and link the
wallet entropy and clock channels. `cargo check` alone does not prove that the
assembly is present; perform a full target build.

Host checks (the probe sends only a public `get_info` request):

```sh
cargo test --offline --manifest-path experiments/guest-https/Cargo.toml
cargo run --offline --release --manifest-path experiments/guest-https/Cargo.toml --example node_probe
```

The probe constrains reads and writes to the guest TCP page's 128-byte payload
and checks a valid node response, hostname rejection, certificate validity
time, the response-size limit, and a polled session with partial writes and
artificial would-block results. It requires network access to the node.
