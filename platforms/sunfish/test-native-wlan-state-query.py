#!/usr/bin/env python3
from pathlib import Path
import os,tomllib,subprocess,importlib.util,re,json,hashlib,argparse
r=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser(description="Run the actual modem runtime tests with WLAN state query enabled or disabled; no device actions.")
p.add_argument('--features',type=Path,required=True)
p.add_argument('--output',type=Path,required=True)
p.add_argument('--disable-query',action='store_true')
p.add_argument('--disable-listener',action='store_true')
a=p.parse_args();o=a.output.resolve();o.mkdir(parents=True,exist_ok=True,mode=0o700)
base=r/'.cache/sunfish-native-wifi/private'
inputs=[r/'rust/Cargo.toml',Path(__file__).resolve(),r/'platforms/sunfish/test-native-mpss-linux-transport.py']
inputs+=list((r/'rust/src').rglob('*.rs'))+list((r/'tests').glob('*.rs'))
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
hashes={str(p.relative_to(r)):sha(p) for p in inputs}
f=tomllib.loads((r/'rust/Cargo.toml').read_text())['features'];selected=set(a.features.read_text().strip().split(','))
if a.disable_query:
 selected.discard('sunfish-wlan-state-query');selected.discard('sunfish-wlan-state-listener')
if a.disable_listener:selected.discard('sunfish-wlan-state-listener')
pending=list(selected)
while pending:
 for dep in f.get(pending.pop(),[]):
  if dep in f and dep not in selected:selected.add(dep);pending.append(dep)
env=dict(os.environ,RUSTUP_TOOLCHAIN='nightly-2026-09-25',RUST_MIN_STACK='16777216',SUNFISH_LINUX_RMTFS_C=str(base/'linux-rmtfs/linux-scm.txt'),SUNFISH_RMTFS_WIRE_C=str(base/'linux-rmtfs-server/wire.txt'),SUNFISH_GLK_MEMORY_C=str(base/'mpss-glink-memory-linux'),SUNFISH_GLK_TRANSPORT_C=str(base/'mpss-glink-transport-linux'))
args=['rustc','--edition=2024','--test','-D','warnings','-A','deprecated','-A','dead_code','-C','opt-level=2','--cfg','mpss_glink_tests','--cfg','mpss_glink_transport_tests']
for feat in sorted(selected):args+=['--cfg',f'feature="{feat}"']
spec=importlib.util.spec_from_file_location('transport_runner',r/'platforms/sunfish/test-native-mpss-linux-transport.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
def expand(path):
 text=path.read_text()
 if path.name=='sunfish_linux_rmtfs_server_test.rs':
  for name in ['qcom_modem_status','qcom_modem_crash']:
   block=f'#[cfg(feature = "sunfish-native-startup-mpss-runtime")]\n#[allow(dead_code)]\n#[path = "../rust/src/{name}.rs"]\npub mod {name};\n'
   assert text.count(block)==1;text=text.replace(block,'',1)
 return module.absolute(re.sub(r'include!\("([^"]+)"\);',lambda m:expand((path.parent/m[1]).resolve()),text),path)
source=o/'runtime-transport.rs';source.write_text(expand(r/'tests/sunfish_mpss_linux_transport_test.rs'))
args += [str(source),'-o',str(o/'runtime-transport')]
(o/'runtime-transport-command.json').write_text(json.dumps(args,indent=2)+'\n')
p=subprocess.run(args,env=env,capture_output=True,text=True);(o/'runtime-transport-build.log').write_text(p.stdout+p.stderr)
if p.returncode:print(p.stderr[-6000:]);raise SystemExit(p.returncode)
p=subprocess.run([str(o/'runtime-transport'),'--test-threads=1'],env=env,capture_output=True,text=True);(o/'runtime-transport-tests.log').write_text(p.stdout+p.stderr);print(p.stdout[-3000:] if p.returncode else p.stdout.splitlines()[-2])
if p.returncode:raise SystemExit(p.returncode)
for rel,digest in hashes.items():assert sha(r/rel)==digest,rel
summary=re.search(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored',p.stdout)
assert summary
(o/'result.json').write_text(json.dumps(dict(result='passed',passed=int(summary[1]),ignored=int(summary[2]),query_enabled='sunfish-wlan-state-query' in selected,listener_enabled='sunfish-wlan-state-listener' in selected,source_sha256=hashes,features=sorted(selected),device_actions=[]),indent=2)+'\n')
