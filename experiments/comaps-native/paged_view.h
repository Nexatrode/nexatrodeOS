/* SPDX-License-Identifier: Apache-2.0 */
#include "paged_format.h"
static struct map_page_pin viewer_pages[MAP_PAGE_COUNT],viewer_search;
static double viewer_origin[2],viewer_bounds[4];
static void viewer_builtin(void){memcpy(viewer_pages,map_pages,sizeof(viewer_pages));viewer_search=map_search_pin;memcpy(viewer_origin,map_spatial_origin,sizeof(viewer_origin));memcpy(viewer_bounds,map_spatial_bounds,sizeof(viewer_bounds));}
static bool region_spatial(unsigned r){return r==2||(r<MAP_REGION_COUNT&&import_regions[r].present==2);}
static bool viewer_directory(const void*bytes,unsigned region){
 struct map_directory d;if(!map_directory_read(bytes,&d)||d.length!=import_regions[region].length||d.version!=import_regions[region].version||memcmp(d.identity,import_regions[region].identity,16))return false;
 for(unsigned i=0;i<MAP_PAGE_COUNT+1;i++){struct map_page_pin*p=i<MAP_PAGE_COUNT?&viewer_pages[i]:&viewer_search;const struct map_segment*s=&d.segments[i];p->offset=s->offset;p->length=s->length;memcpy(p->digest,s->digest,32);memcpy(p->bounds,s->bounds,32);}
 memcpy(viewer_origin,d.origin,16);memcpy(viewer_bounds,d.bounds,32);return true;
}
#define map_pages viewer_pages
#define map_search_pin viewer_search
#define map_spatial_origin viewer_origin
#define map_spatial_bounds viewer_bounds
