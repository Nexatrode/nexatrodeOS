# QEMU TLS test certificates

These CA and server private keys are public test fixtures. Never trust this CA
or reuse either key in a deployed image. The server certificate has the DNS
subject alternative name `personal-os.test`; the QEMU guest embeds only
`ca.der` as its test trust anchor. The fixture is valid for ten years from its
2026 creation so calendar validation can run against QEMU's host-controlled
PL031 clock. The QEMU clock is a development input, not a production source of
trusted time.

`server-chain.crt` contains the leaf followed by the test CA, matching the
certificate-chain shape used by SimpleXMQ's pinned server-identity check.
