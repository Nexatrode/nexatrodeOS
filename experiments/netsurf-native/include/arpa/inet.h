#pragma once
#include <sys/socket.h>
#include <netinet/in.h>
int inet_pton(int, const char *, void *);
int inet_aton(const char *, struct in_addr *);
