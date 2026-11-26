//! Focus changes invalidate only the old and new active decorations.
pub fn changed(previous:Option<(u32,usize)>,focus:u32,height:usize,count:usize)->u8 {
    let Some((old,old_height))=previous else {return (1u8<<count)-1;};
    if old_height!=height {return (1u8<<count)-1;}
    (0..count).fold(0,|mask,i|if (old==i as u32+1)!=(focus==i as u32+1){mask|1<<i}else{mask})
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn focus_only_invalidates_changed_titlebars() {
        assert_eq!(changed(None,1,520,3),7);
        assert_eq!(changed(Some((1,520)),1,520,3),0);
        assert_eq!(changed(Some((1,520)),3,520,3),5);
        assert_eq!(changed(Some((3,520)),2,520,3),6);
        assert_eq!(changed(Some((2,520)),0,520,3),2);
        assert_eq!(changed(Some((2,520)),2,380,3),7);
    }
}
