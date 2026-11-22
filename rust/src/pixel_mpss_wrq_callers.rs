//! Settled read-only caller attribution for the original OACK window.
use super::*;
impl RuntimeDomain {
    pub(super) fn show_wrq_callers(&self,display:&Display,owner:&ModemOwner) {
        let reg=owner.ipa_registration_trace();
        let work=owner.wrq_receive_work();let path=&work.validation;
        let returned=work.finished;let queued=reg.and_then(|r|r.queued_at);
        let published=reg.and_then(|r|r.published_at);let head=owner.oack_grant_outcome().oack_at;
        let read=match owner.first_oack_tail(){qcom_glink_fifo::ServerCheckTail::Consumed(t)=>t,_=>None};
        let close=owner.init_close_fifo().bounds.and_then(|c|c.first_visible);
        let age=|t|self.time_from_us(Some(self.runtime_started),t);
        let us=|t|self.time_from_us(Some(0),Some(t));
        display.replace_line(1,"WRQ POST RETURN CALLERS");
        for(i,(label,value)) in [("RUNTIME ERROR",self.error),("WRQ RXDONE US",age(returned)),
            ("REG QUEUE US",age(queued)),("REG PUB US",age(published)),("OACK HEAD US",age(head)),
            ("OACK READ US",age(read)),("CLOSE VIS US",age(close))].into_iter().enumerate(){display.hex(i+2,label,value);}
        let windows=[returned.zip(queued),queued.zip(published),published.zip(head)]
            .map(|window|window.filter(|(a,b)|a<=b && path.finished.is_some()));
        display.replace_line(9,"SPAN US CHECKS N RX TX");
        let mut missing=0u32;
        for (i,window) in windows.iter().enumerate() {
            if let Some((a,b))=*window {
                let c=path.cost(a,b);missing=missing.saturating_add(c.missing);
                display.header_row(i+10,false,&[(i as u32+1,2),(us(c.total),8),(us(c.operations.iter().sum()),8),(c.count,4),(us(c.rx),8),(us(c.tx),8)]);
                display.header_row(i+10,true,&[(age(Some(a)),8),(age(Some(b)),8),(c.domain_count,4),(c.missing,4)]);
            }else{missing=missing.saturating_add(1);display.header_row(i+10,false,&[(i as u32+1,2),(u32::MAX,8)]);display.header_row(i+10,true,&[]);}
        }
        let mut sites=path.callsite_costs(windows);sites.rank();
        display.replace_line(13,"FILE LINE A B C CHECK US");
        let mut omitted=sites.overflow_count;let mut omitted_ticks=sites.overflow_ticks;
        for entry in &sites.entries[sites.len.min(9)..sites.len] {
            for i in 0..3 {omitted[i]=omitted[i].saturating_add(entry.count[i]);omitted_ticks[i]=omitted_ticks[i].saturating_add(entry.ticks[i]);}
        }
        let mut unknown=0u32;
        for entry in &sites.entries[..sites.len] {unknown=unknown.saturating_add(entry.unknown_rx.iter().sum::<u32>());}
        for i in 0..9 {
            if let Some(entry)=sites.entries[..sites.len].get(i) {
                display.header_row(i+14,false,&[(entry.site.file,8),(entry.site.line,4),(entry.count[0],4),(entry.count[1],4),(entry.count[2],4),(us(entry.ticks.iter().sum()),8)]);
                display.header_row(i+14,true,&[(us(entry.ticks[0]),8),(us(entry.ticks[1]),8),(us(entry.ticks[2]),8),(entry.empty_rx.iter().sum(),4),(entry.max_rx,8)]);
            }else{display.header_row(i+14,false,&[]);display.header_row(i+14,true,&[]);}
        }
        display.header_label(23,"OMIT N A B C US A B C");
        display.header_row(23,true,&[(sites.len.saturating_sub(9) as u32,4),(omitted[0],4),(omitted[1],4),(omitted[2],4),(us(omitted_ticks[0]),8),(us(omitted_ticks[1]),8),(us(omitted_ticks[2]),8)]);
        display.header_label(24,"LOST MISS SITE RX OVER");
        display.header_row(24,true,&[(sites.lost,4),(missing,4),(sites.unknown_site.iter().sum(),4),(unknown,4),(sites.overflow_count.iter().sum(),4)]);
        let first=owner.server_check_oacks()[0];
        display.hex(25,"FIRST DATA US",age(first.and_then(|f|f.first_data_at)));
        display.hex(26,"IPA COMPLETE",u32::from(self.ipa_complete_seen));
        display.hex(27,"WLAN RRQ RX",owner.tftp_wire_counts()[2]);
        display.hex(28,"WLFW SERVERS",self.wlan_query.snapshot(owner)[7]);
    }
}
