
#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned long jmp_buf[32];
int setjmp(jmp_buf);
void longjmp(jmp_buf, int) __attribute__((noreturn));

#ifdef __cplusplus
}
#endif
