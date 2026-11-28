/* SPDX-License-Identifier: Apache-2.0
 * Bounded subset of the CoMaps MWM reader, adapted November 2026.
 * See NOTICE for upstream provenance and the supported format boundary.
 */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define MAP_MAX_FEATURES 32768
#define MAP_MAX_POINTS 524288
#define MAP_MAX_GEOMETRY 32768
struct map_point { float x, y; };
struct map_feature {
    uint32_t first, count;
    float minx, miny, maxx, maxy;
    uint16_t type, types[8];
    uint8_t type_count;
    uint8_t geometry; /* 0 point, 1 polyline, 2 independent triangles */
    char name[96];
};
struct offline_map {
    struct map_feature features[MAP_MAX_FEATURES];
    struct map_point points[MAP_MAX_POINTS];
    uint32_t feature_count, point_count, records, inner, outer;
    double origin_x, origin_y, minx, miny, maxx, maxy;
    uint8_t coord_bits, dat_version;
    const char *error;
};
bool map_load(struct offline_map *, const unsigned char *, size_t);

/* Streaming exporter: one bounded feature at a time, fixed decoder workspace.
 * Callback pointers are valid only until the next feature. */
typedef bool (*map_visitor)(const struct offline_map *,const struct map_feature *,void *);
bool map_visit(struct offline_map *,const unsigned char *,size_t,map_visitor,void *);
