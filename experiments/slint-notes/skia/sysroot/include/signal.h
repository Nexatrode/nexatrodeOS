
#ifdef __cplusplus
extern "C" {
#endif

typedef void (*sighandler_t)(int);
sighandler_t signal(int, sighandler_t);
int raise(int);
#define SIGABRT 6
#define SIGSEGV 11

#ifdef __cplusplus
}
#endif
