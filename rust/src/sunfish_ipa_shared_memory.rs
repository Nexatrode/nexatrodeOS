//! Retained real IMEM/SMEM backing for the opt-in IPA installation stage.
//! No payload access, DMA command or production-ready capability is provided.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::{
    sunfish_ipa_smem::{self as smem, Owner, Report},
    sunfish_ipa_smem_native::Window,
    sunfish_smmu_lpae::LpaePageMapping,
};
use core::marker::PhantomData;
#[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
use core::mem::ManuallyDrop;
#[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
type Modem<'a> = crate::sunfish_mpss_linux_smem::Extension<
    Window<'a>,
    crate::sunfish_mpss_linux_smem_native::Native<
        'a,
        fn(u64) -> Result<(), crate::sunfish_mpss_linux_smem::Error>,
    >,
>;
#[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
pub(crate) type Smp<'a> = crate::sunfish_mpss_linux_smp2p::Controller<
    Window<'a>,
    crate::sunfish_mpss_linux_smem_native::Native<
        'a,
        fn(u64) -> Result<(), crate::sunfish_mpss_linux_smem::Error>,
    >,
    crate::sunfish_mpss_linux_smp2p_native::Platform<'a>,
>;

pub const IMEM_PHYSICAL: u64 = 0x146a8000;
pub const IMEM_BYTES: usize = 8192;
pub const PRIVATE_PAGES: usize = 32;
pub const MAPPING_PAGES: usize = 37;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Mapping,
    Allocation(smem::Error),
    NotAllocated,
    Geometry,
    #[cfg(feature = "sunfish-native-startup-mpss-glink")]
    Glink(crate::sunfish_mpss_linux_smp2p::GlinkError),
}

/// Owns the real reservation view and live allocation, rather than accepting
/// copied addresses as a grant. Its CPU views and SMEM owner stay retained
/// inside the SMMU adapter even if allocation or table installation fails.
pub struct SharedMemory<'a> {
    owner: Option<Owner<Window<'a>>>,
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    modem: Option<
        crate::sunfish_mpss_linux_smem::Extension<
            Window<'a>,
            crate::sunfish_mpss_linux_smem_native::Native<
                'a,
                fn(u64) -> Result<(), crate::sunfish_mpss_linux_smem::Error>,
            >,
        >,
    >,
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    modem_attempted: bool,
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    smp: Option<Smp<'a>>,
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    smp_signal: Option<ManuallyDrop<crate::sunfish_mpss_linux_smp2p_native::Platform<'a>>>,
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    smp_attempted: bool,
    imem: *const u8,
    views: [(usize, usize); 3],
    _reservation: PhantomData<&'a [u8]>,
}
impl<'a> SharedMemory<'a> {
    /// # Safety
    /// Retain the audited reserved IPA IMEM frames at 0x146a8000/8 KiB and
    /// the exact SMEM/lock views supplied to Window. The IMEM CPU view is
    /// uncached RO/NX, used to retain physical frame authority, never read.
    /// The caller must establish IPA read/write mapping authority for these
    /// shared reserved frames; they are not exclusive allocator RAM. There
    /// must be no competing AP allocator/SMMU writer, reset, remap or cached
    /// alias. The native manifest/actual CapDL and pinned Linux profile must
    /// establish these facts. No integer or ItemInfo alone is sufficient.
    pub unsafe fn new(
        window: Window<'a>,
        imem: *const u8,
        physical: u64,
        bytes: usize,
    ) -> Result<Self, Error> {
        if imem.is_null()
            || imem as usize % 4096 != 0
            || physical != IMEM_PHYSICAL
            || bytes != IMEM_BYTES
        {
            return Err(Error::Mapping);
        }
        let [smem, lock] = window.virtual_spans();
        let views = [(imem as usize, IMEM_BYTES), smem, lock];
        for (i, (start, bytes)) in views.iter().copied().enumerate() {
            let end = start.checked_add(bytes).ok_or(Error::Mapping)?;
            for (other, length) in views[..i].iter().copied() {
                let other_end = other.checked_add(length).ok_or(Error::Mapping)?;
                if start < other_end && other < end {
                    return Err(Error::Mapping);
                }
            }
        }
        Ok(Self {
            owner: Some(Owner::new(window)),
            #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
            modem: None,
            #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
            modem_attempted: false,
            #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
            smp: None,
            #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
            smp_signal: None,
            #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
            smp_attempted: false,
            imem,
            views,
            _reservation: PhantomData,
        })
    }
    pub(crate) fn prepare(&mut self) -> Result<(), Error> {
        self.owner
            .as_mut()
            .ok_or(Error::NotAllocated)?
            .allocate_once()
            .map_err(Error::Allocation)?;
        self.shared_pages()?;
        Ok(())
    }
    pub fn report(&self) -> Report {
        self.allocation_owner().report()
    }
    pub fn item_physical(&self) -> Option<u64> {
        self.allocation_owner()
            .retained_item()
            .map(|item| item.physical())
    }
    fn allocation_owner(&self) -> &Owner<Window<'a>> {
        #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
        if let Some(s) = self.smp.as_ref() {
            return s.memory().parent();
        }
        #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
        if let Some(m) = self.modem.as_ref() {
            return m.parent();
        }
        // The actual owner is retained in exactly one of these two fields.
        self.owner.as_ref().expect("retained SMEM owner")
    }
    /// Transfer the actual allocation owner and its delegated writable pages
    /// into the modem extension. No pointer/physical-address substitute is
    /// accepted here. gate is the trusted platform lifecycle/health provider.
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn attach_modem(
        &mut self,
        generation: u64,
        gate: fn(u64) -> Result<(), crate::sunfish_mpss_linux_smem::Error>,
    ) -> Result<(), Error> {
        use crate::{sunfish_mpss_linux_smem as modem, sunfish_mpss_linux_smem_native::Native};
        if self.modem_attempted {
            return Err(Error::Geometry);
        }
        self.modem_attempted = true;
        let view = self
            .owner
            .as_ref()
            .ok_or(Error::NotAllocated)?
            .retained_io()
            .map_err(Error::Allocation)?;
        let pages = view.modem_pages().ok_or(Error::Mapping)?;
        let base = self.views[1].0 as *const u8;
        // SAFETY: the actual parent Window retained these exact delegated
        // pages at construction. They stay under this serialized owner; only
        // the child extension can write through this adapter.
        let payload = unsafe {
            Native::new(
                base,
                smem::SMEM_PHYSICAL,
                (self.views[1].0 + smem::PARTITION_OFFSET) as *mut u32,
                smem::METADATA_PHYSICAL,
                pages,
                0x8611f000,
                gate,
            )
        }
        .map_err(|_| Error::Mapping)?;
        let parent = self.owner.take().ok_or(Error::NotAllocated)?;
        self.modem = Some(modem::Extension::new(parent, payload, generation));
        if self.modem.as_ref().unwrap().report().error.is_some() {
            return Err(Error::Geometry);
        }
        Ok(())
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn modem_report(&self) -> Option<crate::sunfish_mpss_linux_smem::Report> {
        #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
        if let Some(s) = self.smp.as_ref() {
            return Some(s.memory().report());
        }
        self.modem.as_ref().map(|m| m.report())
    }
    /// Move the real prepared allocation owner into the live protocol owner.
    /// The sealed receipt alone cannot replace that owner. Both providers are
    /// retained on every outcome, including a rejected predecessor or mapping.
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn activate_smp2p(
        &mut self,
        prepared: crate::sunfish_mpss_linux_smem::Prepared,
        signal: crate::sunfish_mpss_linux_smp2p_native::Platform<'a>,
    ) -> Result<(), Error> {
        let retained = ManuallyDrop::new(signal);
        if self.smp_attempted {
            core::mem::forget(retained);
            return Err(Error::Geometry);
        }
        self.smp_attempted = true;
        self.smp_signal = Some(retained);
        let (at, n) = self.smp_signal.as_ref().unwrap().span();
        let end = at.checked_add(n).ok_or(Error::Mapping)?;
        if self.views.iter().any(|(b, n)| at < b + n && *b < end) {
            return Err(Error::Mapping);
        }
        let m: Modem<'a> = self.modem.take().ok_or(Error::NotAllocated)?;
        let signal = ManuallyDrop::into_inner(self.smp_signal.take().unwrap());
        self.smp = Some(crate::sunfish_mpss_linux_smp2p::Controller::new(
            m, prepared, signal,
        ));
        let result = self.smp.as_mut().unwrap().initialize();
        result.map(|_| ()).map_err(|_| Error::Geometry)
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn smp2p_report(&self) -> Option<crate::sunfish_mpss_linux_smp2p::Report> {
        self.smp.as_ref().map(|s| s.report())
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn smp2p_poll(&mut self) -> Result<crate::sunfish_mpss_linux_smp2p::Poll, Error> {
        self.smp
            .as_mut()
            .ok_or(Error::NotAllocated)?
            .poll()
            .map_err(|_| Error::Geometry)
    }
    #[cfg(feature = "sunfish-native-startup-mpss-glink")]
    pub(crate) fn glink<T>(
        &mut self,
        action: impl FnOnce(&mut Smp<'a>) -> Result<T, crate::sunfish_mpss_linux_smp2p::GlinkError>,
    ) -> Result<T, Error> {
        action(self.smp.as_mut().ok_or(Error::NotAllocated)?).map_err(Error::Glink)
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn take_mpss_ready_observation(
        &mut self,
    ) -> Result<crate::sunfish_mpss_linux_smp2p::ReadyObservation, Error> {
        self.smp
            .as_mut()
            .ok_or(Error::NotAllocated)?
            .take_ready_observation()
            .map_err(|_| Error::Geometry)
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn modem_step(&mut self) -> Result<crate::sunfish_mpss_linux_smem::Phase, Error> {
        self.modem
            .as_mut()
            .ok_or(Error::NotAllocated)?
            .step()
            .map_err(|_| Error::Geometry)
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn take_modem_prepared(
        &mut self,
    ) -> Result<crate::sunfish_mpss_linux_smem::Prepared, Error> {
        self.modem
            .as_mut()
            .ok_or(Error::NotAllocated)?
            .take_prepared()
            .map_err(|_| Error::Geometry)
    }
    pub(crate) fn virtual_spans(&self) -> [(usize, usize); 3] {
        self.views
    }
    pub(crate) fn shared_pages(&self) -> Result<[u64; 5], Error> {
        let item = self
            .allocation_owner()
            .retained_item()
            .ok_or(Error::NotAllocated)?;
        let (first, bytes) = item.page_span();
        if item.bytes() != smem::ITEM_BYTES
            || bytes != 3 * 4096
            || first < smem::SMEM_PHYSICAL
            || first + bytes as u64 > smem::SMEM_PHYSICAL + smem::SMEM_BYTES as u64
        {
            return Err(Error::Geometry);
        }
        Ok([
            IMEM_PHYSICAL,
            IMEM_PHYSICAL + 4096,
            first,
            first + 4096,
            first + 8192,
        ])
    }
    pub(crate) fn excludes_private_page(page: u64) -> bool {
        (IMEM_PHYSICAL..IMEM_PHYSICAL + IMEM_BYTES as u64).contains(&page)
            || (smem::SMEM_PHYSICAL..smem::SMEM_PHYSICAL + smem::SMEM_BYTES as u64).contains(&page)
    }
    pub(crate) fn mappings(
        &self,
        private: &[u64],
    ) -> Result<[LpaePageMapping; MAPPING_PAGES], Error> {
        if private.len() != PRIVATE_PAGES
            || private.iter().enumerate().any(|(i, page)| {
                *page < 0x80000000
                    || *page & 4095 != 0
                    || *page >= 1u64 << 40
                    || Self::excludes_private_page(*page)
                    || private[..i].contains(page)
            })
        {
            return Err(Error::Geometry);
        }
        let shared = self.shared_pages()?;
        Ok(core::array::from_fn(|i| {
            let (iova_page, physical_page) = if i < 2 {
                (shared[i], shared[i])
            } else if i < 34 {
                (0x80000000 + (i - 2) as u64 * 4096, private[i - 2])
            } else {
                (shared[i - 32], shared[i - 32])
            };
            LpaePageMapping {
                iova_page,
                physical_page,
            }
        }))
    }
    pub(crate) fn validate_mappings(
        &self,
        mappings: &[LpaePageMapping],
        private: &[u64],
    ) -> Result<(), Error> {
        // Keep the retained IMEM view part of this validation, without reading
        // its contents. Only the exact live shared pages can extend the pool.
        if self.imem as usize != self.views[0].0 || mappings != self.mappings(private)? {
            return Err(Error::Geometry);
        }
        Ok(())
    }
}
