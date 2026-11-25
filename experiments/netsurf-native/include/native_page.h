/* SPDX-License-Identifier: GPL-2.0-only */
#pragma once
#include <stdbool.h>
struct nsurl;
struct hlcache_handle;
void native_page_begin(const struct nsurl *url);
void native_page_redirect(const struct nsurl *url);
void native_page_content_event(struct hlcache_handle *content,int event,const char *error);
void native_page_stopped(void);
void native_page_failure(const char *error);
void native_page_layout_notice(bool failed);
const char *native_page_status(void);
const char *native_page_target(void);
bool native_page_loading(void);
bool native_page_error(void);
void native_ui_status_changed(void);
