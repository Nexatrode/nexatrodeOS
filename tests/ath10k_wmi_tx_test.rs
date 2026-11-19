#![forbid(unsafe_code)]
#![allow(dead_code)]

#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/ath10k_wmi_tx.rs"]
mod ath10k_wmi_tx;

use ath10k_ce::CeError;
use ath10k_wmi_tx::{
    Error, HTC_FLAG_NEED_CREDIT_UPDATE, TxPlan, WmiCommandTxQueue, encode_wmi_command_frame,
    encode_wmi_command_transfer,
};

#[test]
fn encodes_upstream_htc_sequence_wmi_header_and_credit_update() {
    let mut output = [0xa5; 32];
    let plan = encode_wmi_command_frame(
        3,
        64,
        16,
        4,
        true,
        false,
        0x7a,
        0x9000,
        &[0xaa, 0xbb],
        &mut output,
    )
    .unwrap();

    assert_eq!(
        plan,
        TxPlan {
            frame_bytes: 14,
            wmi_payload_bytes: 6,
            credits_required: 1,
            credits_remaining: 3,
            credit_update_requested: true,
            sequence_number: 0x7a,
        }
    );
    assert_eq!(output[0], 3);
    assert_eq!(output[1], HTC_FLAG_NEED_CREDIT_UPDATE);
    assert_eq!(&output[2..4], &[6, 0]);
    assert_eq!(output[4], 0); // Trailer/control byte.
    assert_eq!(output[5], 0x7a);
    assert_eq!(&output[6..8], &[0, 0]); // No bundle padding.
    assert_eq!(&output[8..12], &0x9000u32.to_le_bytes());
    assert_eq!(&output[12..14], &[0xaa, 0xbb]);
    assert!(output[14..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn follows_upstream_credit_update_flag_for_flow_and_bundle_modes() {
    let mut output = [0; 32];
    let bundled =
        encode_wmi_command_frame(1, 64, 16, 4, true, true, 1, 0x9000, &[1], &mut output).unwrap();
    assert!(!bundled.credit_update_requested);
    assert_eq!(output[1], 0);

    let no_flow =
        encode_wmi_command_frame(1, 64, 0, 0, false, false, 2, 0x9000, &[1], &mut output).unwrap();
    assert_eq!(no_flow.credits_required, 0);
    assert_eq!(output[1], 0);
    assert_eq!(output[5], 2);
}

#[test]
fn rejects_invalid_frames_without_modifying_output() {
    let mut output = [0xa5; 32];
    let cases = [
        (
            encode_wmi_command_frame(0, 64, 16, 4, true, false, 1, 0x9000, &[1], &mut output),
            Error::InvalidEndpoint,
        ),
        (
            encode_wmi_command_frame(1, 64, 16, 0, true, false, 1, 0x9000, &[1], &mut output),
            Error::InsufficientCredits,
        ),
        (
            encode_wmi_command_frame(1, 64, 0, 0, true, false, 1, 0x9000, &[1], &mut output),
            Error::ZeroCreditSize,
        ),
        (
            encode_wmi_command_frame(1, 64, 16, 4, true, false, 1, 0x0100_0000, &[1], &mut output),
            Error::InvalidCommandId,
        ),
        (
            encode_wmi_command_frame(1, 64, 16, 4, true, false, 1, 0x9000, &[], &mut output),
            Error::EmptyCommandBody,
        ),
    ];
    for (result, expected) in cases {
        assert_eq!(result, Err(expected));
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }

    assert_eq!(
        encode_wmi_command_frame(
            1,
            64,
            16,
            4,
            true,
            false,
            1,
            0x9000,
            &[1],
            &mut output[..12]
        ),
        Err(Error::OutputTooSmall)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_wmi_command_frame(1, 4, 16, 4, true, false, 1, 0x9000, &[1], &mut output),
        Err(Error::MessageTooLarge)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn composes_wmi_htc_frame_with_wcn3990_ce_descriptor() {
    let mut output = [0xa5; 32];
    let plan = encode_wmi_command_transfer(
        3,
        64,
        16,
        4,
        true,
        false,
        0x7a,
        0x9000,
        &[0xaa, 0xbb],
        0x1_2345_6000,
        0x123,
        false,
        false,
        &mut output,
    )
    .unwrap();

    assert_eq!(plan.tx.frame_bytes, 14);
    assert_eq!(plan.tx.credits_required, 1);
    assert_eq!(plan.tx.sequence_number, 0x7a);
    assert_eq!(
        plan.descriptor.as_bytes(),
        &[
            0x00, 0x60, 0x45, 0x23, 0x01, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x30, 0x12, 0x00, 0x00,
            0x00, 0x00,
        ]
    );
    assert_eq!(output[0], 3);
    assert_eq!(output[5], 0x7a);
    assert_eq!(&output[8..12], &0x9000u32.to_le_bytes());
    assert_eq!(&output[12..14], &[0xaa, 0xbb]);
    assert!(output[14..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn transfer_descriptor_errors_precede_frame_mutation() {
    let mut output = [0xa5; 32];
    assert_eq!(
        encode_wmi_command_transfer(
            3,
            64,
            16,
            4,
            true,
            false,
            1,
            0x9000,
            &[1],
            1 << 35,
            1,
            false,
            false,
            &mut output,
        ),
        Err(Error::CeDescriptor(CeError::AddressOutOfRange))
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_wmi_command_transfer(
            0,
            64,
            16,
            4,
            true,
            false,
            1,
            0x9000,
            &[1],
            1 << 35,
            1,
            false,
            false,
            &mut output,
        ),
        Err(Error::InvalidEndpoint)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    assert_eq!(
        encode_wmi_command_transfer(
            3,
            64,
            16,
            4,
            true,
            false,
            1,
            0x9000,
            &[1],
            0x1000,
            1,
            false,
            false,
            &mut output[..12],
        ),
        Err(Error::OutputTooSmall)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn queued_transfer_commits_credit_ring_and_wrapping_sequence_together() {
    let mut queue = WmiCommandTxQueue::<4>::new(3, 64, 16, 2, true, false, 0xff).unwrap();
    let mut output = [0xa5; 32];
    let queued = queue
        .queue_transfer(
            0x9000,
            &[0xaa, 0xbb],
            0x1000,
            1,
            0x1234,
            false,
            false,
            &mut output,
        )
        .unwrap();

    assert_eq!(queued.slot, 0);
    assert_eq!(queued.transfer.tx.sequence_number, 0xff);
    assert_eq!(queued.transfer.tx.credits_required, 1);
    assert_eq!(queue.credits_available(), Ok(1));
    assert_eq!(queue.next_sequence_number(), 0);
    assert_eq!(queue.ring_indices(), (1, 0));
    assert_eq!(output[5], 0xff);

    let mut contexts = [0; 1];
    assert_eq!(queue.reclaim_to(1, &mut contexts), Ok(1));
    assert_eq!(contexts, [0x1234]);
    assert_eq!(queue.credits_available(), Ok(1));
    assert_eq!(queue.apply_credit_report(1), Ok(()));
    assert_eq!(queue.credits_available(), Ok(2));
}

#[test]
fn queue_failures_follow_htc_credit_and_sequence_ownership() {
    let mut no_credit = WmiCommandTxQueue::<4>::new(3, 64, 16, 0, true, false, 7).unwrap();
    let mut output = [0xa5; 32];
    assert_eq!(
        no_credit.queue_transfer(0x9000, &[1], 0x1000, 1, 1, false, false, &mut output),
        Err(Error::InsufficientCredits)
    );
    assert_eq!(no_credit.next_sequence_number(), 7);
    assert_eq!(no_credit.ring_indices(), (0, 0));
    assert!(output.iter().all(|byte| *byte == 0xa5));

    let mut bad_mapping = WmiCommandTxQueue::<4>::new(3, 64, 16, 2, true, false, 9).unwrap();
    assert_eq!(
        bad_mapping.queue_transfer(0x9000, &[1], 1 << 35, 1, 1, false, false, &mut output),
        Err(Error::CeDescriptor(CeError::AddressOutOfRange))
    );
    assert_eq!(bad_mapping.next_sequence_number(), 10);
    assert_eq!(bad_mapping.credits_available(), Ok(2));
    assert_eq!(bad_mapping.ring_indices(), (0, 0));
    assert!(output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn full_ce_ring_restores_credit_but_does_not_reuse_prepared_sequence() {
    let mut queue = WmiCommandTxQueue::<2>::new(3, 64, 16, 2, true, false, 4).unwrap();
    let mut first = [0; 32];
    queue
        .queue_transfer(0x9000, &[1], 0x1000, 1, 0x10, false, false, &mut first)
        .unwrap();
    assert_eq!(queue.credits_available(), Ok(1));

    let mut second = [0xa5; 32];
    assert_eq!(
        queue.queue_transfer(0x9000, &[2], 0x2000, 2, 0x20, false, false, &mut second),
        Err(Error::TxRing(CeError::RingFull))
    );
    assert_eq!(queue.next_sequence_number(), 6);
    assert_eq!(queue.credits_available(), Ok(1));
    assert_eq!(queue.ring_indices(), (1, 0));
    assert_eq!(second[5], 5);

    let mut contexts = [0; 1];
    assert_eq!(queue.reclaim_to(1, &mut contexts), Ok(1));
    assert_eq!(contexts, [0x10]);
    assert_eq!(queue.credits_available(), Ok(1));
}
