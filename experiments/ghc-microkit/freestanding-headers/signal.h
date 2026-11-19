#ifndef GHC_MICROKIT_SIGNAL_H
#define GHC_MICROKIT_SIGNAL_H

/* Compile-only type; Microkit does not provide POSIX signals. */
typedef unsigned long sigset_t;
typedef int sig_atomic_t;
typedef struct { int signo; int code; void *address; } siginfo_t;

#define SIGINT 2
#define SIGTTOU 22
#define SIG_BLOCK 0
#define SIG_SETMASK 2

int sigemptyset(sigset_t *set);
int sigaddset(sigset_t *set, int signal_number);
int sigprocmask(int how, const sigset_t *set, sigset_t *old_set);

#endif
