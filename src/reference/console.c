#include "protocol.h"

#include <stddef.h>
#include <stdint.h>

volatile uintptr_t uart_regs;

#define UART_DR    0x00
#define UART_FR    0x18
#define UART_IBRD  0x24
#define UART_FBRD  0x28
#define UART_LCRH  0x2c
#define UART_CR    0x30
#define UART_IMSC  0x38
#define UART_ICR   0x44

#define UART_FR_RXFE (1u << 4)
#define UART_INT_RX  (1u << 4)
#define UART_INT_RT  (1u << 6)
#define UART_INT_RX_ALL (UART_INT_RX | UART_INT_RT)
#define LINE_CAPACITY 64

static volatile uint32_t *const uart = (volatile uint32_t *)0x51000000;
static char line[LINE_CAPACITY];
static size_t line_length;
static seL4_Word heartbeat_bucket;
static seL4_Bool line_too_long;

static void print_word(seL4_Word value)
{
    char digits[3 * sizeof(value) + 1];
    size_t count = 0;
    do {
        digits[count++] = (char)('0' + value % 10);
        value /= 10;
    } while (value != 0);
    while (count != 0) {
        char digit[2] = {digits[--count], '\0'};
        microkit_dbg_puts(digit);
    }
}

static void prompt(void)
{
    microkit_dbg_puts("> ");
    if (line_length != 0) {
        line[line_length] = '\0';
        microkit_dbg_puts(line);
    }
}

static void print_reply_error(microkit_msginfo reply)
{
    seL4_Word label = microkit_msginfo_get_label(reply);
    if (label == REPLY_OVERFLOW) {
        microkit_dbg_puts("error: unsigned addition overflow\r\n");
    } else {
        microkit_dbg_puts("error: request rejected by service\r\n");
    }
}

static void skip_spaces(const char **cursor)
{
    while (**cursor == ' ' || **cursor == '\t') {
        (*cursor)++;
    }
}

static seL4_Bool read_uptime(seL4_Word *seconds)
{
    microkit_msginfo reply = microkit_ppcall(
        CONSOLE_CALL_CLOCK_CHANNEL,
        microkit_msginfo_new(REQUEST_CLOCK_STATUS, 0));
    if (microkit_msginfo_get_label(reply) != REPLY_OK ||
        microkit_msginfo_get_count(reply) != 1) {
        return seL4_False;
    }
    *seconds = microkit_mr_get(0);
    return seL4_True;
}

static seL4_Bool parse_word(const char **cursor, seL4_Word *value)
{
    seL4_Word parsed = 0;
    seL4_Bool found_digit = seL4_False;
    const seL4_Word maximum = ~(seL4_Word)0;

    while (**cursor >= '0' && **cursor <= '9') {
        seL4_Word digit = (seL4_Word)(**cursor - '0');
        if (parsed > (maximum - digit) / 10) {
            return seL4_False;
        }
        parsed = parsed * 10 + digit;
        found_digit = seL4_True;
        (*cursor)++;
    }
    *value = parsed;
    return found_digit;
}

static void run_calculation(const char *cursor)
{
    seL4_Word left;
    seL4_Word right;
    skip_spaces(&cursor);
    if (!parse_word(&cursor, &left)) {
        microkit_dbg_puts("usage: calc NUMBER + NUMBER\r\n");
        return;
    }
    skip_spaces(&cursor);
    if (*cursor++ != '+') {
        microkit_dbg_puts("usage: calc NUMBER + NUMBER\r\n");
        return;
    }
    skip_spaces(&cursor);
    if (!parse_word(&cursor, &right)) {
        microkit_dbg_puts("usage: calc NUMBER + NUMBER\r\n");
        return;
    }
    skip_spaces(&cursor);
    if (*cursor != '\0') {
        microkit_dbg_puts("usage: calc NUMBER + NUMBER\r\n");
        return;
    }

    microkit_mr_set(0, left);
    microkit_mr_set(1, right);
    microkit_msginfo reply = microkit_ppcall(
        CONSOLE_CALL_CLIENT_CHANNEL,
        microkit_msginfo_new(REQUEST_CONSOLE_CALC, 2));
    if (microkit_msginfo_get_label(reply) != REPLY_OK ||
        microkit_msginfo_get_count(reply) != 1) {
        print_reply_error(reply);
        return;
    }
    microkit_dbg_puts("= ");
    print_word(microkit_mr_get(0));
    microkit_dbg_puts("\r\n");
}

static void print_status(void)
{
    microkit_msginfo reply = microkit_ppcall(
        CONSOLE_CALL_CLIENT_CHANNEL,
        microkit_msginfo_new(REQUEST_CONSOLE_STATUS, 0));
    if (microkit_msginfo_get_label(reply) != REPLY_OK ||
        microkit_msginfo_get_count(reply) != 1) {
        print_reply_error(reply);
        return;
    }
    seL4_Word handled_faults = microkit_mr_get(0);

    seL4_Word seconds;
    if (!read_uptime(&seconds)) {
        microkit_dbg_puts("error: clock service did not respond\r\n");
        return;
    }

    microkit_dbg_puts("uptime: ");
    print_word(seconds);
    microkit_dbg_puts(" seconds\r\ncomponents: console, clock, supervisor, calculator, crasher\r\n");
    microkit_dbg_puts("isolated faults handled: ");
    print_word(handled_faults);
    microkit_dbg_puts("\r\n");
}

static void request_crash(void)
{
    microkit_msginfo reply = microkit_ppcall(
        CONSOLE_CALL_CLIENT_CHANNEL,
        microkit_msginfo_new(REQUEST_CONSOLE_CRASH, 0));
    if (microkit_msginfo_get_label(reply) != REPLY_OK ||
        microkit_msginfo_get_count(reply) != 0) {
        print_reply_error(reply);
        return;
    }
    microkit_notify(CONSOLE_START_CRASHER_CHANNEL);
    microkit_dbg_puts("crasher restarted; watching the supervisor handle its fault\r\n");
}

static void execute_line(void)
{
    line[line_length] = '\0';
    if (line_too_long) {
        microkit_dbg_puts("error: command is too long\r\n");
    } else if (line_length == 0) {
        /* An empty command only redraws the prompt. */
    } else if (line[0] == 'h' && line[1] == 'e' && line[2] == 'l' &&
               line[3] == 'p' && line[4] == '\0') {
        microkit_dbg_puts("help          show commands\r\n"
                          "status        show uptime and component state\r\n"
                          "calc A + B    add two unsigned integers in the isolated service\r\n"
                          "crash         restart the isolated fault demonstration\r\n");
    } else if (line[0] == 's' && line[1] == 't' && line[2] == 'a' &&
               line[3] == 't' && line[4] == 'u' && line[5] == 's' &&
               line[6] == '\0') {
        print_status();
    } else if (line[0] == 'c' && line[1] == 'a' && line[2] == 'l' &&
               line[3] == 'c' && (line[4] == ' ' || line[4] == '\t')) {
        run_calculation(line + 5);
    } else if (line[0] == 'c' && line[1] == 'r' && line[2] == 'a' &&
               line[3] == 's' && line[4] == 'h' && line[5] == '\0') {
        request_crash();
    } else {
        microkit_dbg_puts("unknown command; enter help\r\n");
    }
}

static void receive_character(char input)
{
    /* UTM/QEMU virtio-serial often delivers Enter as CR only. */
    if (input == '\r' || input == '\n') {
        microkit_dbg_puts("\r\n");
        execute_line();
        line_length = 0;
        line_too_long = seL4_False;
        prompt();
        return;
    }
    if (input == '\b' || input == 0x7f) {
        if (line_length != 0) {
            line_length--;
            microkit_dbg_puts("\b \b");
        }
        return;
    }
    if (input < 0x20 || input > 0x7e) {
        return;
    }
    if (line_length == LINE_CAPACITY - 1) {
        line_too_long = seL4_True;
        return;
    }

    line[line_length++] = input;
    char echo[2] = {input, '\0'};
    microkit_dbg_puts(echo);
}

void init(void)
{
    /* QEMU virt's PL011 UART0 uses a 24 MHz input clock. */
    uart[UART_CR / sizeof(*uart)] = 0;
    uart[UART_IBRD / sizeof(*uart)] = 13;
    uart[UART_FBRD / sizeof(*uart)] = 1;
    uart[UART_LCRH / sizeof(*uart)] = 0x70; /* FIFO, 8 data bits, no parity. */
    uart[UART_ICR / sizeof(*uart)] = 0x7ff;
    uart[UART_IMSC / sizeof(*uart)] = UART_INT_RX_ALL;
    uart[UART_CR / sizeof(*uart)] = 0x301; /* UART, transmitter, receiver. */
    microkit_dbg_puts("Personal OS console; enter help for commands.\r\n");
    prompt();
}

void notified(microkit_channel channel)
{
    if (channel == TIMER_TO_CONSOLE_CHANNEL) {
        seL4_Word seconds;
        if (!read_uptime(&seconds)) {
            microkit_dbg_puts("error: clock service did not respond\r\n");
            prompt();
            return;
        }
        if (seconds / 5 > heartbeat_bucket) {
            heartbeat_bucket = seconds / 5;
            microkit_dbg_puts("\r\n[heartbeat: ");
            print_word(seconds);
            microkit_dbg_puts(" seconds; console and clock responded]\r\n");
            prompt();
        }
        return;
    }
    if (channel == CLIENT_TO_CONSOLE_CHANNEL) {
        microkit_dbg_puts("\r\n[boot isolation checks complete; console ready]\r\n");
        prompt();
        return;
    }
    if (channel == UART_IRQ_CHANNEL) {
        while ((uart[UART_FR / sizeof(*uart)] & UART_FR_RXFE) == 0) {
            receive_character((char)(uart[UART_DR / sizeof(*uart)] & 0xff));
        }
        /* Clear PL011 receive/timeout causes before unmasking the GIC line. */
        uart[UART_ICR / sizeof(*uart)] = UART_INT_RX_ALL;
        microkit_irq_ack(channel);
        return;
    }
    microkit_dbg_puts("CONSOLE|ERROR: unexpected notification\r\n");
}
