#ifndef SUNFISH_NOTES_PROTOCOL_H
#define SUNFISH_NOTES_PROTOCOL_H

#include <stdint.h>

#define NOTES_OK 0x500
#define NOTES_KEY 0x501
#define NOTES_SAVE 0x502
#define VAULT_WRITE 0x302
#define VAULT_OK 0x300
#define NOTES_CAPACITY 128

struct notes_view {
    volatile uint32_t revision;
    volatile uint32_t len;
    volatile uint32_t status; /* 0 dirty, 1 accepted by volatile vault, 2 error */
    volatile uint8_t text[NOTES_CAPACITY];
};

#endif
