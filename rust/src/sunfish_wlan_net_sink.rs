//! Adapt the synchronous sDDF Ethernet RX bridge callback to the Sunfish
//! QCACLD receive pipeline.
//!
//! The callback ABI is implemented by `sunfish_wlan_net_bridge_rx_sink` in
//! the platform C bridge. Construction is unsafe because the bridge pointer
//! and callback must remain valid for the sink's lifetime and the callback
//! must synchronously copy or reject the borrowed frame.
#![deny(unsafe_op_in_unsafe_fn)]

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::sunfish_qcacld_rx_delivery::EthernetRxSink;

pub type SddfRxSinkCallback = extern "C" fn(*mut c_void, *const u8, usize) -> i32;

pub struct SddfWlanNetSink {
    bridge_context: NonNull<c_void>,
    callback: SddfRxSinkCallback,
}

impl SddfWlanNetSink {
    /// Bind the platform bridge callback to the Ethernet receive pipeline.
    ///
    /// # Safety
    /// `bridge_context` must point to a live, exclusively-owned
    /// `sunfish_wlan_net_bridge_t` accepted by `callback`. The context and
    /// callback must remain valid until this sink is no longer used. The
    /// callback may not retain the frame pointer after it returns.
    pub unsafe fn new(bridge_context: NonNull<c_void>, callback: SddfRxSinkCallback) -> Self {
        Self {
            bridge_context,
            callback,
        }
    }
}

impl EthernetRxSink for SddfWlanNetSink {
    type Error = i32;

    fn receive(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        let status = (self.callback)(self.bridge_context.as_ptr(), frame.as_ptr(), frame.len());
        if status == 0 { Ok(()) } else { Err(status) }
    }
}
