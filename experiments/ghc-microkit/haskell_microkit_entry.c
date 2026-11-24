/*
 * Minimal single-component entry adapter for a standalone GHC Main module.
 * Microkit owns the ELF's C main(); init starts GHC directly instead of
 * linking GHC's generated process-style main() wrapper.
 */
#include <microkit.h>
#include <Rts.h>
#include <rts/Main.h>

extern StgClosure ZCMain_main_closure;
void sel4_simplex_ipc_init(void) __attribute__((weak));
void sel4_simplex_ipc_notified(microkit_channel channel) __attribute__((weak));
#ifdef SIMPLEX_WAIT_FOR_WRAPPED_VAULT
void simplex_wait_for_vault(void);
#endif

void init(void)
{
    if (sel4_simplex_ipc_init != 0) {
        sel4_simplex_ipc_init();
    }
#ifdef SIMPLEX_WAIT_FOR_WRAPPED_VAULT
    simplex_wait_for_vault();
#endif
    static char program_name[] = "haskell-probe";
    char *argv[] = { program_name, NULL };
    int argc = 1;
    RtsConfig config = defaultRtsConfig;

    /* Microkit PDs receive neither inherited environment nor process argv. */
    config.rts_opts_enabled = RtsOptsIgnoreAll;
    config.rts_hs_main = true;
    hs_main(argc, argv, &ZCMain_main_closure, config);
}

/* The timer bridge accepts only its configured timer notification. */
void notified(microkit_channel channel)
{
    if (sel4_simplex_ipc_notified != 0) {
        sel4_simplex_ipc_notified(channel);
    }
}
