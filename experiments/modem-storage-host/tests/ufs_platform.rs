use modem_storage_host::qcom_ufs_platform as platform;

struct FakeLease(u64);

type TestPlatform = platform::Platform<FakeLease>;

struct FakeIo {
    claim_failure: Option<platform::ClaimFailure>,
    uncertain_claim_retained: bool,
    live: bool,
    lease_checks: usize,
    lose_on_check: Option<usize>,
    fail_step: Option<platform::Step>,
    steps: Vec<platform::Step>,
    claims: usize,
}

impl FakeIo {
    fn new() -> Self {
        Self {
            claim_failure: None,
            uncertain_claim_retained: false,
            live: true,
            lease_checks: 0,
            lose_on_check: None,
            fail_step: None,
            steps: Vec::new(),
            claims: 0,
        }
    }
}

impl platform::PlatformIo for FakeIo {
    type Lease = FakeLease;

    fn claim_exclusive_fresh_boot(
        &mut self,
        map: &platform::SunfishMap,
    ) -> Result<Self::Lease, platform::ClaimFailure> {
        self.claims += 1;
        if map.validate_controller().is_err() {
            return Err(platform::ClaimFailure::Denied);
        }
        if let Some(failure) = self.claim_failure {
            self.uncertain_claim_retained = failure == platform::ClaimFailure::Uncertain;
            return Err(failure);
        }
        Ok(FakeLease(7))
    }

    fn lease_live(&mut self, lease: &Self::Lease) -> bool {
        self.lease_checks += 1;
        if self.lose_on_check == Some(self.lease_checks) {
            self.live = false;
        }
        lease.0 == 7 && self.live
    }

    fn perform(&mut self, lease: &Self::Lease, step: platform::Step) -> Result<(), ()> {
        if lease.0 != 7 {
            return Err(());
        }
        self.steps.push(step);
        if self.fail_step == Some(step) {
            return Err(());
        }
        Ok(())
    }
}

fn map() -> platform::SunfishMap {
    platform::SunfishMap::offline_fixture_layout()
}

fn prepare(p: &mut TestPlatform, io: &mut FakeIo) -> Result<(), platform::Error> {
    p.prepare(io, map(), (0x8000_0000, 0x1000))
}

#[test]
fn reference_layout_matches_audited_sunfish_resources() {
    let m = map();
    assert_eq!(m.host, platform::Window::new(0x01d8_4000, 0x3000));
    assert_eq!(m.phy, platform::Window::new(0x01d8_7000, 0x1000));
    assert_eq!(platform::PHY_RUNTIME_BYTES, 0x0ddc);
    assert_eq!(m.ice, platform::Window::new(0x01d9_0000, 0x8000));
    assert_eq!(m.irq.gic_spi, 265);
    assert_eq!(m.dma.stream_id, 0x300);
    assert_eq!(m.reset_controller, platform::ResetProvider::Gcc);
    assert_eq!(m.reset_id, 3);
    assert_eq!(m.device_reset_gpio, 119);
    assert_eq!(m.validate(), Ok(()));
    assert_eq!(
        platform::UFS_HOST_CLOCKS.map(|clock| {
            (
                clock.name,
                clock.provider,
                clock.id,
                clock.min_rate_hz,
                clock.max_rate_hz,
                clock.required_cx_corner,
            )
        }),
        [
            (
                "core_clk",
                platform::ClockProvider::Gcc,
                0x7e,
                50_000_000,
                200_000_000,
                platform::VddCorner::Nominal
            ),
            (
                "bus_aggr_clk",
                platform::ClockProvider::Gcc,
                0x09,
                0,
                0,
                platform::VddCorner::None
            ),
            (
                "iface_clk",
                platform::ClockProvider::Gcc,
                0x7d,
                0,
                0,
                platform::VddCorner::None
            ),
            (
                "core_clk_unipro",
                platform::ClockProvider::Gcc,
                0x89,
                37_500_000,
                150_000_000,
                platform::VddCorner::Nominal
            ),
            (
                "core_clk_ice",
                platform::ClockProvider::Gcc,
                0x81,
                75_000_000,
                300_000_000,
                platform::VddCorner::Nominal
            ),
            (
                "ref_clk",
                platform::ClockProvider::Rpmh,
                0x00,
                0,
                0,
                platform::VddCorner::None
            ),
            (
                "tx_lane0_sync_clk",
                platform::ClockProvider::Gcc,
                0x88,
                0,
                0,
                platform::VddCorner::None
            ),
            (
                "rx_lane0_sync_clk",
                platform::ClockProvider::Gcc,
                0x87,
                0,
                0,
                platform::VddCorner::None
            ),
        ]
    );
    assert_eq!(
        platform::UFS_PHY_CLOCKS.map(|clock| {
            (
                clock.name,
                clock.provider,
                clock.id,
                clock.min_rate_hz,
                clock.max_rate_hz,
                clock.required_cx_corner,
            )
        }),
        [
            (
                "ref_clk_src",
                platform::ClockProvider::Rpmh,
                0x00,
                0,
                0,
                platform::VddCorner::None
            ),
            (
                "ref_clk",
                platform::ClockProvider::Gcc,
                0x7c,
                0,
                0,
                platform::VddCorner::None
            ),
            (
                "ref_aux_clk",
                platform::ClockProvider::Gcc,
                0x84,
                0,
                0,
                platform::VddCorner::Lower
            ),
        ]
    );
    assert_eq!(platform::UFS_PRE_CAP_CX_FLOOR, platform::VddCorner::Nominal);
    assert_eq!(platform::VddCorner::None.rpmh_level(), None);
    assert_eq!(platform::VddCorner::Min.rpmh_level(), Some(49));
    assert_eq!(platform::VddCorner::Lower.rpmh_level(), Some(65));
    assert_eq!(platform::VddCorner::Low.rpmh_level(), Some(129));
    assert_eq!(platform::VddCorner::LowL1.rpmh_level(), Some(193));
    assert_eq!(platform::VddCorner::Nominal.rpmh_level(), Some(257));
    assert_eq!(platform::VddCorner::High.rpmh_level(), Some(385));
}

#[test]
fn rejects_each_wrong_mmio_range_and_overlap() {
    let mut m = map();
    m.host.base += 0x1000;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.phy.bytes = 0x2000;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.ice.base = m.phy.base;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));
}

#[test]
fn rejects_non_device_or_nonexclusive_windows() {
    let mut m = map();
    m.host.device_uncached_rw = false;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.phy.exclusive = false;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.ice.base += 1;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));
}

#[test]
fn rejects_wrong_interrupt_stream_reset_and_domain() {
    let mut m = map();
    m.irq.gic_spi = 264;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.irq.level_high = false;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.dma.stream_id = 0x301;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.dma.translated = false;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.reset_id = 0;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.device_reset_active_low = false;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.device_reset_gpio = 120;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.power_domain = 0;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));
}

#[test]
fn rejects_incomplete_regulator_clock_and_interconnect_sets() {
    let mut m = map();
    m.regulator_mask &= !platform::SUPPLY_VCC;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.regulator_mask &= !platform::SUPPLY_VDDP_REF_CLK;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.clock_mask &= !0x40;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.interconnect_mask = 1;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));
}

#[test]
fn dma_ranges_are_bounded_and_overflow_safe() {
    let m = map();
    assert_eq!(m.validate_dma_range(0x8000_0000, 4096), Ok(()));
    assert_eq!(m.validate_dma_range(0x80ff_f000, 4096), Ok(()));
    assert_eq!(
        m.validate_dma_range(0x80ff_f000, 8192),
        Err(platform::Error::InvalidDmaAperture)
    );
    assert_eq!(
        m.validate_dma_range(0x7fff_f000, 4096),
        Err(platform::Error::InvalidDmaAperture)
    );
    assert_eq!(
        m.validate_dma_range(u64::MAX - 3, 8),
        Err(platform::Error::InvalidDmaAperture)
    );
    assert_eq!(
        m.validate_dma_range(0x8000_0000, 0),
        Err(platform::Error::InvalidDmaAperture)
    );
}

#[test]
fn rejects_invalid_dma_translation_aperture() {
    let mut m = map();
    m.dma.aperture.exclusive = false;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.dma.aperture.base += 1;
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));

    let mut m = map();
    m.dma.aperture.base = u64::MAX & !(platform::DMA_PAGE_BYTES - 1);
    assert_eq!(m.validate(), Err(platform::Error::InvalidMapping));
}

#[test]
fn phy_wait_steps_match_pinned_driver_windows() {
    assert_eq!(
        platform::Step::WaitResetAssertPropagation.delay_range_us(),
        Some((1_000, 1_100))
    );
    assert_eq!(
        platform::Step::WaitRailsSettle.delay_range_us(),
        Some((1_000, 1_100))
    );
    assert_eq!(platform::Step::WaitPcsReady.delay_range_us(), None);
    assert_eq!(
        &platform::STARTUP_STEPS[..platform::PRE_CAPABILITY_STEPS.len()],
        &platform::PRE_CAPABILITY_STEPS
    );
    assert_eq!(
        &platform::STARTUP_STEPS[platform::PRE_CAPABILITY_STEPS.len()..],
        &platform::PRE_HCE_ENABLE_STEPS
    );
}

#[test]
fn invalid_evidence_is_rejected_before_resource_claim_or_action() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    let mut m = map();
    m.dma.stream_id += 1;
    assert_eq!(
        p.prepare(&mut io, m, (0x8000_0000, 4096)),
        Err(platform::Error::InvalidMapping)
    );
    assert_eq!(p.state(), platform::State::Rejected);
    assert_eq!(io.claims, 0);
    assert!(io.steps.is_empty());
}

#[test]
fn invalid_dma_is_rejected_before_resource_claim_or_action() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    assert_eq!(
        p.prepare(&mut io, map(), (0x80ff_f000, 8192)),
        Err(platform::Error::InvalidDmaAperture)
    );
    assert_eq!(p.state(), platform::State::Rejected);
    assert_eq!(io.claims, 0);
    assert!(io.steps.is_empty());
}

#[test]
fn claim_failure_never_starts_resource_sequence() {
    for (failure, expected_error, expected_state) in [
        (
            platform::ClaimFailure::Denied,
            platform::Error::ClaimFailed,
            platform::State::Rejected,
        ),
        (
            platform::ClaimFailure::Uncertain,
            platform::Error::ClaimUncertain,
            platform::State::Quarantined,
        ),
    ] {
        let mut p = TestPlatform::new();
        let mut io = FakeIo::new();
        io.claim_failure = Some(failure);
        assert_eq!(prepare(&mut p, &mut io), Err(expected_error));
        assert_eq!(p.state(), expected_state);
        assert_eq!(
            io.uncertain_claim_retained,
            failure == platform::ClaimFailure::Uncertain
        );
        assert!(io.steps.is_empty());
    }
}

#[test]
fn successful_preparation_runs_exact_order_and_opens_live_gate() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    let mut m = map();
    m.dma.translated = false;
    p.prepare(&mut io, m, (0x8000_0000, 0x1000)).unwrap();
    assert_eq!(io.steps, platform::STARTUP_STEPS);
    assert_eq!(p.completed_steps() as usize, platform::STARTUP_STEPS.len());
    assert_eq!(p.state(), platform::State::ReadyForHci);
    assert!(p.ready(&mut io));
}

#[test]
fn controller_power_stage_stops_at_capability_read_boundary() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    p.prepare_controller(&mut io, map()).unwrap();

    assert_eq!(io.steps, platform::PRE_CAPABILITY_STEPS);
    assert_eq!(
        p.completed_steps() as usize,
        platform::PRE_CAPABILITY_STEPS.len()
    );
    assert_eq!(p.state(), platform::State::ControllerPowered);
    assert!(p.controller_ready(&mut io));
    assert!(!p.ready(&mut io));
    assert!(!io.steps.contains(&platform::Step::ReserveDmaAndBindIrq));
    assert!(!io.steps.contains(&platform::Step::AssertPhySoftReset));
    assert!(!io.steps.contains(&platform::Step::StartSerdes));
}

#[test]
fn controller_power_stage_does_not_require_irq_or_dma_setup() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    let mut m = map();
    m.irq.exclusive = false;
    m.dma.translated = false;
    m.dma.aperture.base = 0;
    m.dma.aperture.bytes = 0;

    p.prepare_controller(&mut io, m).unwrap();

    assert_eq!(p.state(), platform::State::ControllerPowered);
    assert!(p.controller_ready(&mut io));
    assert_eq!(io.steps, platform::PRE_CAPABILITY_STEPS);
    assert!(!io.steps.contains(&platform::Step::ReserveDmaAndBindIrq));
    assert_eq!(
        p.prepare_hce_enable(&mut io, (0x8000_0000, 0x1000)),
        Err(platform::Error::InvalidMapping)
    );
    assert_eq!(p.state(), platform::State::Quarantined);
}

#[test]
fn hce_preparation_runs_only_after_the_capability_read_phase() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    let mut m = map();
    m.dma.translated = false;
    p.prepare_controller(&mut io, m).unwrap();
    assert!(p.controller_ready(&mut io));
    assert_eq!(p.state(), platform::State::ControllerPowered);

    p.prepare_hce_enable(&mut io, (0x8000_0000, 0x1000))
        .unwrap();
    assert_eq!(io.steps, platform::STARTUP_STEPS);
    assert_eq!(p.state(), platform::State::ReadyForHci);
    assert!(p.ready(&mut io));
}

#[test]
fn invalid_dma_after_capability_read_quarantines_powered_controller() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    p.prepare_controller(&mut io, map()).unwrap();
    let pre_cap_count = io.steps.len();

    assert_eq!(
        p.prepare_hce_enable(&mut io, (0x80ff_f000, 8192)),
        Err(platform::Error::InvalidDmaAperture)
    );
    assert_eq!(p.state(), platform::State::Quarantined);
    assert_eq!(io.steps.len(), pre_cap_count);
    assert!(!p.controller_ready(&mut io));
}

#[test]
fn every_failed_stage_quarantines_and_never_claims_readiness() {
    for step in platform::STARTUP_STEPS {
        let mut p = TestPlatform::new();
        let mut io = FakeIo::new();
        io.fail_step = Some(step);
        assert_eq!(
            prepare(&mut p, &mut io),
            Err(platform::Error::StepFailed(step))
        );
        assert_eq!(
            p.state(),
            platform::State::Quarantined,
            "failed at {step:?}"
        );
        assert!(!p.ready(&mut io));
        assert_eq!(io.claims, 1);
    }
}

#[test]
fn every_lease_loss_point_stops_before_following_action() {
    for check in 1..=(platform::STARTUP_STEPS.len() * 2) {
        let mut p = TestPlatform::new();
        let mut io = FakeIo::new();
        io.lose_on_check = Some(check);
        assert_eq!(prepare(&mut p, &mut io), Err(platform::Error::LeaseLost));
        assert_eq!(
            p.state(),
            platform::State::Quarantined,
            "lease check {check}"
        );
        assert!(io.steps.len() <= check / 2);
        assert!(!p.ready(&mut io));
    }
}

#[test]
fn live_lease_loss_after_setup_revokes_hci_readiness() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    prepare(&mut p, &mut io).unwrap();
    io.live = false;
    assert!(!p.ready(&mut io));
    assert_eq!(p.state(), platform::State::Quarantined);
}

#[test]
fn preparation_is_one_shot_even_after_success() {
    let mut p = TestPlatform::new();
    let mut io = FakeIo::new();
    prepare(&mut p, &mut io).unwrap();
    assert_eq!(prepare(&mut p, &mut io), Err(platform::Error::InvalidState));
    assert_eq!(io.claims, 1);
    assert_eq!(io.steps, platform::STARTUP_STEPS);
}
