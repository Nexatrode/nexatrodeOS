#pragma once

#include <stdint.h>

#define NXSM_PAGE_SIZE 4096u
#define NXSM_HEADER_SIZE 20u
#define NXSM_PAYLOAD_CAPACITY (NXSM_PAGE_SIZE - NXSM_HEADER_SIZE)
#define NXSM_VERSION 1u
#define NXSM_OP_LIST_CONNECTIONS 1u
#define NXSM_STATUS_REQUEST 0u
#define NXSM_STATUS_OK 1u
#define NXSM_STATUS_NO_EVENT 2u

struct nxsm_page {
    uint8_t magic[4];
    uint16_t version;
    uint16_t operation;
    uint32_t request_id;
    uint32_t status;
    uint32_t payload_len;
    uint8_t payload[NXSM_PAYLOAD_CAPACITY];
};

_Static_assert(sizeof(struct nxsm_page) == NXSM_PAGE_SIZE,
               "NXSM page must occupy exactly one page");
_Static_assert(__builtin_offsetof(struct nxsm_page, payload) == NXSM_HEADER_SIZE,
               "NXSM header layout mismatch");
