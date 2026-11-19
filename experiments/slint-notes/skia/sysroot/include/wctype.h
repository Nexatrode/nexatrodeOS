#ifndef GUEST_WCTYPE_H
#define GUEST_WCTYPE_H
#ifdef __cplusplus
extern "C" {
#endif
#include <wchar.h>
typedef unsigned long wctype_t;
typedef unsigned long wctrans_t;
int iswspace(wint_t);
int iswalpha(wint_t);
int iswdigit(wint_t);
wint_t towlower(wint_t);
wint_t towupper(wint_t);
#ifdef __cplusplus
}
#endif
#endif
