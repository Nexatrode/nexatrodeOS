/* SPDX-License-Identifier: Apache-2.0
 * An indexed container: fixed directory, bounded pages and whole-region names.
 * Digests detect damage; neither the header nor directory authenticates authors.
 */
#pragma once
#include "storage_format.h"
#include <math.h>
#include <string.h>
#define MAP_DIRECTORY_BYTES 8192u
#define MAP_BUNDLE_LIMIT (32u*1024*1024)
struct map_segment {uint64_t offset,length;uint8_t digest[32];double bounds[4];};
struct map_directory {uint64_t length,version;uint8_t identity[16];double origin[2],bounds[4];struct map_segment segments[MAP_PAGE_COUNT+1];};
_Static_assert(sizeof(struct map_segment)==80,"paged segment ABI");
static inline bool map_directory_read(const uint8_t *p,struct map_directory *out){
 uint32_t version,count;memcpy(&version,p+8,4);memcpy(&count,p+12,4);
 if(memcmp(p,"NXMPGD01",8)||version!=1||count!=MAP_PAGE_COUNT)return false;
 memset(out,0,sizeof(*out));memcpy(&out->length,p+16,8);memcpy(out->origin,p+24,16);memcpy(out->bounds,p+40,32);memcpy(out->identity,p+72,16);memcpy(&out->version,p+88,8);
 if(!out->version||out->version==UINT64_MAX||out->length<MAP_DIRECTORY_BYTES||out->length>MAP_BUNDLE_LIMIT||out->length%512)return false;
 for(unsigned i=96;i<256;i++)if(p[i])return false;
 for(unsigned i=256+sizeof(out->segments);i<MAP_DIRECTORY_BYTES;i++)if(p[i])return false;
 for(unsigned i=0;i<2;i++)if(!isfinite(out->origin[i])||fabs(out->origin[i])>180)return false;
 for(unsigned i=0;i<4;i++)if(!isfinite(out->bounds[i])||fabs(out->bounds[i])>181)return false;
 if(out->bounds[0]>=out->bounds[2]||out->bounds[1]>=out->bounds[3])return false;
 memcpy(out->segments,p+256,sizeof(out->segments));uint64_t end=MAP_DIRECTORY_BYTES;
 for(unsigned i=0;i<MAP_PAGE_COUNT+1;i++){
  const struct map_segment*s=&out->segments[i];
  if(s->offset!=end||s->length<16||s->length>MAP_FILE_LIMIT||s->length>out->length-end)return false;
  uint64_t aligned=(s->length+511)&~511ULL;if(aligned>out->length-end)return false;end+=aligned;
  for(unsigned j=0;j<4;j++)if(!isfinite(s->bounds[j])||fabs(s->bounds[j])>181)return false;
  if(s->bounds[0]>s->bounds[2]||s->bounds[1]>s->bounds[3])return false;
 }
 return end==out->length;
}
