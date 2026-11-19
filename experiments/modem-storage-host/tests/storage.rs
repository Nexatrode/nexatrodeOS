mod common;
use common::*;
use modem_storage_host::{
    qcom_modem_handlers::{self as h, Backend as _},
    qcom_modem_services::Partition as Part,
    qcom_modem_storage as s,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn profile_rejects_incomplete_reordered_alias_or_invalid_policies() {
    assert!(s::Profile::validate(config()).is_ok());
    for n in 0..10 {
        let mut c = config();
        match n {
            0 => c.generation = 0,
            1 => c.device = [0; 32],
            2 => c.bindings[1].object = c.bindings[0].object,
            3 => c.bindings[0].source = [0; 32],
            4 => c.bindings[0].disk.bytes = 0,
            5 => c.bindings[0].disk.bytes = 513,
            6 => c.bindings[0].disk.bytes = s::MAX_PARTITION_BYTES + 512,
            7 => c.bindings[0].disk.durable = false,
            8 => c.bindings.swap(0, 1),
            _ => c.bindings[0].object = 0,
        };
        assert!(s::Profile::validate(c).is_err());
    }
}
#[test]
fn attach_verifies_every_binding_device_generation_and_ready() {
    for n in 0..8 {
        let f = disk();
        {
            let mut t = f.0.borrow_mut();
            match n {
                0 => t.observation.configuration.generation = 2,
                1 => t.observation.configuration.device = [8; 32],
                2 => t.observation.configuration.bindings[0].source = [8; 32],
                3 => t.observation.configuration.bindings[0].object = 8,
                4 => t.observation.configuration.bindings[0].disk.bytes += 512,
                5 => t.observation.identity = 0,
                6 => t.observation.ready = false,
                _ => t.fault = Some((Tag::Observe, h::Fault::Failed)),
            }
        }
        let mut x = store(&f);
        assert!(x.observe().is_err());
        assert!(x.read(Part::ModemFs1, 0, &mut [0; 512]).is_err());
        assert_eq!(f.count(Tag::Read), 0);
    }
}
#[test]
fn sectors_length_overflow_and_upper_bound_fail_without_io() {
    let f = disk();
    let mut x = store(&f);
    for (at, n) in [
        (0, 0),
        (1, 512),
        (0, 513),
        (0, 4608),
        (16384, 512),
        (u64::MAX - 511, 512),
    ] {
        assert!(x.read(Part::ModemFs1, at, &mut vec![0; n]).is_err());
        assert!(x.write(Part::ModemFs1, at, &vec![0; n]).is_err());
    }
    assert_eq!(f.count(Tag::Read), 0);
    assert_eq!(f.count(Tag::Write), 0);
}
#[test]
fn partitions_have_independent_data_and_dirtiness() {
    let f = disk();
    let mut x = store(&f);
    let before = f.0.borrow().bytes.clone();
    for p in s::PARTITIONS {
        let mut b = [0; 512];
        x.read(p, 512, &mut b).unwrap();
        assert_eq!(&b, &before[s::index(p)][512..1024]);
    }
    x.write(Part::ModemFs1, 4096, &[0x77; 4096]).unwrap();
    assert_eq!(x.status().dirty, [true, false, false, false]);
    assert_eq!(f.0.borrow().bytes[1], before[1]);
    x.flush(Part::ModemFs1).unwrap();
    assert_eq!(x.status().dirty, [false; 4]);
    assert_eq!(f.count(Tag::Write), 1);
    assert_eq!(f.count(Tag::Flush), 1);
}
#[test]
fn readonly_write_is_rejected_even_when_direct() {
    let f = disk();
    let mut x = store(&f);
    assert_eq!(x.write(Part::ModemFsg, 0, &[0; 512]), Err(h::Fault::Failed));
    assert_eq!(f.count(Tag::Write), 0);
}
#[test]
fn nondurable_readonly_cannot_fake_force_sync() {
    let f = disk();
    let mut c = config();
    c.bindings[2].disk.durable = false;
    f.0.borrow_mut().observation.configuration = c;
    let mut x = s::Storage::attach(s::Profile::validate(c).unwrap(), f.clone());
    x.read(Part::ModemFsg, 0, &mut [0; 512]).unwrap();
    assert_eq!(x.flush(Part::ModemFsg), Err(h::Fault::Failed));
    assert_eq!(f.count(Tag::Flush), 0);
}
#[test]
fn short_read_is_failed_and_never_retried() {
    let f = disk();
    let mut x = store(&f);
    f.0.borrow_mut().short = Some(Tag::Read);
    assert_eq!(
        x.read(Part::ModemFs1, 0, &mut [0; 512]),
        Err(h::Fault::Failed)
    );
    assert_eq!(f.count(Tag::Read), 1);
    assert!(x.observe().is_ok());
}
#[test]
fn short_write_latches_dirty_and_prevents_retry_or_sync() {
    let f = disk();
    let mut x = store(&f);
    f.0.borrow_mut().short = Some(Tag::Write);
    assert_eq!(
        x.write(Part::ModemFs1, 0, &[9; 512]),
        Err(h::Fault::Uncertain)
    );
    assert_eq!(&f.0.borrow().bytes[0][..256], &[9; 256]);
    assert!(x.status().dirty[0]);
    assert!(x.write(Part::ModemFs1, 0, &[9; 512]).is_err());
    assert!(x.flush(Part::ModemFs1).is_err());
    assert_eq!(f.count(Tag::Write), 1);
    assert_eq!(f.count(Tag::Flush), 0);
}
#[test]
fn known_write_failure_restores_previous_dirty_state() {
    for dirty in [false, true] {
        let f = disk();
        let mut x = store(&f);
        if dirty {
            x.write(Part::ModemFs1, 0, &[1; 512]).unwrap();
        }
        f.0.borrow_mut().fault = Some((Tag::Write, h::Fault::Failed));
        assert_eq!(
            x.write(Part::ModemFs1, 512, &[2; 512]),
            Err(h::Fault::Failed)
        );
        assert_eq!(x.status().dirty[0], dirty);
        assert!(x.observe().is_ok());
    }
}
#[test]
fn flush_failure_never_clears_dirty_or_reports_ready() {
    for fault in [h::Fault::Failed, h::Fault::Uncertain] {
        let f = disk();
        let mut x = store(&f);
        x.write(Part::ModemFs2, 0, &[3; 512]).unwrap();
        f.0.borrow_mut().fault = Some((Tag::Flush, fault));
        assert_eq!(x.flush(Part::ModemFs2), Err(h::Fault::Uncertain));
        assert!(x.status().dirty[1]);
        assert!(x.observe().is_err());
        assert_eq!(f.count(Tag::Flush), 1);
    }
}
#[test]
fn lifecycle_loss_after_write_or_flush_retains_unacknowledged_dirty_state() {
    for tag in [Tag::Write, Tag::Flush] {
        let f = disk();
        let mut x = store(&f);
        x.write(Part::ModemFs1, 0, &[4; 512]).unwrap();
        f.0.borrow_mut().after = Some((tag, Box::new(|o| o.identity += 1)));
        let r = if tag == Tag::Write {
            x.write(Part::ModemFs1, 512, &[4; 512])
        } else {
            x.flush(Part::ModemFs1)
        };
        assert_eq!(r, Err(h::Fault::Uncertain));
        assert!(x.status().dirty[0]);
        assert!(x.observe().is_err());
    }
}
#[test]
fn changing_binding_source_capacity_policy_or_generation_stops() {
    for n in 0..5 {
        let f = disk();
        let mut x = store(&f);
        {
            let mut v = f.0.borrow_mut();
            match n {
                0 => v.observation.configuration.generation += 1,
                1 => v.observation.configuration.device = [5; 32],
                2 => v.observation.configuration.bindings[0].disk.bytes += 512,
                3 => v.observation.configuration.bindings[0].disk.writable = false,
                _ => v.observation.configuration.bindings[0].source = [6; 32],
            }
        }
        assert!(x.read(Part::ModemFs1, 0, &mut [0; 512]).is_err());
        assert_eq!(f.count(Tag::Read), 0);
    }
}
#[test]
fn unwinding_observe_read_write_or_flush_cannot_repeat() {
    for tag in [Tag::Observe, Tag::Read, Tag::Write, Tag::Flush] {
        let f = disk();
        let mut x = store(&f);
        f.0.borrow_mut().panic = Some(tag);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                match tag {
                    Tag::Observe => {
                        let _ = x.observe();
                    }
                    Tag::Read => {
                        let _ = x.read(Part::ModemFs1, 0, &mut [0; 512]);
                    }
                    Tag::Write => {
                        let _ = x.write(Part::ModemFs1, 0, &[0; 512]);
                    }
                    _ => {
                        let _ = x.flush(Part::ModemFs1);
                    }
                }
            }))
            .is_err()
        );
        let count = f.count(tag);
        assert!(x.observe().is_err());
        assert!(x.write(Part::ModemFs1, 0, &[0; 512]).is_err());
        assert_eq!(f.count(tag), count);
    }
}
#[test]
fn storage_drop_does_not_flush() {
    let f = disk();
    let mut x = store(&f);
    x.write(Part::ModemFs1, 0, &[1; 512]).unwrap();
    drop(x);
    assert_eq!(f.count(Tag::Flush), 0);
}

#[test]
fn combined_attachment_rejects_generation_mapping_and_disk_mismatch() {
    for n in 0..4 {
        let m = pool();
        let d = disk();
        let mut c = handler_config();
        match n {
            0 => c.generation = 2,
            1 => c.storage.base += 4096,
            2 => c.disks[0].bytes += 512,
            _ => c.peer_node = 2,
        };
        let mut x = Backend::attach(c, pools(&m), store(&d));
        assert!(x.error().is_some());
        assert!(x.observe().is_err());
        assert_eq!(m.count(Tag::Invoke), 0);
        assert_eq!(d.count(Tag::Write), 0);
    }
}
#[test]
fn combined_checks_both_resources_before_each_effect() {
    for lose_storage in [true, false] {
        let m = pool();
        let d = disk();
        let mut x = combined(&m, &d);
        assert_eq!(x.error(), None);
        if lose_storage {
            d.0.borrow_mut().observation.ready = false;
        } else {
            m.0.borrow_mut().observation.ready = false;
        }
        assert!(x.zero(h::Pool::Storage, 0, 4096).is_err());
        assert!(x.disk_write(Part::ModemFs1, 0, &[0; 512]).is_err());
        assert_eq!(m.count(Tag::Zero), 0);
        assert_eq!(d.count(Tag::Write), 0);
    }
}
#[test]
fn combined_withholds_receipt_if_storage_dies_after_successful_scm() {
    let m = pool();
    let d = disk();
    let mut x = combined(&m, &d);
    x.zero(h::Pool::Storage, 0, 4096).unwrap();
    let f = d.clone();
    m.0.borrow_mut().after = Some((
        Tag::Complete,
        Box::new(move || f.0.borrow_mut().observation.ready = false),
    ));
    assert!(matches!(x.grant(grant()), Err(h::Fault::Uncertain)));
    assert_eq!(x.pool_status().owners[0], h::Ownership::SharedStorage);
    assert!(x.observe().is_err());
    assert_eq!(m.count(Tag::Invoke), 1);
}
#[test]
fn combined_uncertain_grant_prevents_further_disk_io() {
    let m = pool();
    let d = disk();
    let mut x = combined(&m, &d);
    x.zero(h::Pool::Storage, 0, 4096).unwrap();
    m.0.borrow_mut().raw[0] = 1;
    assert!(x.grant(grant()).is_err());
    assert!(x.disk_read(Part::ModemFs1, 0, &mut [0; 512]).is_err());
    assert_eq!(d.count(Tag::Read), 0);
    assert_eq!(m.count(Tag::Invoke), 1);
}
#[test]
fn combined_pool_loss_during_disk_write_withholds_success() {
    let m = pool();
    let d = disk();
    let mut x = combined(&m, &d);
    let f = m.clone();
    d.0.borrow_mut().after = Some((
        Tag::Write,
        Box::new(move |_| f.0.borrow_mut().observation.ready = false),
    ));
    assert_eq!(
        x.disk_write(Part::ModemFs1, 0, &[7; 512]),
        Err(h::Fault::Uncertain)
    );
    assert!(x.storage_status().dirty[0]);
    assert!(x.sync(Part::ModemFs1).is_err());
    assert_eq!(d.count(Tag::Flush), 0);
}
#[test]
fn combined_write_flush_and_pool_lease_compose_without_mutable_escape() {
    let m = pool();
    let d = disk();
    let mut x = combined(&m, &d);
    x.zero(h::Pool::Storage, 0, 4096).unwrap();
    let r = x.grant(grant()).unwrap();
    x.pool_write(&r, 0, &[8; 512]).unwrap();
    let mut private = [0; 512];
    x.pool_read(&r, 0, &mut private).unwrap();
    x.disk_write(Part::ModemFs2, 512, &private).unwrap();
    assert!(x.storage_status().dirty[1]);
    x.sync(Part::ModemFs2).unwrap();
    assert!(!x.storage_status().dirty[1]);
    assert_eq!(&d.0.borrow().bytes[1][512..1024], &[8; 512]);
    x.reclaim(&r).unwrap();
    assert_eq!(x.pool_status().owners[0], h::Ownership::Host);
    assert_eq!(m.count(Tag::Invoke), 2);
}
#[test]
fn combined_unwind_latches_before_provider_effect_can_repeat() {
    let m = pool();
    let d = disk();
    let mut x = combined(&m, &d);
    d.0.borrow_mut().panic = Some(Tag::Write);
    assert!(
        catch_unwind(AssertUnwindSafe(|| x.disk_write(
            Part::ModemFs1,
            0,
            &[0; 512]
        )))
        .is_err()
    );
    assert!(x.observe().is_err());
    assert!(x.disk_write(Part::ModemFs1, 0, &[0; 512]).is_err());
    assert_eq!(d.count(Tag::Write), 1);
}

#[test]
fn uncertain_read_or_write_is_terminal_without_automatic_flush() {
    for tag in [Tag::Read, Tag::Write] {
        let f = disk();
        let mut x = store(&f);
        f.0.borrow_mut().fault = Some((tag, h::Fault::Uncertain));
        let result = if tag == Tag::Read {
            x.read(Part::ModemFs1, 0, &mut [0; 512])
        } else {
            x.write(Part::ModemFs1, 0, &[0; 512])
        };
        assert_eq!(result, Err(h::Fault::Uncertain));
        assert!(x.observe().is_err());
        assert!(x.flush(Part::ModemFs1).is_err());
        assert_eq!(f.count(Tag::Flush), 0);
    }
}
#[test]
fn combined_rejects_already_dirty_storage_or_an_existing_pool_lease() {
    for active_pool in [false, true] {
        let m = pool();
        let d = disk();
        let mut memory = pools(&m);
        let mut disk = store(&d);
        let retained = if active_pool {
            memory.zero(h::Pool::Storage, 0, 4096).unwrap();
            Some(memory.grant(grant()).unwrap())
        } else {
            disk.write(Part::ModemFs1, 0, &[0; 512]).unwrap();
            None
        };
        let mut x = Backend::attach(handler_config(), memory, disk);
        assert!(x.error().is_some());
        assert!(x.observe().is_err());
        assert_eq!(d.count(Tag::Flush), 0);
        assert_eq!(m.count(Tag::Invoke), usize::from(active_pool));
        if let Some(r) = retained {
            assert!(x.reclaim(&r).is_err());
            assert_eq!(m.count(Tag::Invoke), 1);
        }
    }
}
