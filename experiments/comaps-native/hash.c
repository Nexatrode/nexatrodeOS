/* SPDX-License-Identifier: Apache-2.0
 * SHA-256 for pinned public asset checksums and selection-record integrity.
 * Fixed-width FIPS 180-4 operations; no credential handling or authentication.
 */
#include "storage_format.h"
#include <string.h>
static uint32_t r(uint32_t x,unsigned n){return x>>n|x<<(32-n);}
static void block(uint32_t h[8],const uint8_t p[64]) {
    static const uint32_t k[64]={
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2};
    uint32_t w[64];for(unsigned i=0;i<16;i++)w[i]=(uint32_t)p[i*4]<<24|(uint32_t)p[i*4+1]<<16|(uint32_t)p[i*4+2]<<8|p[i*4+3];
    for(unsigned i=16;i<64;i++)w[i]=w[i-16]+(r(w[i-15],7)^r(w[i-15],18)^(w[i-15]>>3))+w[i-7]+(r(w[i-2],17)^r(w[i-2],19)^(w[i-2]>>10));
    uint32_t a=h[0],b=h[1],c=h[2],d=h[3],e=h[4],f=h[5],g=h[6],z=h[7];
    /* Eight rounds restore the original register roles. This avoids rotating
     * eight live state words and a loop branch on every round, without relying
     * on optional CPU crypto instructions or changing the SHA-256 algorithm. */
#define ROUND(A,B,C,D,E,F,G,H,I) do { \
    uint32_t t=(H)+(r((E),6)^r((E),11)^r((E),25))+(((E)&(F))^(~(E)&(G)))+k[(I)]+w[(I)]; \
    uint32_t u=(r((A),2)^r((A),13)^r((A),22))+(((A)&(B))^((A)&(C))^((B)&(C))); \
    (D)+=t;(H)=t+u; \
} while(0)
    for(unsigned i=0;i<64;i+=8){
        ROUND(a,b,c,d,e,f,g,z,i);ROUND(z,a,b,c,d,e,f,g,i+1);
        ROUND(g,z,a,b,c,d,e,f,i+2);ROUND(f,g,z,a,b,c,d,e,i+3);
        ROUND(e,f,g,z,a,b,c,d,i+4);ROUND(d,e,f,g,z,a,b,c,i+5);
        ROUND(c,d,e,f,g,z,a,b,i+6);ROUND(b,c,d,e,f,g,z,a,i+7);
    }
#undef ROUND
    h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;h[4]+=e;h[5]+=f;h[6]+=g;h[7]+=z;
}
void map_sha256(const void *data,size_t n,uint8_t out[32]) {
    const uint8_t *p=data;uint64_t bits=(uint64_t)n*8;
    uint32_t h[]={0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19};
    while(n>=64){block(h,p);p+=64;n-=64;}
    uint8_t tail[128]={0};memcpy(tail,p,n);tail[n]=128;unsigned size=n<56?64:128;
    for(unsigned i=0;i<8;i++)tail[size-1-i]=(uint8_t)(bits>>(8*i));
    block(h,tail);if(size==128)block(h,tail+64);
    for(unsigned i=0;i<32;i++)out[i]=(uint8_t)(h[i/4]>>(24-8*(i%4)));
}
