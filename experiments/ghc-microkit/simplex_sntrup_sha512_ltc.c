/*
 * Implement the public-domain SimpleX SNTRUP SHA-512 hook using the same
 * target-built LibTomCrypt implementation already used by SQLCipher.
 */
#include <limits.h>
#include <tomcrypt.h>

void crypto_hash_sha512(unsigned char *out, const unsigned char *in,
                        unsigned long long inlen)
{
    hash_state state;

    if (sha512_init(&state) != CRYPT_OK) __builtin_trap();
    while (inlen != 0) {
        unsigned long chunk = inlen > ULONG_MAX ? ULONG_MAX : (unsigned long)inlen;
        if (sha512_process(&state, in, chunk) != CRYPT_OK) __builtin_trap();
        in += chunk;
        inlen -= chunk;
    }
    if (sha512_done(&state, out) != CRYPT_OK) __builtin_trap();
}
