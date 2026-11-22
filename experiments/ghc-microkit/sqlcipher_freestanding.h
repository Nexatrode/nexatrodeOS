/* Narrow declarations needed by the pinned SQLCipher amalgamation. */
#ifndef NEXATRODE_SQLCIPHER_FREESTANDING_H
#define NEXATRODE_SQLCIPHER_FREESTANDING_H

int atoi(const char *text);
int rename(const char *old_path, const char *new_path);
int sqlite3_secure_randomness(int amount, void *buffer);

#endif
