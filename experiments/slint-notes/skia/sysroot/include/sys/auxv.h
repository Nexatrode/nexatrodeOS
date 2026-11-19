#ifndef GUEST_SYS_AUXV_H
#define GUEST_SYS_AUXV_H
#define AT_HWCAP 16
#define AT_HWCAP2 26
unsigned long getauxval(unsigned long type);
#endif
