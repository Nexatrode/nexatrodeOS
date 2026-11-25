# SPDX-License-Identifier: BSD-2-Clause
"""Ephemeral host-loopback TLS certificates for the explicit disposable test build."""
import os
import subprocess
from pathlib import Path


def prepare_tls(base,run):
    out=base/'test-fixture-tls';out.mkdir(exist_ok=True)
    openssl=Path('/opt/homebrew/opt/openssl@3/bin/openssl')
    if not openssl.exists():openssl=Path('/usr/bin/openssl')
    root=out/'ca.der'
    if root.exists() and all((out/name).exists() for name in ('ca.pem','server.key','server.pem')):
        if all(subprocess.run([openssl,'x509','-checkend','3600','-noout','-in',out/name],capture_output=True).returncode==0
               for name in ('ca.pem','server.pem')):return root
    old=os.umask(0o077)
    try:
        run([openssl,'req','-x509','-newkey','ec','-pkeyopt','ec_paramgen_curve:P-256','-nodes','-days','7',
             '-subj','/CN=Disposable NetSurf Test CA','-keyout',out/'ca.key','-out',out/'ca.pem',
             '-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign'],out/'ca.log')
        run([openssl,'req','-new','-newkey','ec','-pkeyopt','ec_paramgen_curve:P-256','-nodes',
             '-subj','/CN=10.0.2.2','-keyout',out/'server.key','-out',out/'server.csr'],out/'csr.log')
        ext=out/'server.ext';ext.write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:10.0.2.2\n')
        run([openssl,'x509','-req','-in',out/'server.csr','-CA',out/'ca.pem','-CAkey',out/'ca.key',
             '-CAcreateserial','-days','7','-extfile',ext,'-out',out/'server.pem'],out/'sign.log')
        run([openssl,'x509','-in',out/'ca.pem','-outform','DER','-out',root],out/'der.log')
    finally:os.umask(old)
    return root
