/* Offline build declarations only. The guest has no network service capability. */
#pragma once
#include <sys/select.h>
#include <sys/types.h>
typedef unsigned int socklen_t;
#define AF_INET 2
#define AF_INET6 10
#define SOCK_STREAM 1
int socket(int, int, int);
