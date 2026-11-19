#ifndef GHC_MICROKIT_SYS_WAIT_H
#define GHC_MICROKIT_SYS_WAIT_H

#include <sys/types.h>
pid_t wait(int *status);
pid_t waitpid(pid_t pid, int *status, int options);

#define WNOHANG 1
#define WUNTRACED 2
#define WIFEXITED(status) (((status) & 0x7f) == 0)
#define WEXITSTATUS(status) (((status) >> 8) & 0xff)
#define WIFSIGNALED(status) (!WIFEXITED(status) && !WIFSTOPPED(status))
#define WTERMSIG(status) ((status) & 0x7f)
#define WIFSTOPPED(status) (((status) & 0xff) == 0x7f)
#define WSTOPSIG(status) WEXITSTATUS(status)

#endif
