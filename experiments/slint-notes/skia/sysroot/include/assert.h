
#ifdef NDEBUG
#define assert(x) ((void)0)
#else
#define assert(x) ((x) ? (void)0 : __assert_fail(#x, __FILE__, __LINE__, __func__))
#endif
#ifdef __cplusplus
extern "C" {
#endif
void __assert_fail(const char *, const char *, int, const char *) __attribute__((noreturn));
#ifdef __cplusplus
}
#endif
