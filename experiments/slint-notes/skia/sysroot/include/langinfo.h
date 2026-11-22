#ifndef GUEST_LANGINFO_H
#define GUEST_LANGINFO_H
#ifdef __cplusplus
extern "C" {
#endif
#include <locale.h>
#define CODESET 0
#define RADIXCHAR 1
#define THOUSEP 2
char *nl_langinfo(int);
char *nl_langinfo_l(int, locale_t);
#ifdef __cplusplus
}
#endif
#endif
