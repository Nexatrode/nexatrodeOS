//! SCM envelope for the pinned working mainline Sunfish IPA loader.
//!
//! Unlike the older downstream adapter, *all three* mutating PAS operations
//! acquire the SCM resources, including MEM_SETUP. The audited Sunfish SCM
//! node has no optional clocks and has a crypto-to-EBI ICC path. Its provider
//! must implement that real path; IPA power-floor diagnostics cannot replace it.
#![forbid(unsafe_code)]

use crate::qcom_pas_init::{Request, Transport};
use crate::sunfish_ipa_pas::{CallKind, IpaPasTransport, TransportError, request_matches};

/// Trusted SCM resource provider. A lease must retain the real source-derived
/// crypto-to-EBI vote and the Sunfish no-optional-clocks configuration for this
/// call. Shared-client providers must preserve all other owners. A cold-test
/// provider may keep nondecreasing floors until reboot, but must declare that
/// difference and must not pretend to restore unknown prior AP contributions.
pub trait Resources {
    type Lease;
    fn acquire(&mut self) -> Result<Self::Lease, u64>;
    fn release(&mut self, lease: Self::Lease) -> Result<(), u64>;
}

pub struct LinuxTransport<'a, S, R> {
    secure: &'a mut S,
    resources: &'a mut R,
    failed: bool,
}
impl<'a, S, R> LinuxTransport<'a, S, R> {
    pub const fn new(secure: &'a mut S, resources: &'a mut R) -> Self {
        Self {
            secure,
            resources,
            failed: false,
        }
    }
}
impl<S: Transport, R: Resources> IpaPasTransport for LinuxTransport<'_, S, R> {
    fn invoke(&mut self, kind: CallKind, request: &Request) -> Result<[u64; 8], TransportError> {
        if self.failed || !request_matches(kind, request) {
            self.failed = true;
            return Err(TransportError::RequestKindMismatch);
        }
        let result = if kind == CallKind::Availability {
            self.secure
                .invoke(request)
                .map_err(TransportError::SecureCall)
        } else {
            match self.resources.acquire() {
                Err(error) => Err(TransportError::BandwidthAcquire(error)),
                Ok(lease) => {
                    let secure = self.secure.invoke(request);
                    let release = self.resources.release(lease);
                    match (secure, release) {
                        (Ok(reply), Ok(())) => Ok(reply),
                        (Err(code), Ok(())) => Err(TransportError::SecureCall(code)),
                        (Ok(_), Err(code)) => Err(TransportError::BandwidthRelease(code)),
                        (Err(secure), Err(bandwidth)) => {
                            Err(TransportError::SecureAndBandwidthRelease { secure, bandwidth })
                        }
                    }
                }
            }
        };
        self.failed = match &result {
            Err(_) => true,
            Ok(reply) => reply[0] != 0 || reply[1] != u64::from(kind == CallKind::Availability),
        };
        result
    }
}
