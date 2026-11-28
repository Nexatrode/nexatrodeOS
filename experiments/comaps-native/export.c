/* SPDX-License-Identifier: Apache-2.0
 * Build-time exporter, never linked into the guest. Same bounded MWM decoder. */
#include "mwm.h"
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
static struct offline_map map;
static unsigned long long points,features;
static bool visit(const struct offline_map *m,const struct map_feature *f,void *v){
    FILE *out=v;points+=f->count;features++;
    return fwrite(f,sizeof(*f),1,out)==1&&fwrite(m->points,sizeof(struct map_point),f->count,out)==f->count;
}
int main(int argc,char **argv){
    if(argc!=3)return 2;FILE *in=fopen(argv[1],"rb"),*out=fopen(argv[2],"wb");if(!in||!out)return 2;
    fseek(in,0,SEEK_END);long n=ftell(in);rewind(in);if(n<16||n>32*1024*1024)return 2;
    void *bytes=malloc(n);if(!bytes||fread(bytes,1,n,in)!=(size_t)n)return 2;fclose(in);
    clock_t start=clock();bool ok=map_visit(&map,bytes,n,visit,out);fclose(out);free(bytes);
    if(!ok){fprintf(stderr,"export failed %s at %u\n",map.error,map.records);return 1;}
    printf("{\"features\":%llu,\"points\":%llu,\"decode_cpu_ms\":%.3f,\"workspace_bytes\":%zu,\"feature_bytes\":%zu,\"bounds\":[%.12g,%.12g,%.12g,%.12g],\"origin\":[%.12g,%.12g]}\n",features,points,1000.*(clock()-start)/CLOCKS_PER_SEC,sizeof(map),sizeof(struct map_feature),map.minx,map.miny,map.maxx,map.maxy,map.origin_x,map.origin_y);
}
