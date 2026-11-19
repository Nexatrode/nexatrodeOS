//! Adapt the sDDF Ethernet TX queue to the Sunfish WLAN transmit path.
//!
//! The callback synchronously copies one frame into caller-owned staging
//! storage. On success the C bridge returns the shared sDDF buffer to its free
//! queue; the WLAN owner then retains only the private copy while it frames,
//! encrypts, and schedules the packet.
#![deny(unsafe_op_in_unsafe_fn)]

use core::ffi::c_void;
use core::ptr::NonNull;

const ETHERNET_HEADER_BYTES: usize = 14;
const SUNFISH_WLAN_NET_OK: i32 = 0;
const SUNFISH_WLAN_NET_IDLE: i32 = 1;

pub type SddfTxSourceCallback = extern "C" fn(*mut c_void, *mut u8, usize, *mut usize) -> i32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Bridge(i32),
    InvalidFrameLength { observed: usize, capacity: usize },
}

/// Pull one private copy of an Ethernet frame from a bounded source queue.
pub trait EthernetFrameSource {
    fn next_frame(&mut self, output: &mut [u8]) -> Result<Option<usize>, Error>;
}

pub struct SddfWlanNetSource {
    bridge_context: NonNull<c_void>,
    callback: SddfTxSourceCallback,
}

impl SddfWlanNetSource {
    /// Bind the sDDF TX queue bridge to the WLAN transmit path.
    ///
    /// # Safety
    /// `bridge_context` must point to a live, exclusively-owned
    /// `sunfish_wlan_net_bridge_t` accepted by `callback`. The context and
    /// callback must remain valid until this source is no longer used. The
    /// callback must validate the frame and output capacity before removing a
    /// queue descriptor, copy synchronously, and return its shared buffer only
    /// after the copy completes.
    pub unsafe fn new(bridge_context: NonNull<c_void>, callback: SddfTxSourceCallback) -> Self {
        Self {
            bridge_context,
            callback,
        }
    }

    /// Copy the next Ethernet frame into private staging storage.
    ///
    /// `Ok(None)` means the queue is empty. A successful frame is already
    /// retired from the shared sDDF queue, so callers must provide enough
    /// capacity for the largest configured sDDF network buffer before polling.
    pub fn next_frame(&mut self, output: &mut [u8]) -> Result<Option<usize>, Error> {
        let mut frame_bytes = 0usize;
        let status = (self.callback)(
            self.bridge_context.as_ptr(),
            output.as_mut_ptr(),
            output.len(),
            &mut frame_bytes,
        );
        match status {
            SUNFISH_WLAN_NET_IDLE => Ok(None),
            SUNFISH_WLAN_NET_OK
                if (ETHERNET_HEADER_BYTES..=output.len()).contains(&frame_bytes) =>
            {
                Ok(Some(frame_bytes))
            }
            SUNFISH_WLAN_NET_OK => Err(Error::InvalidFrameLength {
                observed: frame_bytes,
                capacity: output.len(),
            }),
            other => Err(Error::Bridge(other)),
        }
    }
}

impl EthernetFrameSource for SddfWlanNetSource {
    fn next_frame(&mut self, output: &mut [u8]) -> Result<Option<usize>, Error> {
        SddfWlanNetSource::next_frame(self, output)
    }
}
