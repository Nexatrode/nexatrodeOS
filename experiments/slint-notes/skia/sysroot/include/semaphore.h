#ifndef GUEST_SEMAPHORE_H
#define GUEST_SEMAPHORE_H
#ifdef __cplusplus
extern "C" {
#endif
typedef struct { unsigned long value; } sem_t;
int sem_init(sem_t *, int, unsigned);
int sem_destroy(sem_t *);
int sem_post(sem_t *);
int sem_wait(sem_t *);
int sem_trywait(sem_t *);
#ifdef __cplusplus
}
#endif
#endif
