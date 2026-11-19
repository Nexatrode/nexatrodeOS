//! Passive failure-reason and raw SMP2P status sampling, even after ReadyLost.
use crate::pixel_modem_status::Window;
use personal_os_rust_server::qcom_modem_crash::{self as core, Snapshot};
use personal_os_rust_server::qcom_modem_status::{self as status, Error, Status};
pub fn capture(w: &mut Window) -> Result<Snapshot, Error> {
    let mut r = Err(Error::Unstable);
    for _ in 0..3 {
        r = w.locked(core::snapshot);
        if r != Err(Error::Unstable) {
            break;
        }
        crate::pixel_modem_status::sync();
    }
    r
}
pub fn status(w: &mut Window) -> Result<Option<Status>, Error> {
    w.locked(|m| {
        let d = status::directory(m)?;
        let s = d.item.map(|i| status::payload(m, i)).transpose()?;
        if d != status::directory(m)? {
            return Err(Error::Unstable);
        }
        Ok(s)
    })
}

pub fn partition_stage(w: &mut Window) -> u32 {
    match w.locked(|m| Ok(core::partition_stage(m))) {
        Ok(v) => v,
        Err(e) => 0xff00 | e.code(),
    }
}
