//! Deterministic host buffers and simulated durability/ownership, never a device.
use crate::qcom_modem_handlers::*;
use crate::qcom_modem_services::Partition;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tag {
    Observe,
    Info,
    DiskInfo,
    Zero,
    Grant,
    Reclaim,
    DiskRead,
    DiskWrite,
    Sync,
    PoolRead,
    PoolWrite,
}
#[derive(Clone, Debug)]
pub struct Op {
    pub tag: Tag,
    pub pool: Option<Pool>,
    pub offset: u64,
    pub bytes: usize,
}
pub struct State {
    pub persistence: Persistence,
    pub config: Configuration,
    pub observation: Observation,
    pub disks: [Vec<u8>; 4],
    pub committed: [Vec<u8>; 4],
    pub storage: Vec<u8>,
    pub heap: Vec<u8>,
    pub grants: [Option<Grant>; 2],
    pub log: Vec<Op>,
    pub fail: Option<(Tag, Fault)>,
    pub panic: Option<Tag>,
    pub lie_grant: bool,
    pub lie_reclaim: bool,
    pub corrupt_info: bool,
    pub after: Option<Box<dyn FnMut(Tag)>>,
}
#[derive(Clone)]
pub struct FakeBackend(pub Rc<RefCell<State>>);
pub fn partition(p: Partition) -> usize {
    match p {
        Partition::ModemFs1 => 0,
        Partition::ModemFs2 => 1,
        Partition::ModemFsg => 2,
        Partition::ModemFsc => 3,
    }
}
pub fn pool(p: Pool) -> usize {
    if p == Pool::Storage { 0 } else { 1 }
}
pub fn config(storage: u32, heap: u32, disk_bytes: u64) -> Configuration {
    Configuration {
        generation: 1,
        local_node: 1,
        peer_node: 0,
        storage_port: 0x4000,
        memory_port: 0x4001,
        storage: Region {
            base: 0xa1001000,
            bytes: storage,
        },
        heap: Region {
            base: 0xa2001000,
            bytes: heap,
        },
        disks: std::array::from_fn(|i| Disk {
            bytes: disk_bytes,
            writable: i != 2,
            durable: i != 2,
        }),
    }
}
pub fn backend(c: Configuration) -> FakeBackend {
    let disks: [Vec<u8>; 4] = std::array::from_fn(|i| {
        (0..c.disks[i].bytes as usize)
            .map(|at| (at.wrapping_mul(17) + i * 31) as u8)
            .collect()
    });
    FakeBackend(Rc::new(RefCell::new(State {
        persistence: Persistence::Durable,
        config: c,
        observation: Observation {
            generation: c.generation,
            identity: 71,
            ready: true,
            storage: Ownership::Host,
            heap: Ownership::Host,
        },
        committed: disks.clone(),
        disks,
        storage: vec![0xa5; c.storage.bytes as usize],
        heap: vec![0x5a; c.heap.bytes as usize],
        grants: [None; 2],
        log: Vec::new(),
        fail: None,
        panic: None,
        lie_grant: false,
        lie_reclaim: false,
        corrupt_info: false,
        after: None,
    })))
}
impl FakeBackend {
    pub fn action_count(&self) -> usize {
        self.0
            .borrow()
            .log
            .iter()
            .filter(|op| !matches!(op.tag, Tag::Observe | Tag::Info | Tag::DiskInfo))
            .count()
    }
    pub fn count(&self, tag: Tag) -> usize {
        self.0
            .borrow()
            .log
            .iter()
            .filter(|op| op.tag == tag)
            .count()
    }
    pub fn clear(&self) {
        self.0.borrow_mut().log.clear();
    }
    fn before(&self, tag: Tag, p: Option<Pool>, offset: u64, bytes: usize) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        s.log.push(Op {
            tag,
            pool: p,
            offset,
            bytes,
        });
        if s.panic == Some(tag) {
            panic!("injected backend unwind {tag:?}");
        }
        if let Some((f, error)) = s.fail {
            if f == tag {
                s.fail = None;
                if error == Fault::Uncertain {
                    if let Some(p) = p {
                        if p == Pool::Storage {
                            s.observation.storage = Ownership::Unknown;
                        } else {
                            s.observation.heap = Ownership::Unknown;
                        }
                    }
                }
                return Err(error);
            }
        }
        Ok(())
    }
    fn after(&self, tag: Tag) {
        if let Some(callback) = self.0.borrow_mut().after.as_mut() {
            callback(tag);
        }
    }
}
impl Backend for FakeBackend {
    fn persistence(&self) -> Persistence {
        self.0.borrow().persistence
    }
    type Receipt = Grant;
    fn observe(&mut self) -> Result<Observation, Fault> {
        self.before(Tag::Observe, None, 0, 0)?;
        Ok(self.0.borrow().observation)
    }
    fn disk(&mut self, p: Partition) -> Result<Disk, Fault> {
        self.before(Tag::DiskInfo, None, 0, 0)?;
        Ok(self.0.borrow().config.disks[partition(p)])
    }
    fn zero(&mut self, p: Pool, offset: u32, bytes: usize) -> Result<(), Fault> {
        self.before(Tag::Zero, Some(p), u64::from(offset), bytes)?;
        assert!(bytes <= IO_BYTES);
        {
            let mut s = self.0.borrow_mut();
            let state = if p == Pool::Storage {
                s.observation.storage
            } else {
                s.observation.heap
            };
            assert_eq!(state, Ownership::Host);
            let b = if p == Pool::Storage {
                &mut s.storage
            } else {
                &mut s.heap
            };
            b[offset as usize..offset as usize + bytes].fill(0);
        }
        self.after(Tag::Zero);
        Ok(())
    }
    fn grant(&mut self, g: Grant) -> Result<Self::Receipt, Fault> {
        self.before(
            Tag::Grant,
            Some(g.pool),
            g.region.base,
            g.region.bytes as usize,
        )?;
        {
            let mut s = self.0.borrow_mut();
            let region = if g.pool == Pool::Storage {
                s.config.storage
            } else {
                s.config.heap
            };
            assert!(
                g.region.base >= region.base
                    && g.region.base + u64::from(g.region.bytes)
                        <= region.base + u64::from(region.bytes)
            );
            let at = (g.region.base - region.base) as usize;
            let b = if g.pool == Pool::Storage {
                &s.storage
            } else {
                &s.heap
            };
            assert!(
                b[at..at + g.region.bytes as usize].iter().all(|b| *b == 0),
                "must zero before grant"
            );
            s.grants[pool(g.pool)] = Some(g);
            if g.pool == Pool::Storage {
                s.observation.storage = Ownership::SharedStorage;
            } else {
                s.observation.heap = Ownership::ModemExclusive;
            }
        }
        self.after(Tag::Grant);
        let mut receipt = g;
        if self.0.borrow().lie_grant {
            receipt.serial += 1;
        }
        Ok(receipt)
    }
    fn info(&mut self, r: &Self::Receipt) -> Result<Grant, Fault> {
        self.before(Tag::Info, Some(r.pool), 0, 0)?;
        let s = self.0.borrow();
        let mut g = s.grants[pool(r.pool)].ok_or(Fault::Failed)?;
        if s.corrupt_info {
            g.region.base += 4096;
        }
        if *r != s.grants[pool(r.pool)].unwrap() {
            return Err(Fault::Failed);
        }
        Ok(g)
    }
    fn reclaim(&mut self, r: &Self::Receipt) -> Result<(), Fault> {
        self.before(
            Tag::Reclaim,
            Some(r.pool),
            r.region.base,
            r.region.bytes as usize,
        )?;
        {
            let mut s = self.0.borrow_mut();
            assert_eq!(s.grants[pool(r.pool)], Some(*r));
            if !s.lie_reclaim {
                s.grants[pool(r.pool)] = None;
                if r.pool == Pool::Storage {
                    s.observation.storage = Ownership::Host;
                } else {
                    s.observation.heap = Ownership::Host;
                }
            }
        }
        self.after(Tag::Reclaim);
        Ok(())
    }
    fn disk_read(&mut self, p: Partition, offset: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.before(Tag::DiskRead, None, offset, out.len())?;
        assert!(out.len() <= IO_BYTES);
        out.copy_from_slice(
            &self.0.borrow().disks[partition(p)][offset as usize..offset as usize + out.len()],
        );
        self.after(Tag::DiskRead);
        Ok(())
    }
    fn disk_write(&mut self, p: Partition, offset: u64, bytes: &[u8]) -> Result<(), Fault> {
        self.before(Tag::DiskWrite, None, offset, bytes.len())?;
        assert!(bytes.len() <= IO_BYTES);
        self.0.borrow_mut().disks[partition(p)][offset as usize..offset as usize + bytes.len()]
            .copy_from_slice(bytes);
        self.after(Tag::DiskWrite);
        Ok(())
    }
    fn sync(&mut self, p: Partition) -> Result<(), Fault> {
        self.before(Tag::Sync, None, 0, 0)?;
        {
            let mut s = self.0.borrow_mut();
            s.committed[partition(p)] = s.disks[partition(p)].clone();
        }
        self.after(Tag::Sync);
        Ok(())
    }
    fn pool_read(&mut self, r: &Self::Receipt, offset: u32, out: &mut [u8]) -> Result<(), Fault> {
        self.before(Tag::PoolRead, Some(r.pool), u64::from(offset), out.len())?;
        assert_eq!(r.pool, Pool::Storage);
        assert!(
            out.len() <= IO_BYTES
                && u64::from(offset) + out.len() as u64 <= u64::from(r.region.bytes)
        );
        out.copy_from_slice(&self.0.borrow().storage[offset as usize..offset as usize + out.len()]);
        self.after(Tag::PoolRead);
        Ok(())
    }
    fn pool_write(&mut self, r: &Self::Receipt, offset: u32, bytes: &[u8]) -> Result<(), Fault> {
        self.before(Tag::PoolWrite, Some(r.pool), u64::from(offset), bytes.len())?;
        assert_eq!(r.pool, Pool::Storage);
        assert!(
            bytes.len() <= IO_BYTES
                && u64::from(offset) + bytes.len() as u64 <= u64::from(r.region.bytes)
        );
        self.0.borrow_mut().storage[offset as usize..offset as usize + bytes.len()]
            .copy_from_slice(bytes);
        self.after(Tag::PoolWrite);
        Ok(())
    }
}
