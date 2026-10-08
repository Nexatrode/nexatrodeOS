#include <stddef.h>
#include <stdint.h>

void *memcpy(void *destination, const void *source, size_t count)
{
    unsigned char *to = destination;
    const unsigned char *from = source;
    for (size_t i = 0; i < count; i++) to[i] = from[i];
    return destination;
}

void *memmove(void *destination, const void *source, size_t count)
{
    unsigned char *to = destination;
    const unsigned char *from = source;
    uintptr_t target = (uintptr_t)destination;
    uintptr_t origin = (uintptr_t)source;
    if (target <= origin || target - origin >= count) {
        for (size_t i = 0; i < count; i++) to[i] = from[i];
    } else {
        for (size_t i = count; i != 0; i--) to[i - 1] = from[i - 1];
    }
    return destination;
}

void *memset(void *destination, int value, size_t count)
{
    unsigned char *to = destination;
    for (size_t i = 0; i < count; i++) to[i] = (unsigned char)value;
    return destination;
}

int memcmp(const void *left, const void *right, size_t count)
{
    const unsigned char *a = left;
    const unsigned char *b = right;
    for (size_t i = 0; i < count; i++) {
        if (a[i] != b[i]) return (int)a[i] - (int)b[i];
    }
    return 0;
}
