//! Fixed-size metadata for a logical Files item backed by encrypted segments.
//! Segment ids stay in the trusted Vault/picker path; applications get leases.
pub const PART_BYTES:usize=1024*1024;
pub const MAX_PARTS:usize=64;
pub const MAX_BYTES:usize=PART_BYTES*MAX_PARTS;
pub const PART_OWNER:u64=u64::MAX-1;
pub const HEADER:usize=32+MAX_PARTS*8;
#[derive(Clone)]
pub struct Entry {pub length:usize,pub parts:[u64;MAX_PARTS],pub count:usize,pub name:[u8;120],pub name_len:usize}
impl Entry {
 pub const fn empty()->Self {Self{length:0,parts:[0;MAX_PARTS],count:0,name:[0;120],name_len:0}}
 pub fn begin(length:usize,name:&[u8])->Option<Self>{
  let text=core::str::from_utf8(name).ok()?;
  if length==0||length>MAX_BYTES||name.is_empty()||name.len()>120||text=="."||text==".."||text.chars().any(|c|c.is_control()||c=='/'||c=='\\'){return None;}
  let mut e=Self::empty();e.length=length;e.name[..name.len()].copy_from_slice(name);e.name_len=name.len();Some(e)
 }
 pub fn required(&self)->usize {self.length.div_ceil(PART_BYTES)}
 pub fn part_length(&self,index:usize)->Option<usize>{if index>=self.required(){None}else{Some((self.length-index*PART_BYTES).min(PART_BYTES))}}
 pub fn append(&mut self,id:u64,length:usize)->bool{
  if id==0||self.count>=MAX_PARTS||self.part_length(self.count)!=Some(length)||self.parts[..self.count].contains(&id){return false;}
  self.parts[self.count]=id;self.count+=1;true
 }
 pub fn encode(&self,out:&mut[u8])->Option<usize>{
  if self.count!=self.required()||self.count==0||out.len()<HEADER+self.name_len{return None;}
  out[..HEADER+self.name_len].fill(0);out[..8].copy_from_slice(b"NXFILE3\0");out[8..16].copy_from_slice(&(self.length as u64).to_le_bytes());out[16..20].copy_from_slice(&(self.count as u32).to_le_bytes());out[20..24].copy_from_slice(&(self.name_len as u32).to_le_bytes());
  for (i,id) in self.parts.iter().enumerate(){out[32+i*8..40+i*8].copy_from_slice(&id.to_le_bytes());}out[HEADER..HEADER+self.name_len].copy_from_slice(&self.name[..self.name_len]);Some(HEADER+self.name_len)
 }
 pub fn decode(bytes:&[u8])->Option<Self>{
  if bytes.len()<HEADER||&bytes[..8]!=b"NXFILE3\0"||bytes[24..32].iter().any(|&b|b!=0){return None;}
  let length=usize::try_from(u64::from_le_bytes(bytes[8..16].try_into().ok()?)).ok()?;let count=u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;let n=u32::from_le_bytes(bytes[20..24].try_into().ok()?) as usize;
  if n>120||bytes.len()!=HEADER+n{return None;}let mut e=Self::begin(length,&bytes[HEADER..])?;
  if count!=e.required(){return None;}
  for i in 0..MAX_PARTS {let id=u64::from_le_bytes(bytes[32+i*8..40+i*8].try_into().ok()?);if i<count {if !e.append(id,e.part_length(i)?){return None;}}else if id!=0{return None;}}
  Some(e)
 }
 pub fn clear(&mut self){use zeroize::Zeroize;self.length.zeroize();self.parts.zeroize();self.count.zeroize();self.name.zeroize();self.name_len.zeroize();}
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn segmented_metadata_roundtrip_and_bounds(){
  let mut e=Entry::begin(15*PART_BYTES+123,b"large.nxregion").unwrap();let mut raw=[0u8;HEADER+120];assert!(e.encode(&mut raw).is_none());
  for i in 0..16{assert!(!e.append(0,e.part_length(i).unwrap()));assert!(e.append(100+i as u64,e.part_length(i).unwrap()));}
  let n=e.encode(&mut raw).unwrap();let read=Entry::decode(&raw[..n]).unwrap();assert_eq!(read.length,e.length);assert_eq!(read.parts,e.parts);
  let good=raw;for i in [0,16,20,24,32+63*8]{raw=good;raw[i]^=1;assert!(Entry::decode(&raw[..n]).is_none());}
  assert!(Entry::begin(MAX_BYTES+1,b"a").is_none());assert!(Entry::begin(1,b"../a").is_none());assert!(Entry::begin(1,b"a\0").is_none());e.clear();assert!(e.parts.iter().all(|&x|x==0)&&e.length==0&&e.name.iter().all(|&b|b==0));
 }
 #[test] fn duplicate_segments_and_wrong_sizes_reject(){let mut e=Entry::begin(PART_BYTES+1,b"x").unwrap();assert!(!e.append(1,1));assert!(e.append(1,PART_BYTES));assert!(!e.append(1,1));assert!(!e.append(2,2));assert!(e.append(2,1));assert!(!e.append(3,1));}
}
