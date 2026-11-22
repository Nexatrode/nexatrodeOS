#include <stdatomic.h>
#include <stdint.h>

static _Atomic int slot_state;
static _Atomic int allow_connect;

void test_tcp_reset(void) { slot_state = 0; allow_connect = 0; }
void test_tcp_allow_connect(int value) { allow_connect = value; }
int test_tcp_state(void) { return slot_state; }

int sel4_haskell_dns_start(const uint8_t *name, int length) {
    (void)name; (void)length; return -1;
}
int sel4_haskell_dns_poll(uint8_t *address) { (void)address; return -1; }
int sel4_haskell_tcp_open(uint8_t a, uint8_t b, uint8_t c, uint8_t d,
                          uint16_t port, uint8_t slot) {
    (void)a; (void)b; (void)c; (void)d; (void)port; (void)slot;
    int expected = 0;
    return atomic_compare_exchange_strong(&slot_state, &expected, 1) ? 1 : 2;
}
int sel4_haskell_tcp_status(uint8_t slot) {
    (void)slot;
    if (slot_state == 1 && allow_connect < 0) return -1;
    if (slot_state == 1 && allow_connect) slot_state = 2;
    return slot_state == 2 ? 1 : 0;
}
int sel4_haskell_tcp_send(const uint8_t *data, int length, uint8_t slot) {
    (void)data; (void)slot; return length;
}
int sel4_haskell_tcp_recv(uint8_t *data, int length, uint8_t slot) {
    (void)data; (void)length; (void)slot; return 0;
}
int sel4_haskell_tcp_close(uint8_t slot) { (void)slot; slot_state = 0; return 1; }
void sel4_haskell_tcp_cancel(uint8_t slot) { (void)slot; slot_state = 0; }
