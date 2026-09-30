# Demo TLS TOFU cert (non-production)

Self-signed ECDSA P-256 cert used by `./dev test-net` for the guest TLS smoke.

- `server.key` / `server.crt` — host stub (`tls_stub.py`)
- `server.crt.der` — DER form of the leaf
- Guest pin: `../include/tls_tofu_pin.h` (DER + SHA-256)

Regenerate (from this directory):

```sh
openssl ecparam -name prime256v1 -genkey -noout -out server.key
openssl req -new -x509 -key server.key -out server.crt -days 3650 \
  -subj "/CN=personal-os-tls-demo" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature,keyAgreement" \
  -addext "extendedKeyUsage=serverAuth"
openssl x509 -in server.crt -outform DER -out server.crt.der
# then rebuild tls_tofu_pin.h from server.crt.der (see prior generation script)
```

After regenerating, refresh `tls_tofu_pin.h` and re-run `./dev test-net`.
