
#ifndef GUEST_MATH_H
#define GUEST_MATH_H
#ifdef __cplusplus
extern "C" {
#endif
#define FP_NAN 0
#define FP_INFINITE 1
#define FP_ZERO 2
#define FP_SUBNORMAL 3
#define FP_NORMAL 4
#define MATH_ERRNO 1
#define MATH_ERREXCEPT 2
#define math_errhandling MATH_ERRNO

double sin(double); float sinf(float); long double sinl(long double);
double cos(double); float cosf(float); long double cosl(long double);
double tan(double); float tanf(float); long double tanl(long double);
double asin(double); float asinf(float);
double acos(double); float acosf(float);
double atan(double); float atanf(float);
double atan2(double, double); float atan2f(float, float);
double sinh(double); float sinhf(float);
double cosh(double); float coshf(float);
double tanh(double); float tanhf(float);
double exp(double); float expf(float);
double exp2(double); float exp2f(float);
double log(double); float logf(float);
double log2(double); float log2f(float);
double log10(double); float log10f(float);
double pow(double, double); float powf(float, float);
double sqrt(double); float sqrtf(float);
double cbrt(double); float cbrtf(float);
double fabs(double); float fabsf(float);
double floor(double); float floorf(float);
double ceil(double); float ceilf(float);
double round(double); float roundf(float);
double trunc(double); float truncf(float);
double nearbyint(double); float nearbyintf(float);
double rint(double); float rintf(float);
double fma(double, double, double); float fmaf(float, float, float);
long lrint(double); long lrintf(float);
long lround(double); long lroundf(float);
long long llrint(double); long long llrintf(float);
long long llround(double); long long llroundf(float);
double fmod(double, double); float fmodf(float, float);
double fmin(double, double); float fminf(float, float);
double fmax(double, double); float fmaxf(float, float);
double copysign(double, double); float copysignf(float, float);
double hypot(double, double); float hypotf(float, float);
double nextafter(double, double); float nextafterf(float, float);
double remainder(double, double); float remainderf(float, float);
double scalbn(double, int); float scalbnf(float, int);
int ilogb(double); int ilogbf(float);
double logb(double); float logbf(float);
double log1p(double); float log1pf(float);
double expm1(double); float expm1f(float);
double erf(double); float erff(float);
double erfc(double); float erfcf(float);
double tgamma(double); float tgammaf(float);
double lgamma(double); float lgammaf(float);
double ldexp(double, int); float ldexpf(float, int);
double frexp(double, int *); float frexpf(float, int *);
double modf(double, double *); float modff(float, float *);
int isnan(double); int isinf(double); int finite(double);
#define NAN (__builtin_nanf(""))
#define INFINITY (__builtin_inff())
#define HUGE_VAL (__builtin_huge_val())
#define HUGE_VALF (__builtin_huge_valf())

#ifdef __cplusplus
}
#endif
#endif
