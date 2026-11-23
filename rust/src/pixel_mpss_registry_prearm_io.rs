//! Queue-to-first-conversion observation using dedicated outer I/O timing counters.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display: &Display, owner: &ModemOwner) {
        let pair=owner.registry_window_trace().target;
        let trace=pair.map(|p|p.prearm);
        let io=owner.registry_prearm_io();
        let choice=trace.and_then(|p|p.first_choice);
        let elf=owner.service_registry_trace().elf;
        let age=|at|self.time_from_us(Some(self.runtime_started),at);
        let duration=|ticks|self.time_from_us(Some(0),ticks);
        let parts=io.and_then(|t|t.delta(true));
        display.replace_line(1,"REGISTRY PREARM IO");
        for (row,(label,value)) in [
            ("RUNTIME ERROR",self.error),
            ("QUEUE OBS US",age(io.and_then(|t|t.queued_at))),
            ("ADAPT FIRST US",age(io.and_then(|t|t.first_enter_at))),
            ("CORE FIRST US",age(trace.and_then(|t|t.first_pass_at))),
            ("FIRST END US",age(io.and_then(|t|t.first_end_at))),
            ("OWN DONE US",age(trace.and_then(|t|t.own_done_at))),
            ("ARM OBS US",age(io.and_then(|t|t.arm_at))),
            ("PASSES GATE",trace.map_or(u32::MAX,|t|(t.passes.min(65535)<<16)|u32::from(t.first_gate.map_or(u16::MAX,u16::from)))),
            ("QUEUE FLAGS",trace.map_or(u32::MAX,|t|t.queued_flags)),
            ("FIRST FLAGS",trace.and_then(|t|t.first_flags).unwrap_or(u32::MAX)),
            ("FIRST TXN CMD",choice.map_or(u32::MAX,|c|(u32::from(c.transaction.unwrap_or(u16::MAX))<<16)|u32::from(c.command))),
            ("FIRST PARAM",choice.map_or(u32::MAX,|c|c.parameter)),
            ("FIRST RESULT",choice.map_or(u32::MAX,|c|u32::from(c.result))),
            ("IO TO FIRST US",duration(io.and_then(|t|t.total(false)))),
            ("IO TO ARM US",duration(io.and_then(|t|t.total(true)))),
            ("STATUS US",duration(parts.map(|p|p[0]))),
            ("CURSOR US",duration(parts.map(|p|p[1]))),
            ("READ US",duration(parts.map(|p|p[2]))),
            ("OTHER IO US",duration(parts.map(|p|p[3]))),
            ("ELF RX US",age(elf.and_then(|e|e.received))),
            ("ELF QUEUE US",age(elf.and_then(|e|e.queued))),
            ("ELF PUB US",age(elf.and_then(|e|e.published))),
            ("ELF DONE US",age(elf.and_then(|e|e.done))),
            ("TX4 PUB US",age(pair.and_then(|p|p.newer_published_at))),
            ("IPA COMPLETE",u32::from(self.ipa_complete_seen)),
            ("WLAN RRQ RX",owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS",self.wlan_query.snapshot(owner)[7]),
        ].into_iter().enumerate(){display.hex(row+2,label,value);}
    }
}
