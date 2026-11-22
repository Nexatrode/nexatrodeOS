#ifndef GUEST_ASM_HWCAP_H
#define GUEST_ASM_HWCAP_H
#define HWCAP_FP (1UL << 0)
#define HWCAP_ASIMD (1UL << 1)
#define HWCAP_AES (1UL << 3)
#define HWCAP_PMULL (1UL << 4)
#define HWCAP_CRC32 (1UL << 7)
#define HWCAP2_CRC32 (1UL << 4)
#define HWCAP2_PMULL (1UL << 1)
#endif
