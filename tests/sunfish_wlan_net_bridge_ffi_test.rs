#![allow(dead_code)]

mod sunfish_smmu_lpae {
    pub const SMMU_LPAE_PAGE_BYTES: u64 = 4096;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct LpaePageMapping {
        pub iova_page: u64,
        pub physical_page: u64,
    }
}

#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_delivery.rs"]
mod sunfish_qcacld_rx_delivery;
#[path = "../rust/src/sunfish_qcacld_rx_ind.rs"]
mod sunfish_qcacld_rx_ind;
#[path = "../rust/src/sunfish_qcacld_rx_pipeline.rs"]
mod sunfish_qcacld_rx_pipeline;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_qcacld_rx_transport.rs"]
mod sunfish_qcacld_rx_transport;

#[path = "../rust/src/sunfish_wlan_net_sink.rs"]
mod sunfish_wlan_net_sink;
#[path = "../rust/src/sunfish_wlan_net_source.rs"]
mod sunfish_wlan_net_source;

use core::ffi::c_void;
use core::ptr::NonNull;

use qcom_htc::HtcReady;
use sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use sunfish_htt_rx_ring::SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS;
use sunfish_qcacld_htc::{HttEndpoint, parse_connect_response};
use sunfish_qcacld_rx::ParseOptions;
use sunfish_qcacld_rx_delivery::EthernetRxSink;
use sunfish_qcacld_rx_ind::{HTT_RX_IND_MPDU_STATUS_OK, HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER};
use sunfish_qcacld_rx_session::SunfishQcacldRxSession;
use sunfish_wlan_net_sink::{SddfRxSinkCallback, SddfWlanNetSink};
use sunfish_wlan_net_source::{Error as TxSourceError, SddfTxSourceCallback, SddfWlanNetSource};

unsafe extern "C" {
    fn sunfish_test_bridge_new() -> *mut c_void;
    fn sunfish_test_bridge_destroy(context: *mut c_void);
    fn sunfish_test_bridge_rx_callback(
        context: *mut c_void,
        frame: *const u8,
        frame_bytes: usize,
    ) -> i32;
    fn sunfish_test_bridge_tx_callback(
        context: *mut c_void,
        output: *mut u8,
        output_capacity: usize,
        frame_bytes: *mut usize,
    ) -> i32;
    fn sunfish_test_bridge_queue_tx(
        context: *mut c_void,
        frame: *const u8,
        frame_bytes: usize,
    ) -> i32;
    fn sunfish_test_bridge_rx_active_count(context: *mut c_void) -> u32;
    fn sunfish_test_bridge_rx_free_count(context: *mut c_void) -> u32;
    fn sunfish_test_bridge_tx_active_count(context: *mut c_void) -> u32;
    fn sunfish_test_bridge_tx_free_count(context: *mut c_void) -> u32;
    fn sunfish_test_bridge_notification_count(context: *mut c_void) -> u32;
    fn sunfish_test_bridge_last_notification_channel(context: *mut c_void) -> u8;
    fn sunfish_test_bridge_copy_rx(
        context: *mut c_void,
        index: u32,
        output: *mut u8,
        capacity: usize,
        bytes: *mut usize,
    ) -> i32;
}

struct Fixture(*mut c_void);

extern "C" fn bridge_rx_callback(
    context: *mut c_void,
    frame: *const u8,
    frame_bytes: usize,
) -> i32 {
    // SAFETY: the sink keeps the fixture alive for the synchronous callback,
    // and `receive` supplies a valid borrowed frame for `frame_bytes`.
    unsafe { sunfish_test_bridge_rx_callback(context, frame, frame_bytes) }
}

extern "C" fn bridge_tx_callback(
    context: *mut c_void,
    output: *mut u8,
    output_capacity: usize,
    frame_bytes: *mut usize,
) -> i32 {
    // SAFETY: the source keeps the fixture alive for the synchronous callback;
    // its output slice and length pointer are valid for this call.
    unsafe { sunfish_test_bridge_tx_callback(context, output, output_capacity, frame_bytes) }
}

impl Fixture {
    fn new() -> Self {
        // SAFETY: the C fixture owns all queue and buffer storage until Drop.
        let context = unsafe { sunfish_test_bridge_new() };
        assert!(!context.is_null());
        Self(context)
    }

    fn sink(&mut self) -> SddfWlanNetSink {
        // SAFETY: the fixture outlives this sink in each test, and its C
        // callback copies the borrowed Ethernet frame before returning.
        unsafe {
            SddfWlanNetSink::new(
                NonNull::new(self.0).unwrap(),
                bridge_rx_callback as SddfRxSinkCallback,
            )
        }
    }

    fn source(&mut self) -> SddfWlanNetSource {
        // SAFETY: the fixture outlives this source in each test, and its C
        // callback validates/copies before returning the shared buffer.
        unsafe {
            SddfWlanNetSource::new(
                NonNull::new(self.0).unwrap(),
                bridge_tx_callback as SddfTxSourceCallback,
            )
        }
    }

    fn enqueue_tx(&self, frame: &[u8]) -> i32 {
        // SAFETY: the fixture is live and the helper copies this borrowed frame
        // into its owned TX test region before publishing the descriptor.
        unsafe { sunfish_test_bridge_queue_tx(self.0, frame.as_ptr(), frame.len()) }
    }

    fn active_count(&self) -> u32 {
        // SAFETY: the fixture owns this live context.
        unsafe { sunfish_test_bridge_rx_active_count(self.0) }
    }

    fn free_count(&self) -> u32 {
        // SAFETY: the fixture owns this live context.
        unsafe { sunfish_test_bridge_rx_free_count(self.0) }
    }

    fn tx_active_count(&self) -> u32 {
        // SAFETY: the fixture owns this live context.
        unsafe { sunfish_test_bridge_tx_active_count(self.0) }
    }

    fn tx_free_count(&self) -> u32 {
        // SAFETY: the fixture owns this live context.
        unsafe { sunfish_test_bridge_tx_free_count(self.0) }
    }

    fn notification_count(&self) -> u32 {
        // SAFETY: the fixture owns this live context.
        unsafe { sunfish_test_bridge_notification_count(self.0) }
    }

    fn last_notification_channel(&self) -> u8 {
        // SAFETY: the fixture owns this live context.
        unsafe { sunfish_test_bridge_last_notification_channel(self.0) }
    }

    fn rx_frame(&self, index: u32) -> Vec<u8> {
        let mut output = vec![0u8; 2048];
        let mut bytes = 0;
        // SAFETY: output is writable for its full capacity and the C helper
        // copies only the selected active descriptor's bounded frame.
        let status = unsafe {
            sunfish_test_bridge_copy_rx(
                self.0,
                index,
                output.as_mut_ptr(),
                output.len(),
                &mut bytes,
            )
        };
        assert_eq!(status, 0);
        output.truncate(bytes);
        output
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: the fixture was created by `sunfish_test_bridge_new` and is
        // destroyed exactly once after all sink borrows end.
        unsafe { sunfish_test_bridge_destroy(self.0) };
    }
}

fn ethernet_frame(ethertype: [u8; 2], seed: u8) -> Vec<u8> {
    let mut frame = (0..64).map(|n| seed.wrapping_add(n)).collect::<Vec<_>>();
    frame[12..14].copy_from_slice(&ethertype);
    frame
}

fn put_word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn endpoint() -> HttEndpoint {
    let response = [
        3, 0, 0, 3, // HTC connect response, HTT service
        0, 5, 0, 2, // success, endpoint 5, 512-byte maximum
        0, 0, 0, 0,
    ];
    parse_connect_response(
        &response,
        HtcReady {
            credit_count: 4,
            credit_size: 64,
            max_endpoints: 9,
            htc_version: 1,
            max_messages_per_bundle: 1,
            alternate_data_credit_size: 0,
        },
    )
    .unwrap()
}

fn three_mpdu_indication() -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[0] = 1;
    put_word(&mut bytes, 4, 3 << 24);
    put_word(&mut bytes, 44, 3);
    bytes[48..51].copy_from_slice(&[0, 0, 1]);
    put_word(
        &mut bytes,
        52,
        1 | (u32::from(HTT_RX_IND_MPDU_STATUS_OK) << 8),
    );
    put_word(
        &mut bytes,
        56,
        1 | (u32::from(HTT_RX_IND_MPDU_STATUS_UNAUTH_PEER) << 8),
    );
    put_word(
        &mut bytes,
        60,
        1 | (u32::from(HTT_RX_IND_MPDU_STATUS_OK) << 8),
    );
    bytes
}

fn completed_msdu(payload: &[u8], first_msdu: bool, last_msdu: bool) -> Vec<u8> {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let mut buffer = vec![0; profile.rx_buffer_bytes() as usize];
    let attention = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words) * 4;
    let msdu_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_start_words) * 4;
    let msdu_end = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.msdu_end_words) * 4;
    put_word(&mut buffer, attention, 1 << 31);
    put_word(&mut buffer, msdu_start, payload.len() as u32);
    put_word(&mut buffer, msdu_start + 8, 2 << 8);
    put_word(
        &mut buffer,
        msdu_end + 16,
        (u32::from(first_msdu) << 14) | (u32::from(last_msdu) << 15),
    );
    let payload_start = SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes as usize;
    buffer[payload_start..payload_start + payload.len()].copy_from_slice(payload);
    buffer
}

#[test]
fn rust_qcacld_sink_publishes_frame_through_the_real_sddf_c_bridge() {
    let mut fixture = Fixture::new();
    let ip = ethernet_frame([0x08, 0x00], 0x20);
    let eapol = ethernet_frame([0x88, 0x8e], 0x40);
    let discarded = ethernet_frame([0x08, 0x00], 0x60);
    let owned = [
        completed_msdu(&ip, true, true),
        completed_msdu(&eapol, true, true),
        completed_msdu(&discarded, true, true),
    ];
    let buffers = owned.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let indication_bytes = three_mpdu_indication();
    let mut transport_frame = vec![0u8; 8 + indication_bytes.len()];
    transport_frame[0] = 5;
    transport_frame[2..4].copy_from_slice(&(indication_bytes.len() as u16).to_le_bytes());
    transport_frame[5] = 0xa4;
    transport_frame[8..].copy_from_slice(&indication_bytes);
    let mut session = SunfishQcacldRxSession::<8>::new().unwrap();
    for buffer_id in 0..3 {
        session.post(buffer_id).unwrap();
    }
    let mut msdus = [None; 3];
    let mut leases = [None; 3];
    let mut transport_scratch = [0; 80];
    let mut frame_scratch = [0; 64];
    let mut recycled = [u32::MAX; 8];
    let mut synchronized = Vec::new();
    {
        let mut sink = fixture.sink();
        let report = sunfish_qcacld_rx_pipeline::consume_and_deliver_with_cpu_sync_and_collect(
            endpoint(),
            &mut transport_scratch,
            &mut session,
            &buffers,
            ParseOptions::default(),
            &mut msdus,
            &mut leases,
            &mut frame_scratch,
            &mut sink,
            |buffer_id| {
                synchronized.push(buffer_id);
                Ok::<(), ()>(())
            },
            &mut recycled,
            |destination| {
                destination[..transport_frame.len()].copy_from_slice(&transport_frame);
                Ok::<_, ()>(qcom_ce_v3::RxCompletion {
                    address: 0x9000,
                    nbytes: transport_frame.len() as u16,
                    transfer_id: 7,
                    byte_swapped: false,
                })
            },
        )
        .unwrap();
        assert_eq!(report.pipeline.sequence, 0xa4);
        assert_eq!(report.pipeline.delivery.mpdu_count, 3);
        assert_eq!(report.pipeline.delivery.delivered_msdus, 2);
        assert_eq!(report.pipeline.delivery.filtered_msdus, 1);
        assert_eq!(report.recycled_buffer_count, 3);
    }

    assert_eq!(fixture.active_count(), 2);
    assert_eq!(fixture.free_count(), 0);
    // sDDF coalesces notifications until its consumer clears the signal bit.
    assert_eq!(fixture.notification_count(), 1);
    assert_eq!(fixture.rx_frame(0), ip);
    assert_eq!(fixture.rx_frame(1), eapol);
    assert_eq!(synchronized, [0, 1, 2]);
    assert_eq!(&recycled[..3], [0, 1, 2]);
    assert_eq!(session.occupied_count(), 0);
    assert_eq!(session.posted_count(), 0);

    let backpressure = ethernet_frame([0x08, 0x00], 0x80);
    let mut sink = fixture.sink();
    assert_eq!(sink.receive(&backpressure), Err(2)); // BACKPRESSURE
    assert_eq!(fixture.active_count(), 2);
    assert_eq!(fixture.notification_count(), 1);
}

#[test]
fn rust_wlan_source_copies_before_sddf_bridge_recycles_tx_buffer() {
    let mut fixture = Fixture::new();
    let mut source = fixture.source();
    let mut output = [0xa5; 128];

    assert_eq!(source.next_frame(&mut output), Ok(None));
    let packet = ethernet_frame([0x08, 0x00], 0x70);
    assert_eq!(fixture.enqueue_tx(&packet), 0);
    assert_eq!(fixture.tx_active_count(), 1);

    let mut too_small = [0x5a; 63];
    assert_eq!(
        source.next_frame(&mut too_small),
        Err(TxSourceError::Bridge(7)) // SUNFISH_WLAN_NET_OUTPUT_TOO_SMALL
    );
    assert_eq!(too_small, [0x5a; 63]);
    assert_eq!(fixture.tx_active_count(), 1);
    assert_eq!(fixture.tx_free_count(), 0);
    assert_eq!(fixture.notification_count(), 0);

    assert_eq!(source.next_frame(&mut output), Ok(Some(packet.len())));
    assert_eq!(&output[..packet.len()], packet.as_slice());
    assert_eq!(fixture.tx_active_count(), 0);
    assert_eq!(fixture.tx_free_count(), 1);
    assert_eq!(fixture.notification_count(), 1);
    assert_eq!(fixture.last_notification_channel(), 19);
    assert_eq!(source.next_frame(&mut output), Ok(None));
}
