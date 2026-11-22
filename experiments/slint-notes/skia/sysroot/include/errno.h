
#ifdef __cplusplus
extern "C" {
#endif

int *__errno_location(void);
#define errno (*__errno_location())
#define EPERM 1
#define ENOENT 2
#define EINTR 4
#define EIO 5
#define ENOMEM 12
#define EACCES 13
#define EFAULT 14
#define EBUSY 16
#define EEXIST 17
#define EINVAL 22
#define ERANGE 34
#define EAGAIN 11
#define ETIMEDOUT 110

#ifdef __cplusplus
}
#endif
