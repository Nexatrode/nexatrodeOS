#![allow(dead_code)]
use modem_storage_host::{
    qcom_modem_handlers as h, qcom_modem_pools as p, qcom_modem_services::Partition,
    qcom_modem_storage as s,
};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tag {
    Observe,
    Read,
    Write,
    Flush,
    PoolObserve,
    Zero,
    Stage,
    Invoke,
    Complete,
    PoolRead,
    PoolWrite,
}
#[derive(Clone, Copy, Debug)]
pub struct Op {
    pub tag: Tag,
    pub partition: usize,
    pub bytes: usize,
}
pub struct DiskState {
    pub observation: s::Observation,
    pub bytes: [Vec<u8>; 4],
    pub log: Vec<Op>,
    pub fault: Option<(Tag, h::Fault)>,
    pub panic: Option<Tag>,
    pub short: Option<Tag>,
    pub after: Option<(Tag, Box<dyn FnMut(&mut s::Observation)>)>,
}
#[derive(Clone)]
pub struct FakeDisk(pub Rc<RefCell<DiskState>>);
pub fn config() -> s::Configuration {
    s::Configuration {
        generation: 1,
        device: [9; 32],
        bindings: std::array::from_fn(|i| s::Binding {
            partition: s::PARTITIONS[i],
            object: i as u64 + 1,
            source: [i as u8 + 1; 32],
            disk: h::Disk {
                bytes: 16384,
                writable: i != 2,
                durable: true,
            },
        }),
    }
}
pub fn disk() -> FakeDisk {
    let c = config();
    FakeDisk(Rc::new(RefCell::new(DiskState {
        observation: s::Observation {
            configuration: c,
            identity: 81,
            ready: true,
        },
        bytes: std::array::from_fn(|i| (0..16384).map(|at| (at * 17 + i * 31) as u8).collect()),
        log: vec![],
        fault: None,
        panic: None,
        short: None,
        after: None,
    })))
}
impl FakeDisk {
    pub fn count(&self, t: Tag) -> usize {
        self.0.borrow().log.iter().filter(|o| o.tag == t).count()
    }
    fn op(&self, t: Tag, part: usize, n: usize) -> Result<(), h::Fault> {
        let mut s = self.0.borrow_mut();
        s.log.push(Op {
            tag: t,
            partition: part,
            bytes: n,
        });
        if s.panic == Some(t) {
            s.panic = None;
            drop(s);
            panic!("injected {t:?}");
        }
        if s.fault.is_some_and(|(v, _)| v == t) {
            return Err(s.fault.take().unwrap().1);
        }
        if s.after.as_ref().is_some_and(|(v, _)| *v == t) {
            let (_, mut f) = s.after.take().unwrap();
            f(&mut s.observation);
        }
        Ok(())
    }
}
impl s::Io for FakeDisk {
    fn observe(&mut self) -> Result<s::Observation, h::Fault> {
        self.op(Tag::Observe, 0, 0)?;
        Ok(self.0.borrow().observation)
    }
    fn read(&mut self, p: Partition, at: u64, out: &mut [u8]) -> Result<usize, h::Fault> {
        let i = s::index(p);
        self.op(Tag::Read, i, out.len())?;
        let mut v = self.0.borrow_mut();
        let n = if v.short == Some(Tag::Read) {
            v.short = None;
            out.len() / 2
        } else {
            out.len()
        };
        out[..n].copy_from_slice(&v.bytes[i][at as usize..at as usize + n]);
        Ok(n)
    }
    fn write(&mut self, p: Partition, at: u64, b: &[u8]) -> Result<usize, h::Fault> {
        let i = s::index(p);
        self.op(Tag::Write, i, b.len())?;
        let mut v = self.0.borrow_mut();
        let n = if v.short == Some(Tag::Write) {
            v.short = None;
            b.len() / 2
        } else {
            b.len()
        };
        v.bytes[i][at as usize..at as usize + n].copy_from_slice(&b[..n]);
        Ok(n)
    }
    fn flush(&mut self, p: Partition) -> Result<(), h::Fault> {
        self.op(Tag::Flush, s::index(p), 0)
    }
}
pub fn store(f: &FakeDisk) -> s::Storage<FakeDisk> {
    s::Storage::attach(s::Profile::validate(config()).unwrap(), f.clone())
}
pub struct PoolState {
    pub observation: p::Observation,
    pub bytes: [Vec<u8>; 2],
    pub log: Vec<Tag>,
    pub raw: [u64; 8],
    pub fault: Option<(Tag, h::Fault)>,
    pub panic: Option<Tag>,
    pub after: Option<(Tag, Box<dyn FnMut()>)>,
}
#[derive(Clone)]
pub struct FakePool(pub Rc<RefCell<PoolState>>);
pub fn pool() -> FakePool {
    FakePool(Rc::new(RefCell::new(PoolState {
        observation: p::Observation {
            generation: 1,
            identity: 91,
            ready: true,
            mappings: p::MAPPINGS,
            contract: true,
            initial_hlos: true,
        },
        bytes: [
            vec![0xa5; p::STORAGE.bytes as usize],
            vec![0x5a; p::HEAP.bytes as usize],
        ],
        log: vec![],
        raw: [0; 8],
        fault: None,
        panic: None,
        after: None,
    })))
}
impl FakePool {
    pub fn count(&self, t: Tag) -> usize {
        self.0.borrow().log.iter().filter(|o| **o == t).count()
    }
    fn op(&self, t: Tag) -> Result<(), h::Fault> {
        let mut s = self.0.borrow_mut();
        s.log.push(t);
        if s.panic == Some(t) {
            s.panic = None;
            drop(s);
            panic!("injected {t:?}");
        }
        if s.fault.is_some_and(|(v, _)| v == t) {
            return Err(s.fault.take().unwrap().1);
        }
        if s.after.as_ref().is_some_and(|(v, _)| *v == t) {
            let (_, mut f) = s.after.take().unwrap();
            drop(s);
            f();
        }
        Ok(())
    }
}
fn idx(p: h::Pool) -> usize {
    if p == h::Pool::Storage { 0 } else { 1 }
}
impl p::Io for FakePool {
    fn observe(&mut self) -> Result<p::Observation, h::Fault> {
        self.op(Tag::PoolObserve)?;
        Ok(self.0.borrow().observation)
    }
    fn stage(&mut self, _: &p::Call) -> Result<(), h::Fault> {
        self.op(Tag::Stage)
    }
    fn invoke(&mut self, _: &p::Call) -> Result<[u64; 8], h::Fault> {
        self.op(Tag::Invoke)?;
        Ok(self.0.borrow().raw)
    }
    fn complete(&mut self) -> Result<(), h::Fault> {
        self.op(Tag::Complete)
    }
    fn zero(&mut self, p: h::Pool, at: u32, n: usize) -> Result<(), h::Fault> {
        self.op(Tag::Zero)?;
        self.0.borrow_mut().bytes[idx(p)][at as usize..at as usize + n].fill(0);
        Ok(())
    }
    fn read(&mut self, p: h::Pool, at: u32, b: &mut [u8]) -> Result<(), h::Fault> {
        self.op(Tag::PoolRead)?;
        b.copy_from_slice(&self.0.borrow().bytes[idx(p)][at as usize..at as usize + b.len()]);
        Ok(())
    }
    fn write(&mut self, p: h::Pool, at: u32, b: &[u8]) -> Result<(), h::Fault> {
        self.op(Tag::PoolWrite)?;
        self.0.borrow_mut().bytes[idx(p)][at as usize..at as usize + b.len()].copy_from_slice(b);
        Ok(())
    }
}
pub fn pools(f: &FakePool) -> p::Pools<FakePool> {
    p::Pools::attach(
        p::Reservations::validate(1, 0, p::MAPPINGS, &[]).unwrap(),
        f.clone(),
    )
}
pub fn handler_config() -> h::Configuration {
    h::Configuration {
        generation: 1,
        local_node: 1,
        peer_node: 0,
        storage_port: 0x4000,
        memory_port: 0x4001,
        storage: p::STORAGE,
        heap: p::HEAP,
        disks: config().bindings.map(|b| b.disk),
    }
}
pub type Backend = modem_storage_host::qcom_modem_backend::Backend<FakePool, FakeDisk>;
pub fn combined(m: &FakePool, d: &FakeDisk) -> Backend {
    Backend::attach(handler_config(), pools(m), store(d))
}
pub fn grant() -> h::Grant {
    h::Grant {
        serial: 1,
        pool: h::Pool::Storage,
        region: h::Region {
            base: p::STORAGE.base,
            bytes: 4096,
        },
        generation: 1,
        peer: modem_storage_host::qcom_qmi_router::Address { node: 0, port: 9 },
        ownership: h::Ownership::SharedStorage,
    }
}
