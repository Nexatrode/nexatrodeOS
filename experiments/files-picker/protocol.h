/* SPDX-License-Identifier: Apache-2.0
 * Trusted Files UI has a parent-only input queue and a private Vault endpoint.
 * Maps sees selected metadata, never the directory or original file identity.
 */
#pragma once
#include <stdint.h>
#include <stdbool.h>
#define FILES_READ 0x470
#define FILES_RELEASE 0x471
#define FILES_CHUNK 3072u
#ifdef MAP_LARGE_IMPORT
#define FILES_LIMIT (64u*1024*1024)
#else
#define FILES_LIMIT (8u*1024*1024+4096)
#endif
struct files_state {uint32_t revision,open;uint64_t serial,token[2],length,status;char name[128];uint64_t reads,denied;};
struct files_request {uint64_t open;};
struct files_chunk {uint32_t length;uint8_t bytes[FILES_CHUNK];};
struct files_grant {uint64_t token[2],id,length;};
static inline bool files_permits(const struct files_grant *g,uint64_t a,uint64_t b,uint64_t offset,uint64_t n){
    return g->id&&(g->token[0]||g->token[1])&&a==g->token[0]&&b==g->token[1]&&n&&n<=FILES_CHUNK&&offset<=g->length&&n<=g->length-offset;
}
static inline void files_revoke(struct files_grant *g){volatile uint8_t *p=(void*)g;for(unsigned i=0;i<sizeof(*g);i++)p[i]=0;}
