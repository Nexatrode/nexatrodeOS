
#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#define PROT_READ 1
#define PROT_WRITE 2
#define MAP_PRIVATE 2
#define MAP_ANON 0x20
#define MAP_FAILED ((void *)-1)
void *mmap(void *, size_t, int, int, int, long);
int munmap(void *, size_t);
int mprotect(void *, size_t, int);

#ifdef __cplusplus
}
#endif
