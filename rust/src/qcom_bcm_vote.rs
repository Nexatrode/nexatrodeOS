//! Pure BCM arithmetic and command-word planning, with no hardware backend.
//!
//! Reference: matched msm_bus_arb_rpmh.c / msm_bus_fabric_rpmh.c. Inputs must
//! already include every relevant client and boot-handoff contribution. This
//! module neither discovers them nor establishes authority to replace them.
//! An encoded word is diagnostic data, not an authorized controller command.

use crate::qcom_bcm::{Error as MetadataError, Metadata};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Metadata(MetadataError),
    Divisor,
    Overflow,
    Duplicate,
    ResponseBits,
    /// One kernel clock-domain group exceeds RPMh's 16-command payload.
    /// The pinned driver's shrink helper only splits between VCDs.
    PayloadLimit,
}

/// Fields of the matched BCM packet layout. This is not a per-client vote
/// inventory or automatic rollback authorization. Commit is a control flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decoded {
    pub valid: bool,
    pub commit: bool,
    pub vector: Vector,
}
impl Decoded {
    pub fn parse(raw: u32) -> Result<Self, Error> {
        if raw & !0x6fff_ffff != 0 {
            return Err(Error::ResponseBits);
        }
        Ok(Self {
            valid: raw & (1 << 29) != 0,
            commit: raw & (1 << 30) != 0,
            vector: Vector {
                x: u64::from((raw >> 14) & 0x3fff),
                y: u64::from(raw & 0x3fff),
            },
        })
    }
    pub fn same_request(self, other: Self) -> bool {
        self.valid == other.valid && self.vector == other.vector
    }
}

/// Bytes per second, before BCM-unit conversion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Bandwidth {
    pub average: u64,
    pub peak: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Contexts {
    pub active_only: Bandwidth,
    pub dual: Bandwidth,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Vector {
    pub x: u64,
    pub y: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Vectors {
    pub active: Vector,
    pub sleep: Vector,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Vote {
    pub metadata: Metadata,
    pub vector: Vector,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Encoded {
    pub address: u32,
    pub data: u32,
    pub clock_domain: u8,
    pub complete: bool,
    /// Linux saturates to 14 bits. Expose that loss instead of concealing it.
    pub saturated: bool,
}

fn acv(metadata: &Metadata) -> bool {
    metadata.id == *b"ACV\0\0\0\0\0"
}

fn validate(metadata: &Metadata) -> Result<(), Error> {
    if metadata.address == 0 || (metadata.address >> 16) & 7 != 5 {
        return Err(Error::Metadata(MetadataError::Database(
            crate::qcom_cmd_db::Error::SlaveId,
        )));
    }
    if metadata.unit_size == 0 {
        return Err(Error::Metadata(MetadataError::Unit));
    }
    if metadata.width == 0 && !acv(metadata) {
        return Err(Error::Metadata(MetadataError::Width));
    }
    if metadata.clock_domain >= 16 {
        return Err(Error::Metadata(MetadataError::ClockDomain));
    }
    Ok(())
}

// msm_bus_div64 preserves positive sub-unit requests, but otherwise floors.
// It is deliberately not ceiling division. Refuse the reference's WARN/zero
// divisor case instead of silently continuing with an invalid configuration.
fn divide(value: u64, divisor: u32) -> Result<u64, Error> {
    if divisor == 0 {
        return Err(Error::Divisor);
    }
    Ok(if value != 0 && value < u64::from(divisor) {
        1
    } else {
        value / u64::from(divisor)
    })
}

/// Sum average requests and take maximum peak at one node in one context.
pub fn sum_clients(clients: &[Bandwidth]) -> Result<Bandwidth, Error> {
    let mut result = Bandwidth::default();
    for client in clients {
        result.average = result
            .average
            .checked_add(client.average)
            .ok_or(Error::Overflow)?;
        result.peak = result.peak.max(client.peak);
    }
    Ok(result)
}

/// Scale one node's already aggregated request; units remain bytes/second.
pub fn scale_node(
    metadata: &Metadata,
    request: Bandwidth,
    bus_width: u32,
    aggregation_ports: u32,
) -> Result<Bandwidth, Error> {
    validate(metadata)?;
    let average_divisor = bus_width
        .checked_mul(aggregation_ports)
        .ok_or(Error::Overflow)?;
    let width = u64::from(metadata.width);
    Ok(Bandwidth {
        average: divide(
            request.average.checked_mul(width).ok_or(Error::Overflow)?,
            average_divisor,
        )?,
        peak: divide(
            request.peak.checked_mul(width).ok_or(Error::Overflow)?,
            bus_width,
        )?,
    })
}

fn aggregate_with_carry(
    metadata: &Metadata,
    nodes: &[Contexts],
    prior_acv_masks: [u32; 2],
    carry: &mut Vectors,
) -> Result<Vectors, Error> {
    validate(metadata)?;
    let mut active = Bandwidth {
        average: carry.active.x,
        peak: carry.active.y,
    };
    let mut dual = Bandwidth {
        average: carry.sleep.x,
        peak: carry.sleep.y,
    };
    for node in nodes {
        active.average = active.average.max(
            node.active_only
                .average
                .checked_add(node.dual.average)
                .ok_or(Error::Overflow)?,
        );
        active.peak = active.peak.max(node.active_only.peak.max(node.dual.peak));
        dual.average = dual.average.max(node.dual.average);
        dual.peak = dual.peak.max(node.dual.peak);
    }
    let convert =
        |bandwidth: Bandwidth, prior_mask: u32, numeric: &mut Vector| -> Result<Vector, Error> {
            let x = divide(bandwidth.average, metadata.unit_size)?;
            let y = divide(bandwidth.peak, metadata.unit_size)?;
            *numeric = Vector { x, y };
            if acv(metadata) {
                // This tests the accumulator after unit conversion. The matched
                // legacy loop carries that accumulator across ordered bindings.
                let mask = if x == 0 && y == 0 {
                    prior_mask & !8
                } else {
                    prior_mask | 8
                };
                Ok(Vector {
                    x: 0,
                    y: u64::from(mask),
                })
            } else {
                Ok(Vector { x, y })
            }
        };
    Ok(Vectors {
        active: convert(active, prior_acv_masks[0], &mut carry.active)?,
        sleep: convert(dual, prior_acv_masks[1], &mut carry.sleep)?,
    })
}

/// Aggregate a resource in isolation, with zero incoming loop accumulators.
/// For a node with multiple ordered BCM bindings, use `legacy_update_ordered`.
/// Prior masks are model inputs, never discovered handoff ownership.
pub fn aggregate(
    metadata: &Metadata,
    nodes: &[Contexts],
    prior_acv_masks: [u32; 2],
) -> Result<Vectors, Error> {
    aggregate_with_carry(metadata, nodes, prior_acv_masks, &mut Vectors::default())
}

#[derive(Clone, Copy, Debug)]
pub struct Aggregation<'a> {
    pub metadata: Metadata,
    /// Already scaled contributions of every member of this BCM.
    pub nodes: &'a [Contexts],
    pub prior_acv_masks: [u32; 2],
}

/// Match the pinned legacy `bcm_update_bus_req` outer loop, in the calling
/// node's DT binding order. Its max accumulators are initialized before that
/// loop and retain the previous BCM's divided numeric values. In particular,
/// EBI's MC0 -> width-zero ACV ordering can set Apps mask8. This is deliberately
/// not a general modern interconnect rule, nor an MMIO/voting interface.
pub fn legacy_update_ordered<const N: usize>(
    bindings: [Aggregation<'_>; N],
) -> Result<[Vectors; N], Error> {
    let mut carry = Vectors::default();
    let mut result = [Vectors::default(); N];
    for (index, binding) in bindings.iter().enumerate() {
        if bindings[..index].iter().any(|b| {
            b.metadata.id == binding.metadata.id || b.metadata.address == binding.metadata.address
        }) {
            return Err(Error::Duplicate);
        }
        result[index] = aggregate_with_carry(
            &binding.metadata,
            binding.nodes,
            binding.prior_acv_masks,
            &mut carry,
        )?;
    }
    Ok(result)
}

/// Produce one diagnostic word per resource, sorted by VCD then address.
/// Commit and completion occur only on the final word of each VCD group.
/// This does not schedule TCSs or implement wake/sleep or handoff preservation.
pub fn group_commands<const N: usize>(mut votes: [Vote; N]) -> Result<[Encoded; N], Error> {
    for (index, vote) in votes.iter().enumerate() {
        validate(&vote.metadata)?;
        if votes[..index].iter().any(|other| {
            other.metadata.id == vote.metadata.id || other.metadata.address == vote.metadata.address
        }) {
            return Err(Error::Duplicate);
        }
    }
    votes.sort_unstable_by_key(|vote| (vote.metadata.clock_domain, vote.metadata.address));
    let mut output = [Encoded::default(); N];
    for (index, vote) in votes.iter().enumerate() {
        let complete = votes
            .get(index + 1)
            .is_none_or(|next| next.metadata.clock_domain != vote.metadata.clock_domain);
        let valid = vote.vector.x != 0 || vote.vector.y != 0;
        output[index] = Encoded {
            address: vote.metadata.address,
            data: ((complete as u32) << 30)
                | ((valid as u32) << 29)
                | ((vote.vector.x.min(0x3fff) as u32) << 14)
                | vote.vector.y.min(0x3fff) as u32,
            clock_domain: vote.metadata.clock_domain,
            complete,
            saturated: vote.vector.x > 0x3fff || vote.vector.y > 0x3fff,
        };
    }
    Ok(output)
}

/// RPMh's maximum commands in one TCS message (`include/soc/qcom/tcs.h`).
pub const TCS_MAX_PAYLOAD: usize = 16;
const BCM_VCD_COUNT: usize = 16;

/// Request state of the bus resource controller. The matched Sunfish driver
/// suppresses the sleep-command commit bit only for `Awake`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RpmhRequestState {
    Sleep,
    WakeOnly,
    ActiveOnly,
    Awake,
}

/// One entry in the caller-supplied BCM list. The input order within each
/// clock domain must match the audited Linux BCM list order. This planner does
/// not discover clients, infer handoff ownership, or authorize TCS writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BcmRequest {
    pub metadata: Metadata,
    pub vectors: Vectors,
}

/// Batch lengths are command counts, in submission order. The array is sized
/// to the caller's maximum BCM count; each batch is at most 16 commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchSchedule<const N: usize> {
    pub lengths: [usize; N],
    pub count: usize,
}

/// A pure planned command stream. Encoded words remain diagnostic output, not
/// an executable controller interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhasePlan<const N: usize> {
    pub commands: [Encoded; N],
    pub len: usize,
    pub batches: BatchSchedule<N>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcsPlan<const N: usize> {
    pub active: PhasePlan<N>,
    pub wake: PhasePlan<N>,
    pub sleep: PhasePlan<N>,
}

fn empty_phase<const N: usize>() -> PhasePlan<N> {
    PhasePlan {
        commands: [Encoded::default(); N],
        len: 0,
        batches: BatchSchedule {
            lengths: [0; N],
            count: 0,
        },
    }
}

fn all_vectors_zero(vectors: Vectors) -> bool {
    vectors.active.x == 0 && vectors.active.y == 0 && vectors.sleep.x == 0 && vectors.sleep.y == 0
}

fn encode_phase_command(metadata: Metadata, vector: Vector, commit: bool) -> Encoded {
    let valid = vector.x != 0 || vector.y != 0;
    Encoded {
        address: metadata.address,
        data: ((commit as u32) << 30)
            | ((valid as u32) << 29)
            | ((vector.x.min(0x3fff) as u32) << 14)
            | vector.y.min(0x3fff) as u32,
        clock_domain: metadata.clock_domain,
        complete: commit,
        saturated: vector.x > 0x3fff || vector.y > 0x3fff,
    }
}

/// Match `tcs_cmd_n_shrink`: coalesce adjacent non-empty VCD groups until the
/// sum would exceed 16. The kernel helper cannot split a single oversized VCD,
/// so this model rejects that input instead of emitting an invalid message.
fn schedule_batches<const N: usize>(groups: &[usize]) -> Result<BatchSchedule<N>, Error> {
    let mut schedule = BatchSchedule {
        lengths: [0; N],
        count: 0,
    };
    let mut pending = 0usize;
    for &group_len in groups {
        if group_len == 0 {
            continue;
        }
        if group_len > TCS_MAX_PAYLOAD {
            return Err(Error::PayloadLimit);
        }
        if pending + group_len > TCS_MAX_PAYLOAD {
            if schedule.count >= N {
                return Err(Error::PayloadLimit);
            }
            schedule.lengths[schedule.count] = pending;
            schedule.count += 1;
            pending = 0;
        }
        pending += group_len;
    }
    if pending != 0 {
        if schedule.count >= N {
            return Err(Error::PayloadLimit);
        }
        schedule.lengths[schedule.count] = pending;
        schedule.count += 1;
    }
    Ok(schedule)
}

/// Build the pinned Sunfish bus-fabric active, wake-only, and sleep TCS plans.
///
/// `requests` must be one RSC's complete, already aggregated snapshot of the
/// BCMs in one Linux commit list; do not combine Apps and Display RSC lists.
/// Sunfish has distinct BCM device nodes with repeated CMD_DB names across
/// those RSCs. Preserve the actual runtime commit-list order within a VCD;
/// sorting by BCM name or static DT order can change the emitted sequence.
/// During kernel init,
/// BCMs whose active and sleep vectors are all zero are omitted. Wake and
/// sleep plans contain only BCMs whose active and sleep vectors differ. TCS
/// batches are split only between VCD groups, matching the source driver.
/// Nothing in this function accesses hardware or makes a vote safe to submit.
pub fn plan_tcs<const N: usize>(
    requests: [BcmRequest; N],
    init_time: bool,
    request_state: RpmhRequestState,
) -> Result<TcsPlan<N>, Error> {
    for (index, request) in requests.iter().enumerate() {
        validate(&request.metadata)?;
        if requests[..index].iter().any(|previous| {
            previous.metadata.id == request.metadata.id
                || previous.metadata.address == request.metadata.address
        }) {
            return Err(Error::Duplicate);
        }
    }

    let active_len = requests
        .iter()
        .filter(|request| !(init_time && all_vectors_zero(request.vectors)))
        .count();

    // The C commit path returns before generating any phase if there are no
    // active commands. In particular, an all-zero init list is a no-op.
    if active_len == 0 {
        return Ok(TcsPlan {
            active: empty_phase(),
            wake: empty_phase(),
            sleep: empty_phase(),
        });
    }

    let mut plan = TcsPlan {
        active: empty_phase(),
        wake: empty_phase(),
        sleep: empty_phase(),
    };
    let mut active_groups = [0usize; BCM_VCD_COUNT];
    let mut wake_groups = [0usize; BCM_VCD_COUNT];
    let mut sleep_groups = [0usize; BCM_VCD_COUNT];
    let mut active_group_count = 0usize;
    let mut wake_group_count = 0usize;
    let mut sleep_group_count = 0usize;

    // The C driver iterates VCDs in numeric order, while preserving linked-list
    // insertion order among BCMs in each VCD.
    for domain in 0..BCM_VCD_COUNT {
        let active_in_group = requests
            .iter()
            .filter(|request| {
                usize::from(request.metadata.clock_domain) == domain
                    && !(init_time && all_vectors_zero(request.vectors))
            })
            .count();
        if active_in_group != 0 {
            let mut emitted = 0usize;
            for request in requests.iter().filter(|request| {
                usize::from(request.metadata.clock_domain) == domain
                    && !(init_time && all_vectors_zero(request.vectors))
            }) {
                emitted += 1;
                let command = encode_phase_command(
                    request.metadata,
                    request.vectors.active,
                    emitted == active_in_group,
                );
                plan.active.commands[plan.active.len] = command;
                plan.active.len += 1;
            }
            active_groups[active_group_count] = active_in_group;
            active_group_count += 1;
        }

        let changed_in_group = requests
            .iter()
            .filter(|request| {
                usize::from(request.metadata.clock_domain) == domain
                    && request.vectors.active != request.vectors.sleep
            })
            .count();
        if changed_in_group != 0 {
            let mut emitted = 0usize;
            for request in requests.iter().filter(|request| {
                usize::from(request.metadata.clock_domain) == domain
                    && request.vectors.active != request.vectors.sleep
            }) {
                emitted += 1;
                let commit = emitted == changed_in_group;
                let wake = encode_phase_command(request.metadata, request.vectors.active, commit);
                let sleep_commit = commit && request_state != RpmhRequestState::Awake;
                let sleep =
                    encode_phase_command(request.metadata, request.vectors.sleep, sleep_commit);
                plan.wake.commands[plan.wake.len] = wake;
                plan.wake.len += 1;
                plan.sleep.commands[plan.sleep.len] = sleep;
                plan.sleep.len += 1;
            }
            wake_groups[wake_group_count] = changed_in_group;
            sleep_groups[sleep_group_count] = changed_in_group;
            wake_group_count += 1;
            sleep_group_count += 1;
        }
    }

    plan.active.batches = schedule_batches(&active_groups[..active_group_count])?;
    plan.wake.batches = schedule_batches(&wake_groups[..wake_group_count])?;
    plan.sleep.batches = schedule_batches(&sleep_groups[..sleep_group_count])?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc0() -> Metadata {
        // User photo C1F06CDE: these five fields were actually observed.
        Metadata {
            id: *b"MC0\0\0\0\0\0",
            address: 0x50000,
            unit_size: 0x1b0210,
            width: 16,
            clock_domain: 0,
            reserved: 0,
            auxiliary_version: 0x1000,
        }
    }

    #[test]
    fn linux_division_is_not_general_ceiling() {
        assert_eq!(divide(0, 10), Ok(0));
        assert_eq!(divide(1, 10), Ok(1));
        assert_eq!(divide(10, 10), Ok(1));
        assert_eq!(divide(19, 10), Ok(1));
        assert_eq!(divide(20, 10), Ok(2));
        assert_eq!(divide(1, 0), Err(Error::Divisor));
    }

    #[test]
    fn observed_mc0_fields_reencode_as_one_committed_unchanged_request() {
        use crate::qcom_rpmh_read::{MC0_OBSERVED_READ, MC0_REASSERT_WORD};
        let decoded = Decoded::parse(MC0_OBSERVED_READ).unwrap();
        assert!(decoded.valid);
        assert!(!decoded.commit);
        assert_eq!(decoded.vector, Vector { x: 9797, y: 16245 });
        assert!(decoded.vector.y >= 15819); // Modem proxy's calculated peak field.
        let [encoded] = group_commands([Vote {
            metadata: mc0(),
            vector: decoded.vector,
        }])
        .unwrap();
        assert_eq!(encoded.data, MC0_REASSERT_WORD);
        assert!(!encoded.saturated);
        let committed = Decoded::parse(encoded.data).unwrap();
        assert!(committed.commit && committed.same_request(decoded));
    }

    #[test]
    fn response_decode_preserves_flags_and_refuses_reserved_bits() {
        for (raw, x, y, valid) in [
            (0, 0, 0, false),
            (0x20000008, 0, 8, true),
            (0x213c44f1, 1265, 1265, true),
            (0x240d5adf, 4149, 6879, true),
        ] {
            let value = Decoded::parse(raw).unwrap();
            assert_eq!(value.vector, Vector { x, y });
            assert_eq!(value.valid, valid);
        }
        for raw in [0x80000000, 0x10000000, 0xffffffff] {
            assert_eq!(Decoded::parse(raw), Err(Error::ResponseBits));
        }
        assert!(
            !Decoded::parse(0x20000008)
                .unwrap()
                .same_request(Decoded::parse(0).unwrap())
        );
    }

    #[test]
    fn observed_mc0_scales_modem_proxy_peak_without_u32_overflow() {
        // 7,000,000 KBps is 7 GB/s, not a u32 byte rate or command word.
        let scaled = scale_node(
            &mc0(),
            Bandwidth {
                average: 0,
                peak: 7_000_000_000,
            },
            4,
            2,
        )
        .unwrap();
        assert_eq!(
            scaled,
            Bandwidth {
                average: 0,
                peak: 28_000_000_000
            }
        );
        let vectors = aggregate(
            &mc0(),
            &[Contexts {
                active_only: Bandwidth::default(),
                dual: scaled,
            }],
            [0; 2],
        )
        .unwrap();
        assert_eq!(vectors.active, Vector { x: 0, y: 15819 });
        assert_eq!(vectors.sleep, vectors.active);
        let [word] = group_commands([Vote {
            metadata: mc0(),
            vector: vectors.active,
        }])
        .unwrap();
        assert_eq!(word.data, 0x60003dcb);
        assert!(!word.saturated);
    }

    #[test]
    fn client_averages_sum_peaks_max_and_aggregate_contexts_then_members() {
        assert_eq!(
            sum_clients(&[
                Bandwidth {
                    average: 10,
                    peak: 40
                },
                Bandwidth {
                    average: 20,
                    peak: 30
                }
            ]),
            Ok(Bandwidth {
                average: 30,
                peak: 40
            })
        );
        let mut meta = mc0();
        meta.unit_size = 1;
        let members = [
            Contexts {
                active_only: Bandwidth {
                    average: 10,
                    peak: 100,
                },
                dual: Bandwidth {
                    average: 20,
                    peak: 80,
                },
            },
            Contexts {
                active_only: Bandwidth {
                    average: 1,
                    peak: 90,
                },
                dual: Bandwidth {
                    average: 40,
                    peak: 120,
                },
            },
        ];
        let result = aggregate(&meta, &members, [0; 2]).unwrap();
        assert_eq!(result.active, Vector { x: 41, y: 120 });
        assert_eq!(result.sleep, Vector { x: 40, y: 120 });
        let scaled = scale_node(
            &meta,
            Bandwidth {
                average: 100,
                peak: 100,
            },
            4,
            2,
        )
        .unwrap();
        assert_eq!(
            scaled,
            Bandwidth {
                average: 200,
                peak: 400
            }
        );
    }

    #[test]
    fn zero_width_acv_is_explicit_and_does_not_force_enable_bit() {
        // User photo D2520B77: native ACV is width0/unit1/VCD11.
        let mut meta = mc0();
        meta.id = *b"ACV\0\0\0\0\0";
        meta.width = 0;
        meta.address = 0x5000c;
        meta.unit_size = 1;
        meta.clock_domain = 11;
        let zero = scale_node(
            &meta,
            Bandwidth {
                average: 1,
                peak: 7_000_000_000,
            },
            4,
            2,
        )
        .unwrap();
        assert_eq!(zero, Bandwidth::default());
        let result = aggregate(
            &meta,
            &[Contexts {
                active_only: zero,
                dual: zero,
            }],
            [0x19, 0x28],
        )
        .unwrap();
        assert_eq!(result.active, Vector { x: 0, y: 0x11 });
        assert_eq!(result.sleep, Vector { x: 0, y: 0x20 });
        meta.width = 16;
        let positive = scale_node(
            &meta,
            Bandwidth {
                average: 0,
                peak: 1,
            },
            4,
            2,
        )
        .unwrap();
        let result = aggregate(
            &meta,
            &[Contexts {
                active_only: positive,
                dual: zero,
            }],
            [1, 0x20],
        )
        .unwrap();
        assert_eq!(result.active.y, 9);
        assert_eq!(result.sleep.y, 0x20);
    }

    #[test]
    fn legacy_mc0_then_zero_width_acv_preserves_numeric_loop_carry() {
        let mc = mc0();
        let mut acv = mc;
        acv.id = *b"ACV\0\0\0\0\0";
        acv.address = 0x5000c;
        acv.width = 0;
        acv.unit_size = 1;
        acv.clock_domain = 11;
        let request = Bandwidth {
            average: 0,
            peak: 7_000_000_000,
        };
        let mc_nodes = [Contexts {
            dual: scale_node(&mc, request, 4, 2).unwrap(),
            ..Contexts::default()
        }];
        let acv_nodes = [Contexts {
            dual: scale_node(&acv, request, 4, 2).unwrap(),
            ..Contexts::default()
        }];
        let mc_binding = Aggregation {
            metadata: mc,
            nodes: &mc_nodes,
            prior_acv_masks: [0; 2],
        };
        let acv_binding = Aggregation {
            metadata: acv,
            nodes: &acv_nodes,
            prior_acv_masks: [0x11, 0x20],
        };
        let values = legacy_update_ordered([mc_binding, acv_binding]).unwrap();
        assert_eq!(values[0].active.y, 15819);
        assert_eq!(values[1].active, Vector { x: 0, y: 0x19 });
        assert_eq!(values[1].sleep.y, 0x28);
        // Reset the accumulator for a different calling node/update. A width0
        // resource processed first cannot inherit a later MC0 calculation.
        let reversed = legacy_update_ordered([acv_binding, mc_binding]).unwrap();
        assert_eq!(reversed[0].active.y, 0x11);
        assert_eq!(reversed[0].sleep.y, 0x20);
        assert_eq!(
            legacy_update_ordered([mc_binding, mc_binding]),
            Err(Error::Duplicate)
        );
    }

    #[test]
    fn legacy_carry_is_divided_numeric_data_not_the_acv_output_mask() {
        let mut first = mc0();
        first.unit_size = 10;
        let nodes = [Contexts {
            dual: Bandwidth {
                average: 190,
                peak: 200,
            },
            ..Contexts::default()
        }];
        let mut second = first;
        second.id = *b"ACV\0\0\0\0\0";
        second.address = 0x5000c;
        second.width = 0;
        second.unit_size = 1;
        let mut third = first;
        third.id = *b"SN0\0\0\0\0\0";
        third.address = 0x50078;
        third.unit_size = 2;
        let values = legacy_update_ordered([
            Aggregation {
                metadata: first,
                nodes: &nodes,
                prior_acv_masks: [0; 2],
            },
            Aggregation {
                metadata: second,
                nodes: &[],
                prior_acv_masks: [0x100, 0x200],
            },
            Aggregation {
                metadata: third,
                nodes: &[],
                prior_acv_masks: [0; 2],
            },
        ])
        .unwrap();
        assert_eq!(values[1].active.y, 0x108);
        assert_eq!(values[2].active, Vector { x: 9, y: 10 });
        assert_eq!(values[2].sleep, values[2].active);
    }

    #[test]
    fn pinned_pas_crypto_floor_exposes_ce0_saturation_and_never_reduces_fields() {
        // Fixed reference PAS AB/IB in bytes/s, with observed metadata and
        // final-DT node geometry. This is a floor experiment, not a complete
        // aggregate of all live clients or a bandwidth sufficiency proof.
        let request = Bandwidth {
            average: 393_600_000,
            peak: 3_936_000_000,
        };
        for (
            id,
            address,
            unit,
            width,
            vcd,
            bus_width,
            ports,
            before,
            raw_vector,
            encoded,
            saturated,
        ) in [
            (
                *b"MC0\0\0\0\0\0",
                0x50000,
                1_770_000,
                16,
                0,
                4,
                2,
                0x29917f75,
                Vector { x: 444, y: 8894 },
                0x69917f75,
                false,
            ),
            (
                *b"SH0\0\0\0\0\0",
                0x5003c,
                2_090_000,
                32,
                1,
                16,
                2,
                0x240d5adf,
                Vector { x: 188, y: 3766 },
                0x640d5adf,
                false,
            ),
            (
                *b"SN11\0\0\0\0",
                0x5006c,
                530_000,
                16,
                3,
                16,
                1,
                0x213c44f1,
                Vector { x: 742, y: 7426 },
                0x613c5d02,
                false,
            ),
            (
                *b"SN0\0\0\0\0\0",
                0x50078,
                530_000,
                16,
                3,
                16,
                1,
                0x213c44f1,
                Vector { x: 742, y: 7426 },
                0x613c5d02,
                false,
            ),
            (
                *b"CE0\0\0\0\0\0",
                0x500a0,
                6802,
                8,
                6,
                8,
                1,
                0,
                Vector {
                    x: 57865,
                    y: 578653,
                },
                0x6fffffff,
                true,
            ),
        ] {
            let metadata = Metadata {
                id,
                address,
                unit_size: unit,
                width,
                clock_domain: vcd,
                reserved: 0,
                auxiliary_version: 0x1000,
            };
            let nodes = [Contexts {
                dual: scale_node(&metadata, request, bus_width, ports).unwrap(),
                ..Contexts::default()
            }];
            let vector = aggregate(&metadata, &nodes, [0; 2]).unwrap().active;
            assert_eq!(vector, raw_vector);
            let inherited = Decoded::parse(before).unwrap().vector;
            let [word] = group_commands([Vote {
                metadata,
                vector: Vector {
                    x: inherited.x.max(vector.x),
                    y: inherited.y.max(vector.y),
                },
            }])
            .unwrap();
            assert_eq!(word.data, encoded);
            assert_eq!(word.saturated, saturated);
            let after = Decoded::parse(word.data).unwrap().vector;
            assert!(after.x >= inherited.x && after.y >= inherited.y);
        }
    }

    #[test]
    fn invalid_scaling_and_overflow_are_errors_not_wrapped_votes() {
        let mut meta = mc0();
        meta.unit_size = 0;
        assert_eq!(
            aggregate(&meta, &[], [0; 2]),
            Err(Error::Metadata(MetadataError::Unit))
        );
        meta = mc0();
        meta.width = 0;
        assert_eq!(
            aggregate(&meta, &[], [0; 2]),
            Err(Error::Metadata(MetadataError::Width))
        );
        meta = mc0();
        meta.clock_domain = 16;
        assert_eq!(
            aggregate(&meta, &[], [0; 2]),
            Err(Error::Metadata(MetadataError::ClockDomain))
        );
        assert_eq!(
            scale_node(&mc0(), Bandwidth::default(), 0, 1),
            Err(Error::Divisor)
        );
        assert_eq!(
            scale_node(&mc0(), Bandwidth::default(), 4, 0),
            Err(Error::Divisor)
        );
        assert_eq!(
            scale_node(&mc0(), Bandwidth::default(), u32::MAX, 2),
            Err(Error::Overflow)
        );
        assert_eq!(
            scale_node(
                &mc0(),
                Bandwidth {
                    average: 0,
                    peak: u64::MAX
                },
                4,
                2
            ),
            Err(Error::Overflow)
        );
        assert_eq!(
            sum_clients(&[
                Bandwidth {
                    average: u64::MAX,
                    peak: 0
                },
                Bandwidth {
                    average: 1,
                    peak: 0
                }
            ]),
            Err(Error::Overflow)
        );
        assert_eq!(
            aggregate(
                &mc0(),
                &[Contexts {
                    active_only: Bandwidth {
                        average: u64::MAX,
                        peak: 0
                    },
                    dual: Bandwidth {
                        average: 1,
                        peak: 0
                    }
                }],
                [0; 2]
            ),
            Err(Error::Overflow)
        );
    }

    #[test]
    fn words_group_by_domain_with_exact_commit_valid_and_saturation_bits() {
        let mut a = mc0();
        a.id = *b"SN11\0\0\0\0";
        a.address = 0x5006c;
        a.clock_domain = 3;
        let mut b = a;
        b.id = *b"SN0\0\0\0\0\0";
        b.address = 0x50078;
        let result = group_commands([
            Vote {
                metadata: b,
                vector: Vector { x: 1, y: 2 },
            },
            Vote {
                metadata: mc0(),
                vector: Vector {
                    x: u64::MAX,
                    y: u64::MAX,
                },
            },
            Vote {
                metadata: a,
                vector: Vector::default(),
            },
        ])
        .unwrap();
        assert_eq!(result.map(|word| word.address), [0x50000, 0x5006c, 0x50078]);
        assert_eq!(result.map(|word| word.complete), [true, false, true]);
        assert_eq!(result.map(|word| word.data), [0x6fffffff, 0, 0x60004002]);
        assert_eq!(result.map(|word| word.saturated), [true, false, false]);
        let [zero] = group_commands([Vote {
            metadata: mc0(),
            vector: Vector::default(),
        }])
        .unwrap();
        assert_eq!(zero.data, 0x40000000); // Commit, invalid/zero vector.
        assert_eq!(group_commands([]), Ok([]));
    }

    #[test]
    fn duplicate_name_or_address_cannot_split_one_resource_across_groups() {
        let original = Vote {
            metadata: mc0(),
            vector: Vector::default(),
        };
        let mut changed = original;
        changed.metadata.address += 4;
        assert_eq!(group_commands([original, changed]), Err(Error::Duplicate));
        changed = original;
        changed.metadata.id = *b"CE0\0\0\0\0\0";
        assert_eq!(group_commands([original, changed]), Err(Error::Duplicate));
    }

    fn bcm_request(id: [u8; 8], address: u32, clock_domain: u8, vectors: Vectors) -> BcmRequest {
        BcmRequest {
            metadata: Metadata {
                id,
                address,
                unit_size: 1,
                width: 1,
                clock_domain,
                reserved: 0,
                auxiliary_version: 0x1000,
            },
            vectors,
        }
    }

    #[test]
    fn tcs_planner_preserves_vcd_list_order_and_three_phase_commit_rules() {
        let active = |x, y| Vector { x, y };
        let plan = plan_tcs(
            [
                // Input order is the Linux linked-list order inside each VCD;
                // address sorting would incorrectly swap these two entries.
                bcm_request(
                    *b"SN11\0\0\0\0",
                    0x5006c,
                    0,
                    Vectors {
                        active: active(3, 4),
                        sleep: active(3, 4),
                    },
                ),
                bcm_request(
                    *b"MC0\0\0\0\0\0",
                    0x50000,
                    0,
                    Vectors {
                        active: active(1, 2),
                        sleep: active(1, 1),
                    },
                ),
                bcm_request(
                    *b"CE0\0\0\0\0\0",
                    0x500a0,
                    2,
                    Vectors {
                        active: Vector::default(),
                        sleep: active(5, 6),
                    },
                ),
                bcm_request(
                    *b"SN0\0\0\0\0\0",
                    0x50078,
                    2,
                    Vectors {
                        active: active(7, 8),
                        sleep: active(8, 9),
                    },
                ),
            ],
            false,
            RpmhRequestState::Awake,
        )
        .unwrap();

        assert_eq!(
            plan.active.commands[..plan.active.len]
                .iter()
                .map(|command| command.address)
                .collect::<std::vec::Vec<_>>(),
            [0x5006c, 0x50000, 0x500a0, 0x50078]
        );
        assert_eq!(
            plan.active.commands[..plan.active.len]
                .iter()
                .map(|command| command.complete)
                .collect::<std::vec::Vec<_>>(),
            [false, true, false, true]
        );
        assert_eq!(plan.active.batches.lengths[0], 4);
        assert_eq!(plan.active.batches.count, 1);

        assert_eq!(plan.wake.len, 3);
        assert_eq!(
            plan.wake.commands[..plan.wake.len]
                .iter()
                .map(|command| (command.address, command.complete))
                .collect::<std::vec::Vec<_>>(),
            [(0x50000, true), (0x500a0, false), (0x50078, true)]
        );
        assert_eq!(plan.wake.batches.lengths[0], 3);
        assert_eq!(plan.sleep.len, 3);
        assert!(
            plan.sleep.commands[..plan.sleep.len]
                .iter()
                .all(|command| !command.complete)
        );

        let sleep_state = plan_tcs(
            [
                bcm_request(
                    *b"MC0\0\0\0\0\0",
                    0x50000,
                    0,
                    Vectors {
                        active: active(1, 2),
                        sleep: active(1, 1),
                    },
                ),
                bcm_request(
                    *b"CE0\0\0\0\0\0",
                    0x500a0,
                    2,
                    Vectors {
                        active: Vector::default(),
                        sleep: active(5, 6),
                    },
                ),
            ],
            false,
            RpmhRequestState::Sleep,
        )
        .unwrap();
        assert!(sleep_state.sleep.commands[0].complete);
        assert!(sleep_state.sleep.commands[1].complete);
    }

    #[test]
    fn tcs_planner_skips_only_initial_all_zero_entries() {
        let requests = [
            bcm_request(*b"MC0\0\0\0\0\0", 0x50000, 0, Vectors::default()),
            bcm_request(*b"CE0\0\0\0\0\0", 0x500a0, 1, Vectors::default()),
        ];
        let init = plan_tcs(requests, true, RpmhRequestState::ActiveOnly).unwrap();
        assert_eq!(init.active.len, 0);
        assert_eq!(init.wake.len, 0);
        assert_eq!(init.sleep.len, 0);

        let runtime = plan_tcs(requests, false, RpmhRequestState::ActiveOnly).unwrap();
        assert_eq!(runtime.active.len, 2);
        assert_eq!(runtime.active.commands[0].data, 0x4000_0000);
        assert_eq!(runtime.active.commands[1].data, 0x4000_0000);
    }

    #[test]
    fn tcs_batches_split_between_domains_at_sixteen_commands() {
        let requests: [BcmRequest; 18] = core::array::from_fn(|index| {
            let mut id = [0u8; 8];
            id[0] = index as u8;
            bcm_request(
                id,
                0x50000 + (index as u32 * 4),
                if index < 10 { 0 } else { 2 },
                Vectors {
                    active: Vector { x: 1, y: 1 },
                    sleep: Vector { x: 1, y: 1 },
                },
            )
        });
        let plan = plan_tcs(requests, false, RpmhRequestState::ActiveOnly).unwrap();
        assert_eq!(plan.active.len, 18);
        assert_eq!(plan.active.batches.count, 2);
        assert_eq!(plan.active.batches.lengths[..2], [10, 8]);

        let oversized: [BcmRequest; 17] = core::array::from_fn(|index| {
            let mut id = [0u8; 8];
            id[0] = index as u8;
            bcm_request(
                id,
                0x50000 + (index as u32 * 4),
                0,
                Vectors {
                    active: Vector { x: 1, y: 1 },
                    sleep: Vector { x: 1, y: 1 },
                },
            )
        });
        assert_eq!(
            plan_tcs(oversized, false, RpmhRequestState::ActiveOnly),
            Err(Error::PayloadLimit)
        );
    }
}
