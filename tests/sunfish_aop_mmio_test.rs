//! Exercise the actual Pixel MMIO adapter against canary-filled host RAM.
//! This checks address/write scope, not AOP firmware behavior or bus ordering.

extern crate self as personal_os_rust_server;

#[path = "../rust/src/pixel_aop_mailbox.rs"]
mod pixel_aop_mailbox;
#[path = "../rust/src/qcom_qmp.rs"]
mod qcom_qmp;
#[path = "../rust/src/qcom_qmp_link.rs"]
mod qcom_qmp_link;
#[path = "../rust/src/qcom_qmp_tx.rs"]
mod qcom_qmp_tx;

use pixel_aop_mailbox::Mailbox;
use qcom_qmp::{Descriptor, QMP_MAGIC, QMP_UP};
use qcom_qmp_link::ApWrite;
use qcom_qmp_tx::{OffNotification, OnNotification};

fn connected_ram() -> [u32; 1024] {
    let mut ram = [0xa5a5_a5a5; 1024];
    ram[..16].copy_from_slice(&[
        QMP_MAGIC, 1, 0, QMP_UP, QMP_UP, QMP_UP, QMP_UP, 0x68, 0xc0, QMP_UP, QMP_UP, QMP_UP,
        QMP_UP, 0x2d8, 0x128, 0,
    ]);
    ram[0xc0 / 4] = 0;
    ram[0x128 / 4] = 0;
    ram
}

fn off_packet() -> qcom_qmp_tx::ModemOffPacket {
    let d = Descriptor::from_words(&connected_ram()[..16].try_into().unwrap());
    OffNotification::new(d, 4096, 10, 0, 0)
        .unwrap()
        .start(d, 0, 0)
        .unwrap()
}

#[test]
fn actual_adapter_changes_only_fixed_tx_packet_and_one_doorbell_register() {
    let mut ram = connected_ram();
    let before = ram;
    let mut bell = [0x5a5a_5a5a; 1024];
    let mailbox = unsafe { Mailbox::new(ram.as_mut_ptr(), bell.as_mut_ptr()) };
    let packet = off_packet();
    mailbox.publish_off(&packet).unwrap();
    let start = 0x128 / 4;
    for index in 0..ram.len() {
        if (start..start + 25).contains(&index) {
            assert_eq!(ram[index], packet.words()[index - start].to_le());
        } else {
            assert_eq!(
                ram[index], before[index],
                "unexpected MSGRAM write at word {index}"
            );
        }
    }
    for (index, value) in bell.iter().enumerate() {
        assert_eq!(*value, if index == 3 { 1 } else { 0x5a5a_5a5a });
    }
    let snapshot = mailbox.snapshot().unwrap();
    assert_eq!(snapshot.receive_length, 0);
    assert_eq!(snapshot.transmit_length, 96);
}

#[test]
fn actual_on_publication_has_the_same_fixed_footprint_and_refuses_busy_state() {
    for busy in [false, true] {
        let mut ram = connected_ram();
        if busy {
            ram[0x128 / 4] = 96;
        }
        let before = ram;
        let mut bell = [0x5a5a_5a5a; 1024];
        let mailbox = unsafe { Mailbox::new(ram.as_mut_ptr(), bell.as_mut_ptr()) };
        let descriptor = Descriptor::from_words(&connected_ram()[..16].try_into().unwrap());
        let packet = OnNotification::new(descriptor, 4096, 1, 0, 0)
            .unwrap()
            .start(descriptor, 0, 0)
            .unwrap();
        let r = mailbox.publish_on(&packet);
        if busy {
            assert_eq!(r, Err(0x31));
            assert_eq!(ram, before);
            assert!(bell.iter().all(|v| *v == 0x5a5a_5a5a));
        } else {
            assert_eq!(r, Ok(()));
            for i in 0..ram.len() {
                let start = 0x128 / 4;
                assert_eq!(
                    ram[i],
                    if (start..start + 25).contains(&i) {
                        packet.words()[i - start]
                    } else {
                        before[i]
                    }
                );
            }
            for (i, v) in bell.iter().enumerate() {
                assert_eq!(*v, if i == 3 { 1 } else { 0x5a5a_5a5a });
            }
        }
    }
}

#[test]
fn publication_refuses_busy_rx_tx_and_changed_layout_without_any_write() {
    let packet = off_packet();
    for (index, value, error) in [
        (0xc0 / 4, 4, 0x31),
        (0x128 / 4, 96, 0x31),
        (11, qcom_qmp::QMP_DOWN, 0x31),
        (14, 0x400, 0x12),
        (1, 2, 0x11),
    ] {
        let mut ram = connected_ram();
        ram[index] = value;
        let before = ram;
        let mut bell = [0x5a5a_5a5a; 1024];
        let mailbox = unsafe { Mailbox::new(ram.as_mut_ptr(), bell.as_mut_ptr()) };
        assert_eq!(mailbox.publish_off(&packet), Err(error));
        assert_eq!(ram, before);
        assert!(bell.iter().all(|word| *word == 0x5a5a_5a5a));
    }
}

#[test]
fn handshake_writes_touch_only_the_named_ap_owned_descriptor_fields() {
    let mut ram = connected_ram();
    let before = ram;
    let mut bell = [0x5a5a_5a5a; 1024];
    let mailbox = unsafe { Mailbox::new(ram.as_mut_ptr(), bell.as_mut_ptr()) };
    mailbox.notify([Some(ApWrite::RemoteLinkAck), Some(ApWrite::LocalLink)]);
    mailbox.notify([
        Some(ApWrite::RemoteChannelAck(qcom_qmp::QMP_DOWN)),
        Some(ApWrite::LocalChannel(qcom_qmp::QMP_DOWN)),
    ]);
    for index in 0..ram.len() {
        assert_eq!(
            ram[index],
            match index {
                4 | 9 => QMP_UP,
                6 | 11 => qcom_qmp::QMP_DOWN,
                _ => before[index],
            }
        );
    }
    for (index, value) in bell.iter().enumerate() {
        assert_eq!(*value, if index == 3 { 1 } else { 0x5a5a_5a5a });
    }
}
