/*
 * Freestanding compatibility calls used by SQLCipher's amalgamation.
 *
 * The VFS owns database filenames and atomic file operations, so a POSIX
 * rename path is deliberately unavailable. SQLCipher's migration helper can
 * therefore fail cleanly until it is rewritten to use the VFS API.
 */
#include "sqlcipher_freestanding.h"

#include <errno.h>
#include <limits.h>
#include <sqlite3.h>

int atoi(const char *text)
{
    if (text == 0) return 0;

    while (*text == ' ' || (*text >= '\t' && *text <= '\r')) ++text;
    int negative = 0;
    if (*text == '+' || *text == '-') negative = *text++ == '-';

    unsigned int limit = negative ? (unsigned int)INT_MAX + 1u : (unsigned int)INT_MAX;
    unsigned int value = 0;
    while (*text >= '0' && *text <= '9') {
        unsigned int digit = (unsigned int)(*text++ - '0');
        if (value > (limit - digit) / 10u) {
            value = limit;
            while (*text >= '0' && *text <= '9') ++text;
            break;
        }
        value = value * 10u + digit;
    }

    if (negative) {
        if (value == (unsigned int)INT_MAX + 1u) return INT_MIN;
        return -(int)value;
    }
    return (int)value;
}

int rename(const char *old_path, const char *new_path)
{
    (void)old_path;
    (void)new_path;
    errno = ENOSYS;
    return -1;
}

/* Fetch cryptographic seed bytes directly from SQLite's capability VFS. */
int sqlite3_secure_randomness(int amount, void *buffer)
{
    if (amount < 0 || (amount != 0 && buffer == 0)) return SQLITE_MISUSE;
    sqlite3_vfs *vfs = sqlite3_vfs_find(0);
    if (vfs == 0 || vfs->xRandomness == 0) return SQLITE_ERROR;
    return vfs->xRandomness(vfs, amount, (char *)buffer) == amount
               ? SQLITE_OK
               : SQLITE_ERROR;
}

/* Logging is disabled in this embedded configuration. */
void sqlcipher_log(unsigned int level, const char *message, ...)
{
    (void)level;
    (void)message;
}
