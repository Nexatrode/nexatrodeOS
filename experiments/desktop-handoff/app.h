#pragma once
#include <stddef.h>
#include <stdint.h>

#define IMAGE_ADDR 0x30000000UL
#define IMAGE_BYTES 4096
#define RAM_ADDR 0x31000000UL
#define RAM_BYTES 16384
#define CHESS_PAGE 0x32000000UL
#define OTHER_PAGE 0x33000000UL
#define WALLET_PAGE 0x34000000UL
#define CHESS_PORT 4
#define OTHER_PORT 5
#define TEST_GAME "1. e4 e5 2. Nf3 Nc6"

/* Only the fixture bootstrap supplies these functions. Its channel arguments
 * still resolve in the running domain's own kernel capability space. */
struct app_api {
    int (*save)(unsigned port, const char *value, size_t length);
    int (*reopen)(unsigned port, const char *value, size_t length);
    int (*empty)(unsigned port);
    int (*wallet_read_denied)(unsigned port);
    int (*cap_present)(unsigned port);
    void (*log)(const char *);
};

typedef const char *(*app_entry)(const struct app_api *, size_t image_bytes);
