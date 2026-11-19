
#ifndef GUEST_LOCALE_H
#define GUEST_LOCALE_H
#ifdef __cplusplus
extern "C" {
#endif

struct lconv {
    char *decimal_point;
    char *thousands_sep;
    char *grouping;
    char *int_curr_symbol;
    char *currency_symbol;
    char *mon_decimal_point;
    char *mon_thousands_sep;
    char *mon_grouping;
    char *positive_sign;
    char *negative_sign;
    char int_frac_digits;
    char frac_digits;
    char p_cs_precedes;
    char p_sep_by_space;
    char n_cs_precedes;
    char n_sep_by_space;
    char p_sign_posn;
    char n_sign_posn;
    char int_p_cs_precedes;
    char int_p_sep_by_space;
    char int_n_cs_precedes;
    char int_n_sep_by_space;
    char int_p_sign_posn;
    char int_n_sign_posn;
};
struct lconv *localeconv(void);
char *setlocale(int, const char *);
#define LC_ALL 0
#define LC_CTYPE 1
#define LC_NUMERIC 2
#define LC_TIME 3
#define LC_COLLATE 4
#define LC_MONETARY 5
#define LC_MESSAGES 6
#define LC_ALL_MASK 0x1f
#define LC_CTYPE_MASK 1
#define LC_NUMERIC_MASK 2
#define LC_TIME_MASK 4
#define LC_COLLATE_MASK 8
#define LC_MONETARY_MASK 16
#define LC_MESSAGES_MASK 32
typedef struct __guest_locale *locale_t;
locale_t newlocale(int, const char *, locale_t);
void freelocale(locale_t);
locale_t uselocale(locale_t);
locale_t duplocale(locale_t);

#ifdef __cplusplus
}
#endif
#endif
