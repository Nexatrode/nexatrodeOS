//! Live native owner gate; one lock observer and a consumed ready receipt.
use crate::{
    pixel_glink_inventory as inventory, pixel_modem_status::Window,
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::{
    qcom_glink_fifo::{Fault, Observation},
    qcom_glink_native_io::Gate,
};

pub struct LiveGate {
    window: Window,
    ready: ReadyObservation,
    baseline: inventory::Report,
    failed: bool,
    failure_code: u32,
}
impl LiveGate {
    pub fn new(window: Window, ready: ReadyObservation, baseline: inventory::Report) -> Self {
        Self {
            window,
            ready,
            baseline,
            failed: false,
            failure_code: 0,
        }
    }
}
impl Gate for LiveGate {
    fn observe(&mut self) -> Result<Observation, Fault> {
        if self.failed {
            return Err(Fault);
        }
        self.failed = true;
        let current = match self
            .window
            .locked(|memory| inventory::inspect_ref(memory, &self.ready))
        {
            Ok(current) => current,
            Err(error) => {
                self.failure_code = error.code();
                return Err(Fault);
            }
        };
        if current.items != self.baseline.items {
            self.failure_code = 0x1001;
            return Err(Fault);
        }
        if current.directory.route != self.baseline.directory.route {
            self.failure_code = 0x1002;
            return Err(Fault);
        }
        // The sealed SMP2P receipt carries the boot generation. Success still
        // requires fresh lock-protected ready/fatal/AP/peer/route/item reads;
        // a failed gate is never revived by a later ready bit or a new IRQ.
        self.failed = false;
        self.failure_code = 0;
        Ok(Observation {
            generation: self.ready.generation(),
            identity: 1,
            ready: true,
            fatal: false,
        })
    }

    fn diagnostic(&self) -> u32 {
        self.failure_code
    }
}
