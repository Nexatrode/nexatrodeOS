#include <stddef.h>

/* Small C runtime functions used by aggregate copies in the event probe. */
void *memcpy(void *destination, const void *source, size_t count)
{
    volatile unsigned char *to = destination;
    const unsigned char *from = source;
    for (size_t i = 0; i < count; i++) to[i] = from[i];
    return destination;
}

void *memset(void *destination, int value, size_t count)
{
    volatile unsigned char *to = destination;
    for (size_t i = 0; i < count; i++) to[i] = (unsigned char)value;
    return destination;
}
