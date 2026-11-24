//! Existing original-transfer and ELF observations on the runtime clock.
use super::*;
fn completion_before(end:Option<u64>, boundary:Option<u64>)->u32 {
    end.zip(boundary).map_or(u32::MAX, |(a,b)|u32::from(a<b))
}
#[cfg(not(feature = "sunfish-ack-elf-phase-trace"))]
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self, display:&Display, owner:&ModemOwner) {
        let first=owner.server_check_oacks()[0];
        let write=owner.original_tftp_write_trace();
        let data=write.and_then(|w|w.data);
        let ack=write.map(|w|w.ack);
        let elf=owner.service_registry_trace().elf;
        // This FIFO visibility trace targets the exact modem0:0x1f client.
        // Do not attach its close to a differently addressed original transfer.
        let close=write.filter(|w|w.peer.node==0 && w.peer.port==0x1f)
            .and_then(|_|owner.init_close_fifo().bounds.and_then(|c|c.first_visible));
        let read=write.and_then(|_|match owner.first_oack_tail(){
            qcom_glink_fifo::ServerCheckTail::Consumed(at)=>at,_=>None});
        let query=self.wlan_query.snapshot(owner);
        let (wlan,wlfw)=owner.wlan_discovery_times();
        let age=|at|self.time_from_us(Some(self.runtime_started),at);
        display.replace_line(1,"FIRST DATA ACK ELF");
        for (row,(label,value)) in [
            ("RUNTIME ERROR",self.error),
            ("FIRST PEER",first.map_or(u32::MAX,|f|f.peer.port)),
            ("TRANSFER PORT",first.map_or(u32::MAX,|f|f.transfer_port)),
            ("TRACE MATCH",u32::from(write.is_some())),
            ("OACK PUB US",age(first.and_then(|f|f.oack_at))),
            ("OACK READ US",age(read)),
            ("DATA RX US",age(first.and_then(|f|f.first_data_at))),
            ("DATA RESULT",data.and_then(|d|d.result).unwrap_or(u32::MAX)),
            ("DATA BLOCK",data.map_or(u32::MAX,|d|u32::from(d.block))),
            ("DATA BYTES",data.map_or(u32::MAX,|d|d.body_bytes)),
            ("FINAL BLOCK",write.and_then(|w|w.final_block).map_or(u32::MAX,u32::from)),
            ("ACK PHASE",ack.map_or(u32::MAX,|a|u32::from(a.phase))),
            ("ACK PUB US",age(ack.and_then(|a|a.published_at))),
            ("ACK DONE US",age(ack.and_then(|a|a.done_at))),
            ("CLOSE VIS US",age(close)),
            ("CLOSE RX US",age(first.and_then(|f|f.first_close_at))),
            ("ELF RX US",age(elf.and_then(|e|e.received))),
            ("ELF QUEUE US",age(elf.and_then(|e|e.queued))),
            ("ELF PUB US",age(elf.and_then(|e|e.published))),
            ("ELF DONE US",age(elf.and_then(|e|e.done))),
            ("WRITES COMPLETE",self.server_check.completed_count() as u32),
            ("ACK BEF CLOSE",completion_before(ack.and_then(|a|a.done_at),close)),
            ("PD QUERY STATE",query[1]),
            ("WLAN RRQ RX",owner.tftp_wire_counts()[2]),
            ("WLFW SERVERS",query[7]),
            ("FIRST WLAN US",age(wlan)),
            ("FIRST WLFW US",age(wlfw)),
        ].into_iter().enumerate(){display.hex(row+2,label,value);}
    }
}
