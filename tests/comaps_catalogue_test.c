/* SPDX-License-Identifier: Apache-2.0 */
#include <assert.h>
#include <stdlib.h>
#include <stdio.h>
#include "../experiments/comaps-native/search.h"
static struct map_search_entry entries[MAP_SEARCH_CATALOGUE_MAX],before[MAP_SEARCH_CATALOGUE_MAX];
static struct offline_map map;
static struct map_search search;
static const double bounds[]={-1,-1,1,1};
int main(int argc,char **argv){
    uint8_t bytes[16+3*sizeof(*entries)]={0};uint32_t h[]={0x3153584e,3,108,0};memcpy(bytes,h,16);
    struct map_search_entry data[]={ {"Far Road",.5f,-.5f,1},{"Far Road",.7f,.7f,1},{"Far Village",-.8f,.8f,0} };
    memcpy(bytes+16,data,sizeof(data));unsigned count=0;
    assert(map_search_catalogue_read(entries,&count,bytes,sizeof(bytes),bounds)&&count==3);memcpy(before,entries,sizeof(entries));
    search.catalogue=entries;search.indexed=count;strcpy(search.query,"far");map_search_run(&search,&map);
    assert(search.count==2&&search.result[0]==2&&search.result[1]==0); /* No rendering geometry loaded. */
    for(size_t n=0;n<sizeof(bytes);n++)assert(!map_search_catalogue_read(entries,&count,bytes,n,bounds));
    for(unsigned i=0;i<4;i++){
        uint32_t old;memcpy(&old,bytes+i*4,4);uint32_t bad=i==1?MAP_SEARCH_CATALOGUE_MAX+1:99;memcpy(bytes+i*4,&bad,4);
        assert(!map_search_catalogue_read(entries,&count,bytes,sizeof(bytes),bounds));memcpy(bytes+i*4,&old,4);
    }
    for(unsigned i=0;i<5;i++){
        struct map_search_entry bad=data[2];
        if(i==0)memset(bad.name,'x',96);if(i==1)bad.name[0]=0;if(i==2)bad.geometry=2;if(i==3)bad.x=NAN;if(i==4)bad.y=2;
        memcpy(bytes+16+2*sizeof(bad),&bad,sizeof(bad));assert(!map_search_catalogue_read(entries,&count,bytes,sizeof(bytes),bounds));
        assert(count==3&&!memcmp(entries,before,sizeof(entries)));
    }
    if(argc==2){
        FILE *f=fopen(argv[1],"rb");assert(f);fseek(f,0,SEEK_END);long n=ftell(f);rewind(f);assert(n>16&&n<1024*1024);
        uint8_t *p=malloc(n);assert(p&&fread(p,1,n,f)==(size_t)n);fclose(f);
        assert(map_search_catalogue_read(entries,&count,p,n,bounds)&&count==3275);
        search.catalogue=entries;search.indexed=count;
        for(unsigned i=0;i<2;i++){strcpy(search.query,i?"ruggell":"altneugut");map_search_run(&search,&map);assert(search.count&&search.rank[0]==0);assert(map_search_geometry(&search,&map,search.result[0])==(i?0:1));}
        free(p);
    }
    puts("PASS whole-region search without geometry, duplicate roads, anchors, malformed/truncated catalogue and atomic rejection");
}
