/*
 * Opt-in LD_PRELOAD observer for the temporary Linux reference tqftpserv.
 * Logs only QRTR addresses, opcode, byte count, return status and monotonic
 * time. It neither records packet bodies/paths nor changes socket operations.
 * Instrumentation and stderr output can alter timing.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <linux/qrtr.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

#ifndef TRACE_SOCKET_FAMILY
#define TRACE_SOCKET_FAMILY AF_QIPCRTR
#endif

static int (*next_connect)(int, const struct sockaddr *, socklen_t);
static ssize_t (*next_send)(int, const void *, size_t, int);
static ssize_t (*next_recvfrom)(int, void *, size_t, int, struct sockaddr *, socklen_t *);
static int trace_busy;

/* A bad caller pointer must not turn a syscall error into a tracer crash. */
static int safe_copy(void *out, const void *input, size_t length)
{
    if (!input) return 0;
    struct iovec local = { .iov_base = out, .iov_len = length };
    struct iovec remote = { .iov_base = (void *)input, .iov_len = length };
    return process_vm_readv(getpid(), &local, 1, &remote, 1, 0) == (ssize_t)length;
}

__attribute__((constructor)) static void init_trace(void)
{
    *(void **)(&next_connect) = dlsym(RTLD_NEXT, "connect");
    *(void **)(&next_send) = dlsym(RTLD_NEXT, "send");
    *(void **)(&next_recvfrom) = dlsym(RTLD_NEXT, "recvfrom");
}

static void log_socket(const char *event, int fd, ssize_t result, int error,
                       const struct sockaddr *remote, socklen_t remote_len,
                       const void *buffer, size_t available)
{
    int saved_errno = errno;
    if (__atomic_exchange_n(&trace_busy, 1, __ATOMIC_RELAXED)) return;
    struct sockaddr_storage local_storage = {0}, peer_storage = {0};
    socklen_t local_len = sizeof(local_storage), peer_len = sizeof(peer_storage);
    if (getsockname(fd, (struct sockaddr *)&local_storage, &local_len) == 0 &&
        local_storage.ss_family == TRACE_SOCKET_FAMILY && local_len >= sizeof(struct sockaddr_qrtr)) {
        const struct sockaddr_qrtr *local = (const struct sockaddr_qrtr *)&local_storage;
        const struct sockaddr_qrtr *peer = NULL;
        struct sockaddr_qrtr remote_copy = {0};
        if (remote_len >= sizeof(remote_copy) && safe_copy(&remote_copy, remote, sizeof(remote_copy)) &&
            remote_copy.sq_family == TRACE_SOCKET_FAMILY)
            peer = &remote_copy;
        else if (getpeername(fd, (struct sockaddr *)&peer_storage, &peer_len) == 0 &&
                 peer_storage.ss_family == TRACE_SOCKET_FAMILY && peer_len >= sizeof(*peer))
            peer = (const struct sockaddr_qrtr *)&peer_storage;
        unsigned int opcode = UINT32_MAX;
        unsigned char bytes[2];
        if (available >= sizeof(bytes) && safe_copy(bytes, buffer, sizeof(bytes))) {
            opcode = ((unsigned int)bytes[0] << 8) | bytes[1];
        }
        struct timespec ts = {0};
        int timed = clock_gettime(CLOCK_MONOTONIC, &ts) == 0;
        dprintf(STDERR_FILENO,
                "TFTP_META event=%s time_sec=%lld time_ns=%ld time_ok=%d local_node=%u local_port=%u peer_node=%u peer_port=%u opcode=%u result=%lld errno=%d\n",
                event, (long long)ts.tv_sec, ts.tv_nsec, timed,
                local->sq_node, local->sq_port, peer ? peer->sq_node : UINT32_MAX,
                peer ? peer->sq_port : UINT32_MAX, opcode, (long long)result, error);
    }
    __atomic_store_n(&trace_busy, 0, __ATOMIC_RELAXED);
    errno = saved_errno;
}

int connect(int fd, const struct sockaddr *address, socklen_t length)
{
    if (!next_connect) { errno = ENOSYS; return -1; }
    int result = next_connect(fd, address, length);
    int saved_errno = errno;
    log_socket("CONNECT", fd, result, result < 0 ? saved_errno : 0, address, length, NULL, 0);
    errno = saved_errno;
    return result;
}

ssize_t send(int fd, const void *buffer, size_t length, int flags)
{
    if (!next_send) { errno = ENOSYS; return -1; }
    ssize_t result = next_send(fd, buffer, length, flags);
    int saved_errno = errno;
    log_socket("SEND", fd, result, result < 0 ? saved_errno : 0, NULL, 0,
               result >= 2 ? buffer : NULL, result >= 2 ? 2 : 0);
    errno = saved_errno;
    return result;
}

ssize_t recvfrom(int fd, void *buffer, size_t length, int flags,
                 struct sockaddr *address, socklen_t *address_length)
{
    if (!next_recvfrom) { errno = ENOSYS; return -1; }
    int before = errno;
    socklen_t supplied_length = 0;
    if (address && address_length) (void)safe_copy(&supplied_length, address_length, sizeof(supplied_length));
    errno = before;
    ssize_t result = next_recvfrom(fd, buffer, length, flags, address, address_length);
    int saved_errno = errno;
    socklen_t usable_length = 0;
    if (result >= 0 && address && address_length) (void)safe_copy(&usable_length, address_length, sizeof(usable_length));
    if (usable_length > supplied_length) usable_length = supplied_length;
    log_socket("RECV", fd, result, result < 0 ? saved_errno : 0,
               result >= 0 ? address : NULL, usable_length,
               result >= 2 && length >= 2 ? buffer : NULL, result >= 2 && length >= 2 ? 2 : 0);
    errno = saved_errno;
    return result;
}
