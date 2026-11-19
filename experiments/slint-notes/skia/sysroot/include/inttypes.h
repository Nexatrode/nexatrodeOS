
#include <stdint.h>
#define PRId64 "lld"
#define PRIu64 "llu"
#define PRIx64 "llx"
#ifdef __cplusplus
extern "C" {
#endif
long long strtoll(const char *, char **, int);
unsigned long long strtoull(const char *, char **, int);
#ifdef __cplusplus
}
#endif
