#ifndef _CTYPE_H
#define _CTYPE_H

static inline int isspace(int c)
{
    return c == ' ' || (c >= '\t' && c <= '\r');
}

static inline int isdigit(int c)
{
    return c >= '0' && c <= '9';
}

#endif
