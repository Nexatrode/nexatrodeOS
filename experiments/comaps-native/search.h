/* SPDX-License-Identifier: Apache-2.0
 * Fixed-capacity name search over the current decoded region. No filesystem,
 * upstream search index, networking, dynamic allocation or saved query log.
 */
#pragma once
#include "mwm.h"
#include <string.h>
#include <math.h>
#define MAP_SEARCH_QUERY 48
#define MAP_SEARCH_RESULTS 8
#define MAP_SEARCH_CATALOGUE_MAX 8192
struct map_search_entry {char name[96];float x,y;uint32_t geometry;};
_Static_assert(sizeof(struct map_search_entry)==108,"search-v1 little-endian ABI");
/* Validate the complete candidate before replacing a previously usable index.
 * Bounds are local map coordinates, as in native rendering pages. */
static inline bool map_search_catalogue_read(struct map_search_entry *out,unsigned *count,const void *bytes,size_t length,const double bounds[4]){
    uint32_t h[4];if(length<16)return false;memcpy(h,bytes,16);
    if(h[0]!=0x3153584e||h[1]>MAP_SEARCH_CATALOGUE_MAX||h[2]!=sizeof(*out)||h[3]||length!=16+(size_t)h[1]*sizeof(*out))return false;
    for(unsigned i=0;i<h[1];i++){
        struct map_search_entry e;memcpy(&e,(const uint8_t*)bytes+16+i*sizeof(e),sizeof(e));
        if(!e.name[0]||!memchr(e.name,0,sizeof(e.name))||e.geometry>1||!isfinite(e.x)||!isfinite(e.y)||e.x<bounds[0]||e.y<bounds[1]||e.x>bounds[2]||e.y>bounds[3])return false;
    }
    memcpy(out,(const uint8_t*)bytes+16,h[1]*sizeof(*out));*count=h[1];return true;
}
struct map_search {
    uint32_t index[MAP_MAX_FEATURES],result[MAP_SEARCH_RESULTS];
    uint8_t rank[MAP_SEARCH_RESULTS];
    unsigned indexed,count,cursor,offset;
    bool more;
    char query[MAP_SEARCH_QUERY+1];
    const struct map_search_entry *catalogue;
};
static inline const char *map_search_name(const struct map_search *s,const struct offline_map *m,unsigned id){return s->catalogue?s->catalogue[id].name:m->features[id].name;}
static inline unsigned map_search_geometry(const struct map_search *s,const struct offline_map *m,unsigned id){return s->catalogue?s->catalogue[id].geometry:m->features[id].geometry;}
static inline void map_search_fold(char *out,const char *in,size_t size){
    size_t n=0;bool space=false;
    for(;*in&&n+1<size;in++){
        unsigned char c=(unsigned char)*in;
        if(c==' '||c=='\t'){space=n!=0;continue;}
        if(space){if(n+2>=size)break;out[n++]=' ';space=false;}
        out[n++]=(char)(c>='A'&&c<='Z'?c+32:c);
    }
    out[n]=0;
}
static inline int map_search_rank(const char *name,const char *query){
    const char *p=strstr(name,query);if(!*query||!p)return -1;
    if(!strcmp(name,query))return 0;
    if(p==name)return 1;
    return p[-1]==' '||p[-1]=='-'?2:3;
}
static inline void map_search_run(struct map_search *s,const struct offline_map *map){
    s->count=s->cursor=s->offset=0;s->more=false;
    char query[MAP_SEARCH_QUERY+1];map_search_fold(query,s->query,sizeof(query));if(!*query)return;
    for(unsigned i=0;i<s->indexed;i++){
        unsigned id=s->catalogue?i:s->index[i],geometry=map_search_geometry(s,map,id);
        char name[96];map_search_fold(name,map_search_name(s,map,id),sizeof(name));int rank=map_search_rank(name,query);if(rank<0)continue;
        unsigned insert=s->count;bool duplicate=false;
        for(unsigned j=0;j<s->count;j++){
            unsigned old=map_search_geometry(s,map,s->result[j]);char other[96];map_search_fold(other,map_search_name(s,map,s->result[j]),sizeof(other));
            int order=strcmp(name,other);
            if(geometry==old&&!order){duplicate=true;break;}
            if(insert==s->count&&(rank<s->rank[j]||(rank==s->rank[j]&&(geometry<old||(geometry==old&&order<0)))))insert=j;
        }
        if(duplicate)continue;
        if(s->count==MAP_SEARCH_RESULTS){s->more=true;if(insert==s->count)continue;}
        unsigned end=s->count<MAP_SEARCH_RESULTS?s->count:MAP_SEARCH_RESULTS-1;
        for(unsigned j=end;j>insert;j--){s->result[j]=s->result[j-1];s->rank[j]=s->rank[j-1];}
        s->result[insert]=id;s->rank[insert]=(uint8_t)rank;if(s->count<MAP_SEARCH_RESULTS)s->count++;
    }
}
static inline void map_search_build(struct map_search *s,const struct offline_map *map,bool (*eligible)(const struct map_feature *)){
    memset(s,0,sizeof(*s));
    for(unsigned i=0;i<map->feature_count&&i<MAP_MAX_FEATURES;i++)if(map->features[i].name[0]&&eligible(&map->features[i]))s->index[s->indexed++]=i;
}
static inline bool map_search_character(struct map_search *s,uint32_t cp){
    size_t n=strlen(s->query);
    if(cp==8||cp==127){if(!n)return false;n--;while(n&&((unsigned char)s->query[n]&192)==128)n--;s->query[n]=0;return true;}
    if(cp<32||cp>0x10ffff||(cp>=0xd800&&cp<=0xdfff))return false;
    unsigned bytes=cp<128?1:cp<2048?2:cp<65536?3:4;if(bytes>MAP_SEARCH_QUERY-n)return false;
    if(bytes==1)s->query[n++]=(char)cp;
    else {
        s->query[n++]=(char)((bytes==2?0xc0:bytes==3?0xe0:0xf0)|(cp>>(6*(bytes-1))));
        for(unsigned i=bytes-1;i>0;i--)s->query[n++]=(char)(0x80|((cp>>(6*(i-1)))&63));
    }
    s->query[n]=0;return true;
}
