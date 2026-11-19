//! One experimental boot with the freshly verified GLINK allocations. No
//! post-AUTH writes exist here. The resulting identity permits only a bounded
//! readout, never production lifecycle, packet/index or recovery operations.
use crate::{
    pixel_glink_prepare as memory, pixel_glink_receive as receive,
    pixel_modem_status::{Window, sync},
    pixel_smp2p_handshake::{Handshake, ReadyObservation},
};
use personal_os_rust_server::{
    qcom_modem_status::{Error, Item, Read},
    qcom_pas_auth::{AuthOnce, Report as AuthReport},
    qcom_pas_init::Transport,
    qcom_pas_metadata::Inspection,
};
pub struct Boot {
    auth: AuthOnce,
    prepared: memory::Report,
}
pub struct Started {
    auth: AuthOnce,
    identity: BootIdentity,
}
pub struct BootIdentity {
    descriptor: Item,
    tx: Item,
}
/// Distinct evidence for the separately built, fixed version experiment.
/// It can only consume an actual Started session, never a readout identity.
pub struct FreshBoot {
    identity: BootIdentity,
}
pub struct Failure {
    pub error: Error,
    pub auth: AuthReport,
}
pub struct Report {
    pub receive: receive::Report,
    pub tx_zero_passes: u32,
}
impl Boot {
    /// # Safety
    /// Same exclusive five-page uncached mapping contract as Preparation::new.
    /// Caller transfers the actual staged/INFO-complete AuthOnce. On any
    /// preparation failure it is dropped, without authentication or rollback.
    pub unsafe fn prepare(
        w: &mut Window,
        h: &Handshake,
        mut auth: AuthOnce,
        fresh: &Inspection,
        ap: *mut u8,
        tail: *mut u8,
    ) -> Result<Self, Error> {
        let permit = auth
            .glink_boot_memory_prepare(fresh)
            .map_err(|_| Error::Auth)?;
        let mut preparation = unsafe { memory::Preparation::new(ap, tail) };
        let prepared = preparation.run(w, h, permit)?;
        Ok(Self { auth, prepared })
    }
    pub fn prepared(&self) -> &memory::Report {
        &self.prepared
    }
    pub fn authenticate(
        mut self,
        w: &mut Window,
        h: &Handshake,
        io: &mut impl Transport,
        fresh: &Inspection,
        before: impl FnMut(&AuthReport),
    ) -> Result<Started, Failure> {
        if let Err(error) = w.locked(|m| memory::revalidate(m, h)) {
            return Err(Failure {
                error,
                auth: *self.auth.report(),
            });
        }
        if self.auth.authenticate(io, fresh, before).is_err() {
            return Err(Failure {
                error: Error::Auth,
                auth: *self.auth.report(),
            });
        }
        Ok(Started {
            auth: self.auth,
            identity: BootIdentity {
                descriptor: self.prepared.descriptor,
                tx: self.prepared.tx,
            },
        })
    }
}
impl Started {
    pub fn into_parts(self) -> (AuthOnce, BootIdentity) {
        (self.auth, self.identity)
    }
    pub fn into_fresh_parts(self) -> (AuthOnce, FreshBoot) {
        (
            self.auth,
            FreshBoot {
                identity: self.identity,
            },
        )
    }
}
impl FreshBoot {
    pub(crate) fn items(&self) -> (Item, Item) {
        (self.identity.descriptor, self.identity.tx)
    }
    pub(crate) fn inspect(&self, m: &impl Read, ready: &ReadyObservation) -> Result<Report, Error> {
        inspect_ref(m, &self.identity, ready)
    }
}
impl BootIdentity {
    fn validate(&self, r: &receive::Report) -> Result<(), Error> {
        if r.inventory.items[0] != Some(self.descriptor) || r.inventory.items[1] != Some(self.tx) {
            return Err(Error::RouteChanged);
        }
        // No AP packet has been published or RX packet consumed. Remote TX
        // tail cannot validly advance without an AP head publication either.
        if r.words[..3] != [0; 3] || r.words[4..] != [0; 4] {
            return Err(Error::Outbound);
        }
        Ok(())
    }
}
pub(crate) fn inspect(
    m: &impl Read,
    identity: BootIdentity,
    ready: ReadyObservation,
) -> Result<Report, Error> {
    inspect_ref(m, &identity, &ready)
}
fn inspect_ref(
    m: &impl Read,
    identity: &BootIdentity,
    ready: &ReadyObservation,
) -> Result<Report, Error> {
    let one = receive::inspect_ref(m, ready)?;
    identity.validate(&one)?;
    for _ in 0..2 {
        for i in 0..identity.tx.size {
            if m.read8(identity.tx.offset + i) != 0 {
                return Err(Error::Outbound);
            }
        }
        sync();
    }
    let two = receive::inspect_ref(m, ready)?;
    identity.validate(&two)?;
    if one.inventory != two.inventory
        || one.words != two.words
        || one.available != two.available
        || one.header != two.header
    {
        return Err(Error::Unstable);
    }
    Ok(Report {
        receive: two,
        tx_zero_passes: 2,
    })
}
pub fn read(
    w: &mut Window,
    identity: BootIdentity,
    ready: ReadyObservation,
) -> Result<Report, Error> {
    w.locked(|m| inspect(m, identity, ready))
}
