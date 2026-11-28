/* SPDX-License-Identifier: Apache-2.0 */
#include "spatial.h"
#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
static struct offline_map m;
int main(int argc,char **argv){
    assert(argc==2||argc==3);FILE *f=fopen(argv[1],"rb");assert(f);fseek(f,0,SEEK_END);size_t n=ftell(f);rewind(f);
    unsigned char *b=malloc(n),*c=malloc(n);assert(b&&c&&fread(b,1,n,f)==n);fclose(f);
    assert(map_page_append(&m,b,n,NULL));unsigned count=m.feature_count,points=m.point_count;
    memset(&m,0,sizeof(m));double outside[]={1000,1000,1001,1001};assert(map_page_append(&m,b,n,outside)&&!m.feature_count&&!m.point_count);
    for(size_t i=0;i<n;i+=17003){memset(&m,0,sizeof(m));assert(!map_page_append(&m,b,i,NULL));}
    if(count){memset(&m,0,sizeof(m));m.feature_count=MAP_MAX_FEATURES;assert(!map_page_append(&m,b,n,NULL));
    memset(&m,0,sizeof(m));m.point_count=MAP_MAX_POINTS;assert(!map_page_append(&m,b,n,NULL));}
    memcpy(c,b,n);c[0]^=128;assert(!map_page_append(&m,c,n,NULL));
    uint32_t seed=91;for(unsigned t=0;t<(argc==3?0:200);t++){memcpy(c,b,n);for(unsigned i=0;i<8;i++){seed=seed*1664525+1013904223;c[seed%n]^=seed>>24;}memset(&m,0,sizeof(m));map_page_append(&m,c,n,NULL);}
    printf("PASS page: %u features %u points; bounds, truncations, capacity, %u mutations\n",count,points,argc==3?0:200);free(c);free(b);
}
