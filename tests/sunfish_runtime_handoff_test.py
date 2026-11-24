"""Run the actual split handoff bodies against receipt/owner lifecycle fixtures."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def body(source, name):
    import re
    match = re.search(r'(?:pub\(super\) )?fn ' + name + r'\(', source)
    if match is None:
        raise AssertionError(f'missing split function {name}')
    start = source.index('{', match.start())
    depth = 1
    end = start + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[match.start():end]


class RuntimeHandoffTests(unittest.TestCase):
    def test_actual_split_preserves_order_failure_and_single_owner(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        # Select the opt-in wrapper rather than the retained legacy start.
        source = source[source.index('fn construct_runtime'):]
        bodies = '\n'.join(body(source, name) for name in ['construct_runtime', 'begin_prepared_runtime', 'start'])
        harness = r'''
use std::cell::RefCell;
thread_local! {static EVENTS:RefCell<Vec<&'static str>>=RefCell::new(Vec::new());}
fn event(e:&'static str){EVENTS.with_borrow_mut(|v|v.push(e));}
struct Display;
impl Display {fn replace_line(&self,_:usize,_:&str){} fn hex(&self,_:usize,_:&str,v:u32){event(match v{1=>"mark1",2=>"mark2",3=>"mark3",_=>panic!()});} fn flush(&self){}}
struct Parts;
struct RuntimeDomain;
struct Startup{_mpss_runtime:Option<RuntimeDomain>,ready:bool,token:bool,fail:bool,begun:bool,early:bool}
impl RuntimeDomain {fn new(_:Parts)->Self{event("construct");Self} fn begin(&mut self,_:&Display,s:&mut Startup){event("begin");assert!(!s.token);assert!(s._mpss_runtime.is_none());if s.early{return;}s.begun=true;}}
mod pixel_mpss_auth {use super::*;
pub fn runtime_ready(s:&Startup)->bool{s.ready}
pub fn take_runtime(s:&mut Startup)->Result<Parts,()>{event("take");if s.fail{return Err(());} assert!(s.token);s.token=false;Ok(Parts)}
pub fn show(_:&Display,_:&Startup){event("failure");}}
mod actual {use super::*;
ACTUAL_BODIES
}
fn fresh(ready:bool,fail:bool)->Startup{Startup{_mpss_runtime:None,ready,token:true,fail,begun:false,early:false}}
fn events()->Vec<&'static str>{EVENTS.with_borrow_mut(std::mem::take)}
fn main(){
let d=Display;let mut s=fresh(false,false);actual::start(&d,&mut s);assert!(events().is_empty());assert!(s.token);
let mut s=fresh(true,true);actual::start(&d,&mut s);assert_eq!(events(),["mark1","take","failure"]);assert!(s.token && s._mpss_runtime.is_none() && !s.begun);
let mut s=fresh(true,false);actual::start(&d,&mut s);assert_eq!(events(),["mark1","take","construct","mark2","mark3","begin"]);assert!(!s.token && s._mpss_runtime.is_some() && s.begun);
actual::start(&d,&mut s);assert!(events().is_empty());
let mut s=fresh(true,false);s.early=true;actual::start(&d,&mut s);assert_eq!(events(),["mark1","take","construct","mark2","mark3","begin"]);assert!(!s.token && s._mpss_runtime.is_some() && !s.begun);
actual::start(&d,&mut s);assert!(events().is_empty());
println!("Readiness, failed receipt retention, construction-before-begin and exactly one runtime owner passed.");
}
'''.replace('ACTUAL_BODIES', bodies)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            (path / 'test.rs').write_text(harness)
            subprocess.run(['rustc', '+nightly-2026-09-25', '--edition=2024', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
            subprocess.run([str(path / 'test')], check=True)


if __name__ == '__main__':
    unittest.main()
