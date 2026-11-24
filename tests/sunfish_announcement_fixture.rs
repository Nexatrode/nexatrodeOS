//! Pure Session prefix shared by Core and actual FIFO-boundary tests.
use crate::qcom_glink_qmi::{Config,Event,Published,Reply,RequestHandle,Session};
use crate::qcom_modem_services::Service;
use crate::qcom_servreg;
fn word(b:&[u8],at:usize)->u32{u32::from_le_bytes(b[at..at+4].try_into().unwrap())}
fn control(command:u16,parameter:u32)->Vec<u8>{let mut b=Vec::new();b.extend(command.to_le_bytes());b.extend(2u16.to_le_bytes());b.extend(parameter.to_le_bytes());b}
fn intents(entries:&[(u32,u32)])->Vec<u8>{let mut b=control(5,entries.len()as u32);for(id,size)in entries{b.extend(size.to_le_bytes());b.extend(id.to_le_bytes());}b}
fn data(command:u16,id:u32,packet:&[u8],remaining:usize)->Vec<u8>{let mut b=control(command,id);b.extend((packet.len()as u32).to_le_bytes());b.extend((remaining as u32).to_le_bytes());b.extend(packet);b.resize((b.len()+7)&!7,0);b}
fn qrtr(kind:u32,source:u32,destination:u32,confirm:bool,payload:&[u8])->Vec<u8>{let mut b=Vec::new();for v in [1,kind,0,source,u32::from(confirm),payload.len()as u32,1,destination]{b.extend(v.to_le_bytes());}b.extend(payload);b.resize((b.len()+3)&!3,0);b}
fn qrtr_control(kind:u32,fields:[u32;4])->Vec<u8>{let mut body=kind.to_le_bytes().to_vec();for v in fields{body.extend(v.to_le_bytes());}qrtr(kind,0xfffffffe,0xfffffffe,false,&body)}
fn session()->Session{let mut s=Session::new(Config{local_node:1,peer_node:0,local_channel:1,remote_channel:2},1).unwrap();s.register(1,Service::RemoteStorage,0x4000).unwrap();s.register(1,Service::HeapMemory,0x4001).unwrap();s}
fn publish(s:&mut Session)->Option<(Published,Vec<u8>)>{let mut b=Vec::new();s.publish_next(1,|r|{b.extend(r);Ok(())}).unwrap().map(|e|(e,b))}
fn offer(s:&mut Session)->u32{let id=s.offer_receive(1).unwrap();let(e,b)=publish(s).unwrap();assert_eq!(e,Published::ReceiveIntent(id));assert_eq!(word(&b,12),id);id}
pub(super) fn ready() -> (Session,u32,RequestHandle) {
    let mut s=session();let port=qcom_servreg::LOCAL_PORT;
    s.register(1,Service::ServiceRegistry,port).unwrap();s.prepare_service_available(1,Service::ServiceRegistry).unwrap();s.start_control(1).unwrap();
    s.ingest(1,&intents(&[(0x66,4096),(0x67,4096)])).unwrap();
    let id=offer(&mut s);s.ingest(1,&data(9,id,&qrtr_control(2,[0;4]),0)).unwrap();
    for _ in 0..32 {let Some((e,b))=publish(&mut s)else{break};match e{Published::ReceiveDone(x)if x==id=>assert_eq!(b[0],6),Published::Data{remote_intent,..}=>{s.ingest(1,&control(14,remote_intent)).unwrap();},_=>{}}}
    let request_id=offer(&mut s);let body=[0,3,0,0x21,0,6,0,1,3,0,b'f',b'o',b'o'];
    let Event::Request(request)=s.ingest(1,&data(9,request_id,&qrtr(1,3,port,false,&body),0)).unwrap()else{panic!("registry request")};
    s.queue_reply(&request,Reply::Failure{error:3}).unwrap();
    let mut published=false;for _ in 0..32 {let Some((e,_))=publish(&mut s)else{break};if matches!(e,Published::Data{last_fragment:true,..}){published=true;break;}}
    assert!(published,"transaction3 must be fully published and still awaiting its original receipt");
    let id=offer(&mut s);(s,id,request)
}
