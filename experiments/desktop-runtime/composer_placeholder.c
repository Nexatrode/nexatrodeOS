/* The Microkit image composer requires a PD description. The dedicated root
 * task ignores that generated specification and creates its own processes. */
#include <microkit.h>
void init(void) {}
void notified(microkit_channel channel) { (void)channel; }
