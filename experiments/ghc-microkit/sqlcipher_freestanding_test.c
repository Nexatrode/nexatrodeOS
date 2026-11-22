#include "sqlcipher_freestanding.h"

#include <assert.h>
#include <errno.h>
#include <sqlite3.h>
#include <stdio.h>
#include <string.h>

static int short_entropy;

static int test_randomness(sqlite3_vfs *vfs, int amount, char *buffer)
{
    (void)vfs;
    memset(buffer, 0xA7, (size_t)amount);
    return short_entropy && amount > 0 ? amount - 1 : amount;
}

int main(void)
{
    assert(atoi("  -2147483648tail") == (-2147483647 - 1));
    assert(atoi("+42") == 42);
    assert(atoi("not a number") == 0);

    sqlite3_vfs *original = sqlite3_vfs_find(NULL);
    assert(original != NULL);
    sqlite3_vfs test_vfs = *original;
    test_vfs.zName = "nexatrode-test-entropy";
    test_vfs.xRandomness = test_randomness;
    assert(sqlite3_vfs_register(&test_vfs, 1) == SQLITE_OK);

    unsigned char bytes[16];
    assert(sqlite3_secure_randomness((int)sizeof(bytes), bytes) == SQLITE_OK);
    for (size_t i = 0; i < sizeof(bytes); ++i) assert(bytes[i] == 0xA7);

    short_entropy = 1;
    assert(sqlite3_secure_randomness((int)sizeof(bytes), bytes) == SQLITE_ERROR);
    assert(sqlite3_secure_randomness(-1, bytes) == SQLITE_MISUSE);
    assert(sqlite3_secure_randomness(1, NULL) == SQLITE_MISUSE);

    errno = 0;
    assert(rename("old", "new") == -1);
    assert(errno == ENOSYS);
    puts("SQLCIPHER_FREESTANDING|PASS|strict VFS entropy and unsupported rename");
    return 0;
}
