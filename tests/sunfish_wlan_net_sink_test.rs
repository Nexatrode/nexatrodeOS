mod sunfish_qcacld_rx_delivery {
    pub trait EthernetRxSink {
        type Error;

        fn receive(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
    }
}

#[path = "../rust/src/sunfish_wlan_net_sink.rs"]
mod sunfish_wlan_net_sink;

use core::ffi::c_void;
use core::ptr::NonNull;

use sunfish_qcacld_rx_delivery::EthernetRxSink;
use sunfish_wlan_net_sink::SddfWlanNetSink;

struct Capture {
    frame: [u8; 64],
    frame_bytes: usize,
    calls: usize,
    status: i32,
}

extern "C" fn capture_rx(bridge_context: *mut c_void, frame: *const u8, frame_bytes: usize) -> i32 {
    if bridge_context.is_null() || frame.is_null() || frame_bytes > 64 {
        return -127;
    }
    // SAFETY: the test binds a live `Capture` for the full sink lifetime and
    // passes a valid borrowed frame; the callback copies the bytes before it
    // returns, matching the production bridge's callback contract.
    let capture = unsafe { &mut *bridge_context.cast::<Capture>() };
    // SAFETY: validated non-null above; the caller supplies `frame_bytes`
    // readable bytes for the duration of this synchronous callback.
    let bytes = unsafe { core::slice::from_raw_parts(frame, frame_bytes) };
    capture.frame[..frame_bytes].copy_from_slice(bytes);
    capture.frame_bytes = frame_bytes;
    capture.calls += 1;
    capture.status
}

#[test]
fn callback_copies_frame_synchronously_and_propagates_bridge_status() {
    let mut capture = Capture {
        frame: [0; 64],
        frame_bytes: 0,
        calls: 0,
        status: 0,
    };
    let bridge_context = NonNull::from(&mut capture).cast::<c_void>();
    // SAFETY: `capture_rx` only accesses the live `Capture` context supplied
    // here and copies the frame before returning.
    let mut sink = unsafe { SddfWlanNetSink::new(bridge_context, capture_rx) };

    let frame: Vec<u8> = (0..60).map(|byte| byte as u8).collect();
    assert_eq!(sink.receive(&frame), Ok(()));
    assert_eq!(capture.calls, 1);
    assert_eq!(capture.frame_bytes, frame.len());
    assert_eq!(&capture.frame[..capture.frame_bytes], frame.as_slice());

    capture.status = -3;
    assert_eq!(sink.receive(&frame), Err(-3));
    assert_eq!(capture.calls, 2);
}
