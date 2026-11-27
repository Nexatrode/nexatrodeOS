/* SPDX-License-Identifier: Apache-2.0 */
#include "mwm.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static struct offline_map map;
static void reject(unsigned char *bytes,size_t n){assert(!map_load(&map,bytes,n));}
int main(int argc,char **argv) {
    assert(argc==2);FILE *f=fopen(argv[1],"rb");assert(f);
    assert(!fseek(f,0,SEEK_END));size_t n=ftell(f);rewind(f);
    unsigned char *bytes=malloc(n),*copy=malloc(n);assert(bytes&&copy);assert(fread(bytes,1,n,f)==n);fclose(f);
    assert(map_load(&map,bytes,n));assert(map.records==8957&&map.feature_count==8957&&map.point_count==140685);
    unsigned points=0,lines=0,areas=0;bool gibraltar=false;
    for(unsigned i=0;i<map.feature_count;i++) {
        struct map_feature *a=&map.features[i];assert(a->count&&a->first+a->count<=map.point_count);
        if(!a->geometry)points++;else if(a->geometry==1)lines++;else {areas++;assert(a->count%3==0);}
        if(!strcmp(a->name,"Gibraltar"))gibraltar=true;
    }
    assert(points>1000&&lines>1000&&areas>1000&&gibraltar);
    for(size_t end=0;end<n;end+=(end<32?1:17003))reject(bytes,end);
    memcpy(copy,bytes,n);memset(copy,255,8);reject(copy,n); // outside section table
    memcpy(copy,bytes,n);copy[64]=127;reject(copy,n);       // unsupported DAT version
    memcpy(copy,bytes,n);copy[80]=0;reject(copy,n);         // zero-length feature
    memcpy(copy,bytes,n);memset(copy+80,255,11);reject(copy,n); // overflow varint
    memcpy(copy,bytes,n);copy[83]=0x10;reject(copy,n);      // outer geometry without any scale
    uint32_t seed=0x6e657861;
    for(unsigned trial=0;trial<300;trial++) {
        memcpy(copy,bytes,n);
        for(unsigned i=0;i<8;i++){seed=seed*1664525+1013904223;size_t at=seed%n;copy[at]^=(unsigned char)(seed>>24);}
        // Some mutations are valid changes to unused metadata; neither result
        // may escape bounds or trigger undefined behavior under sanitizers.
        map_load(&map,copy,n);
    }
    free(copy);free(bytes);
    printf("PASS MWM reader: full region, truncated inputs, bad table/version/feature/varint/geometry and 300 deterministic mutations\n");
    return 0;
}
