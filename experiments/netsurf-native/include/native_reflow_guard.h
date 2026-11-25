/* SPDX-License-Identifier: GPL-2.0-only */
#pragma once
#include <stdbool.h>
void native_reflow_alloc_begin(void);
bool native_reflow_alloc_end(void);
struct content;
void native_reflow_stop(struct content *content);
void native_reflow_layout_begin(void);
#ifdef NATIVE_BROWSER_TESTS
void native_reflow_test_arm(unsigned allocation);
#endif
