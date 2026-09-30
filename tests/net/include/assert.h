/* Freestanding shim for Mbed TLS on the Personal OS net fragment. */
#pragma once

#include <sddf/util/util.h>

#ifndef assert
#define assert(expr) do { if (!(expr)) { while (1) {} } } while (0)
#endif
