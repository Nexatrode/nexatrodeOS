/* SPDX-License-Identifier: Apache-2.0 */
#pragma once
#ifdef MAP_PRIVATE
#define PRIVATE_MAX 16
struct saved_place {char name[96];double x,y;uint32_t region,kind;};
struct saved_places {uint64_t magic;uint32_t count,reserved;struct saved_place items[PRIVATE_MAX];uint8_t pad[112];};
_Static_assert(sizeof(struct saved_place)==120&&sizeof(struct saved_places)==2048,"private storage ABI");
static struct saved_places places;
static struct saved_place private_pending;
static uint64_t private_generation,private_sequence,private_last_activity,private_retry_at;
static int private_pressed=-1,private_press_x,private_scroll;
static void motion(void);
static unsigned private_mode,private_cursor,private_offset,private_viewed,private_target,private_bank,private_phase,private_failures;
static bool private_ready,private_busy,private_enrolling,private_pending_save;
static char private_password[129],private_confirm[129];
static unsigned private_length;
static const char *private_status="";
uintptr_t map_private_cipher;
#ifdef MAP_SESSION_PRIVATE
#define PRIVATE_LOCK_CAPTION "HIDE"
uintptr_t map_session_key;
static uint64_t private_session_epoch;
extern unsigned mp_session_open(const void*,const void*,void*,uint64_t*,unsigned*);
#else
#define PRIVATE_LOCK_CAPTION "LOCK"
#endif
extern unsigned mp_status(const void*),mp_opened(void),mp_cleared(void),mp_enroll(const void*,size_t,const void*,const void*,void*),mp_unlock(const void*,size_t,const void*,void*,uint64_t*,unsigned*),mp_seal(const void*,const void*,uint64_t,void*);
extern void mp_lock(void);
static void private_lock(void),private_begin(void),private_reply(void),private_draw(void),private_card(void),private_character(unsigned),private_submit(void),private_clear_input(void),private_save_selection(void),private_tick(void),private_restore(void);
static void private_add_pending(void);
static bool private_button(unsigned),private_key(unsigned,unsigned);
static bool private_keyboard(void){return (private_mode>=2&&private_mode<=4)||private_mode==6;}
static void private_wipe(void *v,size_t n){volatile unsigned char *p=v;while(n--)*p++=0;}
#else
static bool private_keyboard(void){return false;}
#endif
