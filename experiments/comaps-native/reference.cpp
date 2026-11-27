// SPDX-License-Identifier: Apache-2.0
// Host-only comparison against the pinned, unmodified CoMaps geometry decoder.
#include "coding/geometry_coding.hpp"
#include "coding/point_coding.hpp"
#include "base/src_point.hpp"
extern "C" {
#include "mwm.h"
}
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>
#include <vector>
namespace base {
static bool failed(SrcPoint const &,std::string const &message){std::fprintf(stderr,"CoMaps reference assertion: %s\n",message.c_str());std::abort();}
AssertFailedFn OnAssertFailed=failed;
void SrcPoint::TruncateFileName() {}
}
static unsigned verified;
extern "C" bool map_reference(const unsigned char *raw,size_t bytes,unsigned count,unsigned kind,
                              unsigned bits,uint32_t bx,uint32_t by,const map_point *actual,
                              unsigned total,double ox,double oy) {
    serial::DeltasT deltas;
    if(count) ReadVarUint64Array(reinterpret_cast<const char *>(raw),count,base::MakeBackInsertFunctor(deltas));
    else ReadVarUint64Array(reinterpret_cast<const char *>(raw),reinterpret_cast<const char *>(raw+bytes),base::MakeBackInsertFunctor(deltas));
    std::vector<m2::PointU> result(deltas.size()*3);
    coding::OutPointsT out(result);
    auto input=make_read_adapter(deltas);
    m2::PointU base(bx,by),max((1ULL<<bits)-1,(1ULL<<bits)-1);
    if(kind==0)coding::DecodePolyline(input,base,max,out);
    else if(kind==1) {
        std::vector<m2::PointU> strip(deltas.size());coding::OutPointsT stripOut(strip);
        coding::DecodeTriangleStrip(input,base,max,stripOut);
        result.clear();strip.resize(stripOut.size());
        for(size_t i=2;i<strip.size();i++)for(size_t j=i-2;j<=i;j++)result.push_back(strip[j]);
    } else serial::DecodeTriangles(input,base,max,out);
    if(kind!=1)result.resize(out.size());
    if(result.size()!=total)return false;
    for(unsigned i=0;i<total;i++) {
        auto p=PointUToPointD(result[i],bits);
        if(actual[i].x!=static_cast<float>(p.x-ox)||actual[i].y!=static_cast<float>(p.y-oy)) {
            std::fprintf(stderr,"Reference geometry mismatch block %u point %u\n",verified,i);return false;
        }
    }
    verified++;return true;
}
static offline_map map;
int main(int argc,char **argv) {
    if(argc!=2)return 2;
    std::ifstream stream(argv[1],std::ios::binary);
    std::vector<unsigned char> blob((std::istreambuf_iterator<char>(stream)),{});
    if(!map_load(&map,blob.data(),blob.size())) {std::fprintf(stderr,"Reader failed %s at record %u\n",map.error,map.records);return 1;}
    if(map.records!=8957||map.point_count!=140685||verified<6000)return 1;
    std::printf("PASS %u geometry blocks compared point-for-point against unmodified CoMaps; records=%u points=%u\n",verified,map.records,map.point_count);
    return 0;
}
