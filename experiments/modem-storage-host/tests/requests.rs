mod common;
use common::*;
use modem_storage_host::{
    Files, open_verified,
    qcom_glink_qmi::{Config, Event, Published, Session},
    qcom_modem_backend as b,
    qcom_modem_handlers::{self as h, Fault},
    qcom_modem_services::{Partition, Service},
    qcom_modem_storage::{self as s, Io},
};
use sha2::{Digest as _, Sha256};
use std::{
    cell::RefCell,
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Default)]
struct Checks {
    reads: usize,
    writes: usize,
    flushes: usize,
    max_bytes: usize,
    fail_flush: bool,
}
struct Traced {
    files: Files,
    checks: Rc<RefCell<Checks>>,
}
impl Io for Traced {
    fn observe(&mut self) -> Result<s::Observation, Fault> {
        self.files.observe()
    }
    fn read(&mut self, p: Partition, at: u64, out: &mut [u8]) -> Result<usize, Fault> {
        {
            let mut c = self.checks.borrow_mut();
            c.reads += 1;
            c.max_bytes = c.max_bytes.max(out.len());
        }
        self.files.read(p, at, out)
    }
    fn write(&mut self, p: Partition, at: u64, data: &[u8]) -> Result<usize, Fault> {
        {
            let mut c = self.checks.borrow_mut();
            c.writes += 1;
            c.max_bytes = c.max_bytes.max(data.len());
        }
        self.files.write(p, at, data)
    }
    fn flush(&mut self, p: Partition) -> Result<(), Fault> {
        let mut c = self.checks.borrow_mut();
        c.flushes += 1;
        if c.fail_flush {
            return Err(Fault::Uncertain);
        }
        drop(c);
        self.files.flush(p)
    }
}
type Worker = h::Handlers<b::Backend<FakePool, Traced>>;
struct Rig {
    dir: PathBuf,
    pool: FakePool,
    checks: Rc<RefCell<Checks>>,
    worker: Worker,
    original: [Vec<u8>; 4],
}
impl Rig {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "modem-qmi-private-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let original: [Vec<u8>; 4] =
            std::array::from_fn(|i| (0..16384).map(|at| (at * 17 + i * 31) as u8).collect());
        let mut c = config();
        let files = std::array::from_fn(|i| {
            let path = dir.join(i.to_string());
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            f.write_all(&original[i]).unwrap();
            f.sync_all().unwrap();
            c.bindings[i].source = Sha256::digest(&original[i]).into();
            open_verified(&path, c.bindings[i], c.bindings[i].source).unwrap()
        });
        let checks = Rc::new(RefCell::new(Checks::default()));
        let storage = s::Storage::attach(
            s::Profile::validate(c).unwrap(),
            Traced {
                files: Files::attach(1, [9; 32], files),
                checks: checks.clone(),
            },
        );
        let pool = pool();
        let backend = b::Backend::attach(handler_config(), pools(&pool), storage);
        assert_eq!(backend.error(), None);
        let worker = h::Handlers::attach(
            h::Profile::validate(handler_config(), &[]).unwrap(),
            backend,
        );
        assert_eq!(worker.error(), None);
        Self {
            dir,
            pool,
            checks,
            worker,
            original,
        }
    }
    fn request(
        &mut self,
        service: Service,
        id: u16,
        fields: &[Vec<u8>],
    ) -> Result<h::Response, h::Error> {
        let mut s = Session::new(
            Config {
                local_node: 1,
                peer_node: 0,
                local_channel: 1,
                remote_channel: 2,
            },
            1,
        )
        .unwrap();
        s.register(1, Service::RemoteStorage, 0x4000).unwrap();
        s.register(1, Service::HeapMemory, 0x4001).unwrap();
        let intent = s.offer_receive(1).unwrap();
        s.publish_next(1, |_| Ok(())).unwrap();
        let payload: Vec<u8> = fields.iter().flatten().copied().collect();
        let mut q = vec![0, 0x34, 0x12];
        q.extend(id.to_le_bytes());
        q.extend((payload.len() as u16).to_le_bytes());
        q.extend(payload);
        let mut packet: Vec<u8> = [
            1u32,
            1,
            0,
            9,
            0,
            q.len() as u32,
            1,
            if service == Service::RemoteStorage {
                0x4000
            } else {
                0x4001
            },
        ]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
        packet.extend(q);
        packet.resize((packet.len() + 3) & !3, 0);
        let mut record: Vec<u8> = [0x20009u32, intent, packet.len() as u32, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        record.extend(packet);
        record.resize((record.len() + 7) & !7, 0);
        let Event::Request(handle) = s.ingest(1, &record).unwrap() else {
            panic!("request")
        };
        let (context, request) = s.request(&handle).unwrap();
        self.worker.begin(handle, context, request)?;
        for _ in 0..2048 {
            let count = {
                let c = self.checks.borrow();
                c.reads + c.writes + c.flushes
            };
            let progress = self.worker.step()?;
            let c = self.checks.borrow();
            assert!(c.reads + c.writes + c.flushes - count <= 1);
            assert!(c.max_bytes <= 4096);
            drop(c);
            if progress == h::Progress::ReplyReady {
                let response = self.worker.response().unwrap().clone();
                s.queue_reply(&handle, response.reply()).unwrap();
                self.worker.reply_queued(&handle).unwrap();
                let intents: Vec<u8> = [0x20005u32, 1, 4096, 0]
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect();
                s.ingest(1, &intents).unwrap();
                for _ in 0..4 {
                    let mut out = vec![];
                    let event = s
                        .publish_next(1, |r| {
                            out.extend(r);
                            Ok(())
                        })
                        .unwrap();
                    if matches!(event, Some(Published::Data { .. })) {
                        let body = u32::from_le_bytes(out[36..40].try_into().unwrap()) as usize;
                        let q = modem_storage_host::qcom_qmi::Message::decode(&out[48..48 + body])
                            .unwrap();
                        let result = q.required(2).unwrap();
                        assert_eq!(
                            u16::from_le_bytes(result[..2].try_into().unwrap()),
                            u16::from(response.error().is_some())
                        );
                        assert_eq!(
                            u16::from_le_bytes(result[2..4].try_into().unwrap()),
                            response.error().unwrap_or(0)
                        );
                        return Ok(response);
                    }
                }
                panic!("no encoded reply")
            }
        }
        panic!("bounded request did not finish")
    }
    fn open(&mut self, path: &[u8]) -> u32 {
        let r = self
            .request(Service::RemoteStorage, 1, &[field(1, path)])
            .unwrap();
        assert_eq!(r.error(), None);
        u32::from_le_bytes(r.fields()[3..7].try_into().unwrap())
    }
    fn allocate(&mut self, h: u32) {
        let r = self
            .request(Service::RemoteStorage, 4, &[number(1, h), number(2, 8192)])
            .unwrap();
        assert_eq!(r.error(), None);
        assert_eq!(
            u64::from_le_bytes(r.fields()[3..11].try_into().unwrap()),
            modem_storage_host::qcom_modem_pools::STORAGE.base
        );
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).unwrap();
    }
}
fn field(k: u8, b: &[u8]) -> Vec<u8> {
    let mut v = vec![k];
    v.extend((b.len() as u16).to_le_bytes());
    v.extend(b);
    v
}
fn number(k: u8, v: u32) -> Vec<u8> {
    field(k, &v.to_le_bytes())
}
fn rw(h: u32, write: bool, entries: &[(u32, u32, u32)]) -> Vec<Vec<u8>> {
    let mut v = vec![entries.len() as u8];
    for (sector, offset, sectors) in entries {
        v.extend(sector.to_le_bytes());
        v.extend(
            (modem_storage_host::qcom_modem_pools::STORAGE.base as u32 + offset).to_le_bytes(),
        );
        v.extend(sectors.to_le_bytes());
    }
    vec![
        number(1, h),
        field(2, &[u8::from(write)]),
        field(3, &v),
        field(4, &[0]),
    ]
}

#[test]
fn qmi_read_copies_verified_file_data_through_confirmed_pool_lease() {
    let mut r = Rig::new();
    let h = r.open(b"/boot/modem_fs1");
    r.allocate(h);
    let response = r
        .request(
            Service::RemoteStorage,
            3,
            &rw(h, false, &[(1, 0, 8), (12, 4096, 4)]),
        )
        .unwrap();
    assert_eq!(response.error(), None);
    let pool = r.pool.0.borrow();
    assert_eq!(&pool.bytes[0][..4096], &r.original[0][512..4608]);
    assert_eq!(&pool.bytes[0][4096..6144], &r.original[0][6144..8192]);
    assert_eq!(r.checks.borrow().reads, 2);
    assert_eq!(r.checks.borrow().writes, 0);
}
#[test]
fn qmi_write_uses_private_copies_and_syncs_before_encoded_success() {
    let mut r = Rig::new();
    let h = r.open(b"/boot/modem_fs2");
    r.allocate(h);
    r.pool.0.borrow_mut().bytes[0][..8192].fill(0x61);
    let response = r
        .request(
            Service::RemoteStorage,
            3,
            &rw(h, true, &[(2, 0, 8), (12, 4096, 4)]),
        )
        .unwrap();
    assert_eq!(response.error(), None);
    let bytes = fs::read(r.dir.join("1")).unwrap();
    assert_eq!(&bytes[1024..5120], &[0x61; 4096]);
    assert_eq!(&bytes[6144..8192], &[0x61; 2048]);
    assert_eq!(r.checks.borrow().writes, 2);
    assert_eq!(r.checks.borrow().flushes, 1);
    for i in [0, 2, 3] {
        assert_eq!(fs::read(r.dir.join(i.to_string())).unwrap(), r.original[i]);
    }
}
#[test]
fn qmi_invalid_iovec_fails_before_any_file_effect() {
    let mut r = Rig::new();
    let h = r.open(b"/boot/modem_fs2");
    r.allocate(h);
    let response = r
        .request(
            Service::RemoteStorage,
            3,
            &rw(h, true, &[(0, 0, 1), (100, 512, 1)]),
        )
        .unwrap();
    assert_eq!(response.error(), Some(1));
    assert_eq!(r.checks.borrow().writes, 0);
    assert_eq!(fs::read(r.dir.join("1")).unwrap(), r.original[1]);
}
#[test]
fn qmi_failed_sync_withholds_success_and_retains_the_pool() {
    let mut r = Rig::new();
    let h = r.open(b"/boot/modem_fs2");
    r.allocate(h);
    r.pool.0.borrow_mut().bytes[0][..512].fill(0x92);
    r.checks.borrow_mut().fail_flush = true;
    assert!(
        r.request(Service::RemoteStorage, 3, &rw(h, true, &[(0, 0, 1)]))
            .is_err()
    );
    assert!(r.worker.error().is_some());
    assert!(r.worker.response().is_none());
    assert!(r.worker.leased(h::Pool::Storage));
    assert_eq!(r.checks.borrow().writes, 1);
    assert_eq!(r.checks.borrow().flushes, 1);
    assert_eq!(r.pool.count(Tag::Invoke), 1);
    assert!(r.worker.step().is_err());
    assert_eq!(r.checks.borrow().writes, 1);
}
