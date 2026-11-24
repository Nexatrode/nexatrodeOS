//! Bounded accepted/committed IDs between exact predecessor publication/receipt.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_tftp_completion(&self,display:&Display,owner:&ModemOwner){
        let prior=owner.registry_window_trace().predecessor;let fifo=owner.registry_reply_fifo();
        let receipt=fifo.and_then(|f|f.completion);let wire=owner.registry_wire_headers();
        let age=|at|self.time_from_us(Some(self.runtime_started),at);
        display.replace_line(1,"REGISTRY TX3 WIRE LOG");
        for(row,(label,value))in[("RUNTIME ERROR",self.error),("OMIT UNPARSED",wire.map_or(u32::MAX,|w|diagnostic::packed_counts(w.omitted,w.unparsed))),
            ("TX3 PUB US",age(prior.and_then(|p|p.published_at))),("TX3 TAIL US",age(fifo.and_then(|f|f.peer_tail_at))),
            ("TX3 RX VIS US",age(receipt.and_then(|r|r.first_visible))),("TX3 DONE US",age(prior.and_then(|p|p.done_at)))].into_iter().enumerate(){display.hex(row+2,label,value);}
        if let Some(wire)=wire{
            for(index,header)in wire.headers.into_iter().chain([wire.completion]).enumerate(){if let Some(h)=header{
                let[kind,source,destination]=h.qrtr.unwrap_or([u32::MAX;3]);
                let(tag,a,b)=if let Some([node,port])=h.del_client{(0xD,node,port)}else if let Some((flag,txn,message))=h.qmi{(u32::from(flag),u32::from(txn),u32::from(message))}else{(0xF,u32::MAX,u32::MAX)};
                display.wire_row(index/4+8,index%4,&[(age(h.at),8),(u32::from(h.tx),1),(u32::from(h.command),4),(u32::from(h.channel),4),(h.parameter,8),(u32::from(h.bytes),4),
                    (kind,2),(source,8),(destination,8),(tag,1),(a,8),(b,8)]);
            }}
        }
        display.hex(28,"WLAN RRQ WLFW",diagnostic::packed_counts(owner.tftp_wire_counts()[2],self.wlan_query.snapshot(owner)[7]));
    }
}
