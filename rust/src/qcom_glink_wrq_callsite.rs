//! Attribute existing successful validation samples; no I/O or additional clock.
use super::*;

#[derive(Clone,Copy,Default,PartialEq,Eq,Debug)]
pub struct SampleSite {pub file:u32,pub line:u32}
impl SampleSite {
    pub fn file_id(path:&str)->u32 {
        let name=path.rsplit('/').next().unwrap_or(path);
        name.bytes().fold(2166136261u32,|hash,b|(hash^u32::from(b)).wrapping_mul(16777619))
    }
    #[track_caller]
    pub(crate) fn caller()->Self {
        let at=::core::panic::Location::caller();
        Self{file:Self::file_id(at.file()),line:at.line()}
    }
}

#[derive(Clone,Copy,Default)]
pub struct CallsiteCost {
    pub site:SampleSite,
    pub count:[u32;3],
    pub ticks:[u64;3],
    pub empty_rx:[u32;3],
    pub unknown_rx:[u32;3],
    pub max_rx:u32,
}
pub struct CallsiteCosts {
    pub entries:[CallsiteCost;32],pub len:usize,
    pub overflow_count:[u32;3],pub overflow_ticks:[u64;3],
    pub unknown_site:[u32;3],pub unknown_ticks:[u64;3],pub lost:u32,
}
impl CallsiteCosts {
    pub fn rank(&mut self) {
        self.entries[..self.len].sort_unstable_by(|a,b|
            b.ticks.iter().sum::<u64>().cmp(&a.ticks.iter().sum::<u64>())
            .then(a.site.file.cmp(&b.site.file)).then(a.site.line.cmp(&b.site.line)));
    }
}
impl WrqValidationWork {
    pub(crate) fn sample_parts_at(&mut self,edges:[u64;4],domains:Option<[[u64;4];3]>,
        parts:Option<[[[u64;7];2];3]>,site:SampleSite,rx:Option<u32>) {
        if self.finished.is_some() || !edges.windows(2).all(|w|w[0]<=w[1]) {return;}
        self.sample_parts(edges,domains,parts);
        let sample=&mut self.samples[(self.next+self.samples.len()-1)%self.samples.len()];
        sample.site=Some((site,rx));
    }
    pub fn callsite_costs(&self,windows:[Option<(u64,u64)>;3])->CallsiteCosts {
        let mut result=CallsiteCosts {entries:[CallsiteCost::default();32],len:0,
            overflow_count:[0;3],overflow_ticks:[0;3],unknown_site:[0;3],unknown_ticks:[0;3],lost:0};
        for (which,window) in windows.iter().enumerate() {
            let Some((from,to))=*window else {continue;};
            let to=to.min(self.finished.unwrap_or(to));
            if to<=from {continue;}
            result.lost|=self.cost(from,to).lost;
            for sample in &self.samples {
                let ticks=overlap(sample.edges[0],sample.edges[3],from,to);
                if ticks==0 {continue;}
                let Some((site,rx))=sample.site else {
                    result.unknown_site[which]+=1;result.unknown_ticks[which]+=ticks;continue;
                };
                let index=if let Some(index)=result.entries[..result.len].iter().position(|c|c.site==site) {index}
                else if result.len<result.entries.len() {
                    let index=result.len;result.entries[index].site=site;result.len+=1;index
                } else {
                    result.overflow_count[which]+=1;result.overflow_ticks[which]+=ticks;continue;
                };
                let entry=&mut result.entries[index];entry.count[which]+=1;entry.ticks[which]+=ticks;
                match rx {Some(0)=>entry.empty_rx[which]+=1,Some(n)=>entry.max_rx=entry.max_rx.max(n),None=>entry.unknown_rx[which]+=1}
            }
        }
        result
    }
}
