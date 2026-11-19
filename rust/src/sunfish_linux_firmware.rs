//! Feed the pinned Linux monolithic IPA/modem images to native placement code.
//! This adapter borrows checked source bytes and builds plans/CPU metadata;
//! it does not authenticate an image, invoke SCM, own target memory or mint
//! any hardware readiness. A caller must separately pin firmware identity.
#![forbid(unsafe_code)]
use crate::{
    qcom_mdt::MDT_MAX_PROGRAM_HEADERS, qcom_mdt_packed::Packed, qcom_modem_image,
    qcom_pas_metadata, sunfish_ipa_fws,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Packed(crate::qcom_mdt_packed::Error),
    Ipa(sunfish_ipa_fws::Error),
    Modem(qcom_modem_image::Error),
    MetadataTooLarge,
}
pub struct Ipa<'a> {
    packed: Packed<'a>,
    plan: sunfish_ipa_fws::LoadPlan,
}
impl<'a> Ipa<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let packed = Packed::parse(bytes).map_err(Error::Packed)?;
        let mut files =
            [sunfish_ipa_fws::SegmentFile { index: 0, bytes: 0 }; MDT_MAX_PROGRAM_HEADERS];
        let mut count = 0;
        for segment in packed.segments().filter(|s| !s.bytes.is_empty()) {
            files[count] = sunfish_ipa_fws::SegmentFile {
                index: segment.descriptor.index,
                bytes: segment.bytes.len(),
            };
            count += 1;
        }
        let plan = sunfish_ipa_fws::plan(packed.image(), &files[..count]).map_err(Error::Ipa)?;
        Ok(Self { packed, plan })
    }
    pub const fn packed(&self) -> &Packed<'a> {
        &self.packed
    }
    pub const fn plan(&self) -> &sunfish_ipa_fws::LoadPlan {
        &self.plan
    }
    /// Invoke only after source-required PAS_INIT_IMAGE and MEM_SETUP and
    /// target-memory ownership. The existing staging path checks all sources
    /// before writing, preserves gaps and zeros only BSS. This API does not
    /// establish those hardware preconditions or perform PAS_AUTH_AND_RESET.
    pub fn stage_after_pas_init(
        &self,
        memory: &mut impl sunfish_ipa_fws::StagingMemory,
    ) -> Result<sunfish_ipa_fws::StageReport, Error> {
        let mut files = core::array::from_fn::<_, MDT_MAX_PROGRAM_HEADERS, _>(|_| {
            sunfish_ipa_fws::SegmentData {
                index: 0,
                bytes: &[],
            }
        });
        let mut count = 0;
        for segment in self.packed.segments().filter(|s| !s.bytes.is_empty()) {
            files[count] = sunfish_ipa_fws::SegmentData {
                index: segment.descriptor.index,
                bytes: segment.bytes,
            };
            count += 1;
        }
        sunfish_ipa_fws::stage(&self.plan, &files[..count], memory).map_err(Error::Ipa)
    }
}

/// Reuse the existing fixed Sunfish 29-segment modem plan, including its
/// identity/geometry checks and MEM_SETUP-gated staging lifecycle. Source
/// slices borrow the monolithic file; the temporary compact metadata and
/// array of descriptors are not retained by the returned plan.
pub fn modem_plan<'a>(packed: &Packed<'a>) -> Result<qcom_modem_image::Plan<'a>, Error> {
    if packed.image().loadable_segment_count != qcom_modem_image::LOAD_COUNT {
        return Err(Error::Modem(qcom_modem_image::Error::SourcePlan));
    }
    let mut metadata = [0; qcom_pas_metadata::CAPACITY];
    let len = packed
        .copy_metadata(&mut metadata)
        .map_err(|_| Error::MetadataTooLarge)?;
    let mut sources = [qcom_modem_image::Source {
        index: 0,
        bytes: &[],
    }; qcom_modem_image::LOAD_COUNT];
    for (i, segment) in packed.segments().enumerate() {
        sources[i] = qcom_modem_image::Source {
            index: segment.descriptor.index,
            bytes: segment.bytes,
        };
    }
    qcom_modem_image::Plan::new(&metadata[..len], &sources).map_err(Error::Modem)
}
