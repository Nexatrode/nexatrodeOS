# Pixel 4a (sunfish / SM7150), single-core EL1 bring-up.
# Installed as src/plat/sunfish/config.cmake in a clean seL4 16.0 tree.

declare_platform(sunfish KernelPlatformSunfish PLAT_SUNFISH KernelArchARM)

if(KernelPlatformSunfish)
    declare_seL4_arch(aarch64)
    # CPU0 is a Kryo 470 Silver (Cortex-A55 class); the larger cores can be
    # brought online only after single-core boot is proven.
    set(KernelArmCortexA55 ON)
    set(KernelArchArmV8a ON)
    set(KernelArmGicV3 ON)
    set(KernelArmHypervisorSupport OFF)
    # The generic ARM_SMMU option is TX2-only and its boot initializer resets
    # all stream routes/context banks. Keep it explicitly disabled on Sunfish.
    set(KernelArmSMMU OFF)
    config_set(KernelARMPlatform ARM_PLAT sunfish)
    # build_sdk.py supplies the extracted live DT and sel4-overlay.dts via
    # KernelCustomDTS / KernelCustomDTSOverlay. No invented static RAM map.
    declare_default_headers(
        TIMER_FREQUENCY 19200000
        MAX_IRQ 511
        NUM_PPI 32
        TIMER drivers/timer/arm_generic.h
        INTERRUPT_CONTROLLER arch/machine/gic_v3.h
        KERNEL_WCET 10u
    )
endif()

add_sources(
    DEP "KernelPlatformSunfish"
    CFILES src/arch/arm/machine/gic_v3.c src/arch/arm/machine/l2c_nop.c
)
