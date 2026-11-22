//! Bounded observation only; all endpoints come from existing checked operations.
use super::*;

#[derive(Clone, Copy, Default)]
struct ValidationSample {
    edges: [u64;4],
    domains: Option<[[u64;4];3]>,
    #[cfg(feature="sunfish-wrq-domain-costs")]
    parts:Option<[[[u64;7];2];3]>,
    #[cfg(feature="sunfish-wrq-post-return-callers")]
    site:Option<(SampleSite,Option<u32>)>,
}

pub struct WrqValidationWork {
    pub records: IpaIngressWork,
    samples: [ValidationSample;192],
    next: usize,
    evicted: Option<u64>,
    pub finished: Option<u64>,
}
impl Default for WrqValidationWork {
    fn default()->Self {Self {records:Default::default(),samples:[ValidationSample::default();192],
        next:0,evicted:None,finished:None}}
}
#[derive(Default)]
pub struct WrqValidationCost {
    pub total:u64,
    pub operations:[u64;3],
    pub domains:[u64;3], // pre, controller body, post; nested in operations
    pub count:u32,
    pub domain_count:u32,
    pub missing:u32,
    pub rx:u64,
    pub tx:u64,
    pub lost:u32,
    #[cfg(feature="sunfish-wrq-domain-costs")] pub parts:[u64;6],
    #[cfg(feature="sunfish-wrq-domain-costs")] pub parts_count:u32,
    #[cfg(feature="sunfish-wrq-domain-costs")] pub parts_missing:u32,
}
fn overlap(a:u64,b:u64,from:u64,to:u64)->u64 {b.min(to).saturating_sub(a.max(from))}
impl WrqValidationWork {
    pub(super) fn sample(&mut self,edges:[u64;4],domains:Option<[[u64;4];3]>) {
        if self.finished.is_some() || !edges.windows(2).all(|w|w[0]<=w[1]) {return;}
        let domains=domains.filter(|groups| groups.iter().enumerate().all(|(i,g)|
            g.windows(2).all(|w|w[0]<=w[1]) && edges[i]<=g[0] && g[3]<=edges[i+1]));
        let old=self.samples[self.next];
        if old.edges[3]>old.edges[0] {self.evicted=Some(old.edges[3]);}
        self.samples[self.next]=ValidationSample {edges,domains,
            #[cfg(feature="sunfish-wrq-domain-costs")] parts:None,
            #[cfg(feature="sunfish-wrq-post-return-callers")] site:None};
        self.next=(self.next+1)%self.samples.len();
    }
    #[cfg(feature="sunfish-wrq-domain-costs")]
    pub(super) fn sample_parts(&mut self,edges:[u64;4],domains:Option<[[u64;4];3]>,parts:Option<[[[u64;7];2];3]>) {
        if self.finished.is_some() || !edges.windows(2).all(|w|w[0]<=w[1]) {return;}
        self.sample(edges,domains);
        let s=&mut self.samples[(self.next+self.samples.len()-1)%self.samples.len()];
        if let Some(d)=s.domains {
            s.parts=parts.filter(|p|p.iter().enumerate().all(|(i,groups)|groups.iter().enumerate().all(|(side,g)|
                g.windows(2).all(|w|w[0]<=w[1]) && d[i][side*2]<=g[0] && g[6]<=d[i][side*2+1])));
        }
    }
    pub(super) fn record(&mut self,r:IpaIngressRecord) {
        if self.finished.is_none() {self.records.push(r);}
    }
    pub(super) fn finish(&mut self,at:u64) {self.finished.get_or_insert(at);}
    pub fn cost(&self,from:u64,to:u64)->WrqValidationCost {
        let to=to.min(self.finished.unwrap_or(to));
        if to<=from {return WrqValidationCost::default();}
        let mut c=WrqValidationCost {total:to-from,
            lost:u32::from(self.records.lost(from)) | (u32::from(self.evicted.is_some_and(|t|t>from))<<1),
            ..Default::default()};
        for s in &self.samples {
            if overlap(s.edges[0],s.edges[3],from,to)==0 {continue;}
            c.count+=1;
            for i in 0..3 {c.operations[i]+=overlap(s.edges[i],s.edges[i+1],from,to);}
            if let Some(groups)=s.domains {
                c.domain_count+=1;
                for g in groups {for i in 0..3 {c.domains[i]+=overlap(g[i],g[i+1],from,to);}}
            } else {c.missing+=1;}
            #[cfg(feature="sunfish-wrq-domain-costs")]
            if let Some(parts)=s.parts {
                c.parts_count+=1;
                for groups in parts {for group in groups {for i in 0..6 {c.parts[i]+=overlap(group[i],group[i+1],from,to);}}}
            }else{c.parts_missing+=1;}
        }
        for r in self.records.records(from,to) {
            let ticks=overlap(r.start,r.end,from,to);
            match r.kind {0=>c.rx+=ticks,1|2|3|6=>c.tx+=ticks,_=>{}}
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clips_nested_native_spans_and_keeps_post_return_window() {
        let mut w=WrqValidationWork::default();
        w.sample([10,30,60,100],Some([[11,16,24,29],[31,38,52,59],[61,70,90,99]]));
        let a=w.cost(20,65);
        assert_eq!(a.operations,[10,30,5]);
        assert_eq!(a.domains,[11,18,12]);
        assert_eq!((a.count,a.domain_count,a.missing),(1,1,0));
        w.sample([100,110,120,130],None);
        w.finish(125);w.sample([130,140,150,160],None);w.finish(160);
        let b=w.cost(100,150);
        assert_eq!(b.operations,[10,10,5]);
        assert_eq!((b.total,b.count,b.missing),(25,1,1));
        assert_eq!(w.finished,Some(125));
    }
    #[test]
    fn reports_eviction_and_rejects_reversed_or_incomplete_subspans() {
        let mut w=WrqValidationWork::default();
        for i in 0..200 {w.sample([i*10,i*10+2,i*10+4,i*10+6],None);}
        assert_eq!(w.cost(0,2000).lost,2);
        assert_eq!(w.cost(1990,2000).lost,0);
        let mut w=WrqValidationWork::default();
        w.sample([1,3,5,7],Some([[1,2,2,3],[3,4,4,5],[5,6,8,9]]));
        assert_eq!(w.cost(0,10).missing,1);
        w.sample([10,9,12,13],None);
        assert_eq!(w.cost(0,20).count,1);
        assert_eq!(w.cost(20,10).total,0);
    }
}

#[cfg(all(test,feature="sunfish-wrq-domain-costs"))]
mod domain_parts_tests {
    use super::*;
    #[test]
    fn category_transitions_skip_empty_groups_but_reject_partial_and_reversed_evidence() {
        let mut d=DomainEdges::default();
        assert!(d.needs_stamp(0));d.mark(0,10);assert!(!d.needs_stamp(0));
        d.mark(1,20);d.mark(3,40);d.mark(4,50);d.mark(5,60);
        assert_eq!(d.complete(),None);d.mark(6,80);
        assert_eq!(d.complete(),Some([10,20,40,40,50,60,80]));
        let mut reversed=DomainEdges::default();reversed.mark(0,10);reversed.mark(1,5);
        reversed.mark(6,30);assert_eq!(reversed.complete(),None);
        let mut incomplete=DomainEdges::default();incomplete.mark(1,10);incomplete.mark(6,30);
        assert_eq!(incomplete.complete(),None);
    }
    #[test]
    fn domain_parts_clip_inside_native_pre_and_post_and_missing_is_explicit() {
        let mut w=WrqValidationWork::default();
        let d=[[10,30,50,70],[80,100,120,140],[150,170,190,210]];
        let parts=::core::array::from_fn(|i|::core::array::from_fn(|side|{
            let start=d[i][side*2];[start,start+2,start+4,start+6,start+8,start+10,start+20]
        }));
        w.sample_parts([0,75,145,215],Some(d),Some(parts));
        let c=w.cost(15,55);assert_eq!(c.parts,[2,2,2,2,2,10]);
        assert_eq!((c.parts_count,c.parts_missing),(1,0));
        let mut bad=parts;bad[1][1][6]=141;
        let mut invalid=WrqValidationWork::default();
        invalid.sample_parts([0,75,145,215],Some(d),Some(bad));
        let rejected=invalid.cost(0,215);
        assert_eq!((rejected.domain_count,rejected.parts_count,rejected.parts_missing),(1,0,1));
        w.sample_parts([215,220,225,230],None,Some(bad));
        assert_eq!(w.cost(215,230).parts_missing,1);
    }
}

/// Timestamp only transitions between existing successful validation stages.
#[cfg(feature="sunfish-wrq-domain-costs")]
#[derive(Clone,Copy,Default)]
pub struct DomainEdges {
    edges:[Option<u64>;7],last:Option<usize>,seen:u8,bad:bool,
}
#[cfg(feature="sunfish-wrq-domain-costs")]
impl DomainEdges {
    pub(crate) fn needs_stamp(&self,category:usize)->bool {self.last!=Some(category)}
    pub(crate) fn mark(&mut self,category:usize,at:u64) {
        if category>6 || self.bad {self.bad=true;return;}
        if self.last.is_none() && category!=0 {self.bad=true;return;}
        if let Some(last)=self.last {
            if category<last || self.edges[last].is_some_and(|t|at<t) {self.bad=true;return;}
            if category==last {return;}
        }
        for i in self.last.map_or(0,|n|n+1)..=category {self.edges[i]=Some(at);}
        self.last=Some(category);self.seen|=1<<category;
    }
    pub(crate) fn complete(&self)->Option<[u64;7]> {
        // Entry, ASID, setup, table scan and completion always occur. A zero-
        // route table or empty final-register set can have a zero-width group.
        if self.bad || self.last!=Some(6) || self.seen&0x73!=0x73 {return None;}
        let mut out=[0;7];for (i,v) in self.edges.iter().enumerate(){out[i]=(*v)?;}Some(out)
    }
}

#[cfg(feature="sunfish-wrq-domain-costs")]
const _:()=assert!(::core::mem::size_of::<WrqValidationWork>()<=128*1024);

#[cfg(feature="sunfish-wrq-post-return-callers")]
#[path="qcom_glink_wrq_callsite.rs"]
mod callsite;
#[cfg(feature="sunfish-wrq-post-return-callers")]
pub use callsite::*;

#[cfg(all(test,feature="sunfish-wrq-post-return-callers"))]
mod callsite_tests {
    use super::*;
    #[track_caller]
    fn forwarded_site()->SampleSite {SampleSite::caller()}
    #[test]
    fn caller_capture_preserves_forwarded_location_and_basename() {
        let line=line!()+1;
        let site=forwarded_site();
        assert_eq!(site.line,line);
        assert_eq!(site.file,SampleSite::file_id(file!()));
        assert_eq!(SampleSite::file_id("/a/qcom_glink_fifo.rs"),SampleSite::file_id("/b/qcom_glink_fifo.rs"));
    }
    #[test]
    fn callsites_clip_three_windows_and_retain_validated_rx_metadata() {
        let mut w=WrqValidationWork::default();
        let a=SampleSite{file:1,line:10};let b=SampleSite{file:1,line:20};
        w.sample_parts_at([10,20,30,40],None,None,a,Some(0));
        w.sample_parts_at([40,50,60,70],None,None,b,Some(24));
        let mut c=w.callsite_costs([Some((15,45)),Some((45,65)),Some((65,70))]);
        assert_eq!(c.len,2);assert_eq!(c.lost,0);
        let x=c.entries.iter().find(|x|x.site==a).unwrap();
        assert_eq!(x.count,[1,0,0]);assert_eq!(x.ticks,[25,0,0]);assert_eq!(x.empty_rx,[1,0,0]);
        let x=c.entries.iter().find(|x|x.site==b).unwrap();
        assert_eq!(x.count,[1,1,1]);assert_eq!(x.ticks,[5,20,5]);assert_eq!(x.max_rx,24);
        c.rank();assert_eq!(c.entries[0].site,b);
    }
    #[test]
    fn capacity_unknown_sites_unknown_rx_and_freeze_stay_explicit() {
        let mut w=WrqValidationWork::default();
        for i in 0..33 {let t=u64::from(i)*10;
            w.sample_parts_at([t,t+3,t+6,t+10],None,None,SampleSite{file:1,line:i+1},None);
        }
        let c=w.callsite_costs([Some((0,330)),None,Some((40,40))]);
        assert_eq!(c.len,32);assert_eq!(c.overflow_count,[1,0,0]);assert_eq!(c.overflow_ticks,[10,0,0]);
        assert_eq!(c.entries[0].unknown_rx,[1,0,0]);
        w.finish(330);w.sample_parts_at([330,333,336,340],None,None,SampleSite{file:2,line:1},Some(0));
        assert_eq!(w.cost(0,400).count,33);
        let mut unknown=WrqValidationWork::default();unknown.sample([1,2,3,4],None);
        let c=unknown.callsite_costs([Some((0,5)),None,None]);assert_eq!(c.unknown_site,[1,0,0]);
    }
    #[test]
    fn eviction_and_unusable_windows_are_reported() {
        let mut w=WrqValidationWork::default();
        for i in 0..200 {w.sample_parts_at([i*10,i*10+2,i*10+4,i*10+6],None,None,SampleSite{file:1,line:1},Some(0));}
        let c=w.callsite_costs([Some((0,2000)),Some((1990,2000)),Some((20,10))]);
        assert_ne!(c.lost,0);assert_eq!(c.entries[0].count[1],1);assert_eq!(c.entries[0].count[2],0);
    }
}
