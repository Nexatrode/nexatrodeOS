
#ifdef __cplusplus
extern "C" {
#endif

#include <sys/types.h>
#include <time.h>
struct stat { dev_t st_dev; ino_t st_ino; mode_t st_mode; nlink_t st_nlink; off_t st_size; time_t st_mtime; };
int stat(const char *, struct stat *);
int fstat(int, struct stat *);
int mkdir(const char *, mode_t);
#define S_ISREG(m) (((m) & 0170000) == 0100000)
#define S_IFREG 0100000
#define S_IFDIR 0040000

#ifdef __cplusplus
}
#endif
