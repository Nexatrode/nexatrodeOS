#ifndef GHC_MICROKIT_TERMIOS_H
#define GHC_MICROKIT_TERMIOS_H

#include <sys/types.h>
typedef unsigned int tcflag_t;
typedef unsigned char cc_t;
typedef unsigned int speed_t;

struct termios {
    tcflag_t c_iflag;
    tcflag_t c_oflag;
    tcflag_t c_cflag;
    tcflag_t c_lflag;
    cc_t c_cc[32];
};

#define ECHO 0000010
#define ICANON 0000002
#define TCSANOW 0
#define VMIN 6
#define VTIME 5

int tcgetattr(int fd, struct termios *attributes);
int tcsetattr(int fd, int action, const struct termios *attributes);

#endif
