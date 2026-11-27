"""Adaptive presentation cannot change transport or select a different producer."""
from pathlib import Path
import re,subprocess,unittest
ROOT=Path(__file__).resolve().parents[1];BASE='ca12cc96'
COUNTS={'rust/src/pixel_mpss_tx3_elf_retry.rs':3,'rust/src/pixel_mpss_elf_pool_release.rs':1}
def without_startup(path,source):
 pat=r'/\* STARTUP_PRESSURE_PANEL_BEGIN \*/\n.*?/\* STARTUP_PRESSURE_PANEL_END \*/\n';blocks=re.findall(pat,source,re.S)
 assert len(blocks)==COUNTS.get(path,0),(path,len(blocks))
 assert all('feature="sunfish-startup-pressure"'in b for b in blocks)
 source=re.sub(pat,'',source,flags=re.S)
 if path=='rust/Cargo.toml':source=source.replace('sunfish-startup-pressure = ["sunfish-elf-pool-release"]\n','')
 return source
class Source(unittest.TestCase):
 def test_all_parent_rust_sources_recovered(self):
  files=subprocess.check_output(['git','ls-tree','-r','--name-only',BASE,'rust/src'],text=True).splitlines()+['rust/Cargo.toml']
  for f in files:self.assertEqual(without_startup(f,(ROOT/f).read_text()),subprocess.check_output(['git','show',BASE+':'+f],text=True),f)
 def test_choice_reads_only_existing_pool_history(self):
  s=(ROOT/'rust/src/pixel_mpss_tx3_elf_retry.rs').read_text();self.assertIn('if owner.elf_tx_slots().is_some(){self.show_elf_pool_release(d,owner);return;}',s)
  for f in COUNTS:
   blocks=re.findall(r'/\* STARTUP_PRESSURE_PANEL_BEGIN \*/\n(.*?)/\* STARTUP_PRESSURE_PANEL_END \*/', (ROOT/f).read_text(),re.S)
   for b in blocks:
    for bad in ['&mut','unsafe','with_io','revalidate','queue_reply','publish_next','clock()']:self.assertNotIn(bad,b)
if __name__=='__main__':unittest.main()
