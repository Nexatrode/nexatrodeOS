/* SPDX-License-Identifier: Apache-2.0 */
#include <assert.h>
#include <stdio.h>
#include "../experiments/comaps-native/search.h"
static struct offline_map map;
static struct map_search search;
static bool eligible(const struct map_feature *f){return f->geometry<=1;}
static void add(const char *name,unsigned kind){struct map_feature *f=&map.features[map.feature_count++];snprintf(f->name,sizeof(f->name),"%s",name);f->geometry=kind;}
static void query(const char *q){snprintf(search.query,sizeof(search.query),"%s",q);map_search_run(&search,&map);}
int main(void){
    add("Kingsway",1);add("Kingsway",1);add("Old Kingsway",1);add("Kingsway",0);add("Kingsway North",1);add("The kingsway",1);add("San Marino",0);add("Not searchable",2);add("Café Road",1);
    map_search_build(&search,&map,eligible);assert(search.indexed==8);
    query(" KINGSWAY ");assert(search.count==5&&!search.more);assert(search.result[0]==3&&search.result[1]==0);assert(search.rank[0]==0&&search.rank[2]==1&&search.rank[3]==2);
    query("san   MARINO");assert(search.count==1&&search.result[0]==6);
    query("ing");assert(search.count==5);query("not searchable");assert(search.count==0);
    query("  ");assert(search.count==0);query("café");assert(search.count==1&&search.result[0]==8);
    for(unsigned i=0;i<20;i++){char name[96];snprintf(name,sizeof(name),"Road %02u",i);add(name,1);}
    map_search_build(&search,&map,eligible);query("road");assert(search.count==8&&search.more);
    for(unsigned i=0;i<search.count;i++)for(unsigned j=0;j<i;j++)assert(search.result[i]!=search.result[j]);
    memset(search.query,0,sizeof(search.query));assert(map_search_character(&search,0xe9)&&strlen(search.query)==2);assert(map_search_character(&search,8)&&!search.query[0]);
    assert(!map_search_character(&search,0xd800)&&!map_search_character(&search,0x110000)&&!map_search_character(&search,9));
    for(unsigned i=0;i<MAP_SEARCH_QUERY;i++)assert(map_search_character(&search,'x'));
    assert(!map_search_character(&search,'y')&&!map_search_character(&search,0x1f600));assert(strlen(search.query)==MAP_SEARCH_QUERY);
    map_search_build(&search,&map,eligible);assert(!search.query[0]&&!search.count);
    puts("PASS ranking, ASCII case/space folding, road deduplication, kind separation, capacity, UTF-8 deletion and region reset");
}
