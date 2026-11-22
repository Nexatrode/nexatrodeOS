
#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>
#include <time.h>
typedef unsigned long pthread_t;
typedef int pthread_key_t;
typedef int pthread_once_t;
typedef unsigned long pthread_mutex_t;
typedef unsigned long pthread_mutexattr_t;
typedef unsigned long pthread_cond_t;
typedef unsigned long pthread_condattr_t;
typedef unsigned long pthread_attr_t;
typedef unsigned long pthread_rwlock_t;
#define PTHREAD_ONCE_INIT 0
#define PTHREAD_MUTEX_INITIALIZER 0
#define PTHREAD_COND_INITIALIZER 0
#define PTHREAD_MUTEX_NORMAL 0
#define PTHREAD_MUTEX_RECURSIVE 1
#define PTHREAD_CREATE_JOINABLE 0
int pthread_mutex_init(pthread_mutex_t *, const pthread_mutexattr_t *);
int pthread_mutex_destroy(pthread_mutex_t *);
int pthread_mutex_lock(pthread_mutex_t *);
int pthread_mutex_trylock(pthread_mutex_t *);
int pthread_mutex_unlock(pthread_mutex_t *);
int pthread_mutexattr_init(pthread_mutexattr_t *);
int pthread_mutexattr_destroy(pthread_mutexattr_t *);
int pthread_mutexattr_settype(pthread_mutexattr_t *, int);
int pthread_cond_init(pthread_cond_t *, const pthread_condattr_t *);
int pthread_cond_destroy(pthread_cond_t *);
int pthread_cond_signal(pthread_cond_t *);
int pthread_cond_broadcast(pthread_cond_t *);
int pthread_cond_wait(pthread_cond_t *, pthread_mutex_t *);
int pthread_cond_timedwait(pthread_cond_t *, pthread_mutex_t *, const struct timespec *);
int pthread_once(pthread_once_t *, void (*)(void));
int pthread_key_create(pthread_key_t *, void (*)(void *));
int pthread_key_delete(pthread_key_t);
void *pthread_getspecific(pthread_key_t);
int pthread_setspecific(pthread_key_t, const void *);
pthread_t pthread_self(void);
int pthread_equal(pthread_t, pthread_t);
int pthread_create(pthread_t *, const pthread_attr_t *, void *(*)(void *), void *);
int pthread_join(pthread_t, void **);
int pthread_detach(pthread_t);

#ifdef __cplusplus
}
#endif
