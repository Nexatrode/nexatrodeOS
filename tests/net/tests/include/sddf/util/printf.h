#pragma once
/* Host tests run without a Microkit serial driver. */
#define sddf_printf(...) ((void)0)
#define sddf_dprintf(...) ((void)0)
