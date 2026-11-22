
#ifdef __cplusplus
extern "C" {
#endif

#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 64
#define O_TRUNC 512
#define O_APPEND 1024
int open(const char *, int, ...);

#ifdef __cplusplus
}
#endif
