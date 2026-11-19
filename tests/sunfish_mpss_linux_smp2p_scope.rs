//! Tests inside the protocol module can forge sealed requests to challenge
//! the real volatile store boundary. No such constructor exists in production.
use super::*;
use crate::smp2p::prepared_native as prepared;
#[test]
fn native_whitelist_rejects_forged_init_offsets_values_and_cross_item_stores() {
    for mode in 0..7 {
        let (mut e, pages, _lock) = prepared();
        let before = pages.clone();
        let mut r = Write {
            at: 0,
            value: 0x504d5324,
            field: Field::Init,
        };
        match mode {
            0 => r.at = 36,
            1 => r.at = 340,
            2 => r.at = 2,
            3 => r.value ^= 1,
            4 => r.field = Field::Features,
            5 => r.at = memory::ITEMS[1].1.offset - AP.offset,
            _ => r.at = usize::MAX,
        }
        let p = e.smp2p_parts().unwrap().1;
        assert!(p.smp_store(&r).is_err());
        assert_eq!(
            p.smp_store(&Write {
                at: 0,
                value: 0x504d5324,
                field: Field::Init
            }),
            Err(Error::Consumed)
        );
        for (a, b) in pages.iter().zip(&before) {
            assert_eq!(a.0, b.0);
        }
    }
}
#[test]
fn native_whitelist_enforces_negotiation_ssr_and_ipa_enabled_before_valid() {
    for mode in 0..10 {
        let (mut e, pages, _lock) = prepared();
        let p = e.smp2p_parts().unwrap().1;
        for (at, value) in INIT {
            p.smp_store(&Write {
                at,
                value,
                field: Field::Init,
            })
            .unwrap();
        }
        if mode >= 2 {
            p.smp_store(&Write {
                at: 4,
                value: 0x102,
                field: Field::Features,
            })
            .unwrap();
        }
        let r = match mode {
            0 => Write {
                at: 56,
                value: 2,
                field: Field::IpaEnabled,
            },
            1 => Write {
                at: 4,
                value: 0x202,
                field: Field::Features,
            },
            2 => Write {
                at: 4,
                value: 0x101,
                field: Field::Version,
            },
            3 => Write {
                at: 16,
                value: 1,
                field: Field::RestartAck,
            },
            4 => Write {
                at: 56,
                value: 3,
                field: Field::IpaValid,
            },
            5 => Write {
                at: 36,
                value: 2,
                field: Field::IpaEnabled,
            },
            6 => Write {
                at: 56,
                value: 1,
                field: Field::IpaEnabled,
            },
            7 => {
                p.smp_store(&Write {
                    at: 56,
                    value: 2,
                    field: Field::IpaEnabled,
                })
                .unwrap();
                Write {
                    at: 56,
                    value: 1,
                    field: Field::IpaValid,
                }
            }
            8 => {
                p.smp_store(&Write {
                    at: 56,
                    value: 0,
                    field: Field::IpaEnabled,
                })
                .unwrap();
                Write {
                    at: 56,
                    value: 3,
                    field: Field::IpaValid,
                }
            }
            _ => {
                p.smp_store(&Write {
                    at: 56,
                    value: 2,
                    field: Field::IpaEnabled,
                })
                .unwrap();
                p.smp_store(&Write {
                    at: 56,
                    value: 3,
                    field: Field::IpaValid,
                })
                .unwrap();
                Write {
                    at: 56,
                    value: 2,
                    field: Field::IpaEnabled,
                }
            }
        };
        let before = pages.clone();
        assert!(p.smp_store(&r).is_err(), "mode {mode}");
        for (a, b) in pages.iter().zip(&before) {
            assert_eq!(a.0, b.0);
        }
    }
}
