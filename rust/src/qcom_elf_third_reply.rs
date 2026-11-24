//! A narrowly selected ELF reply may use a third independently owned TX slot.
use super::*;
impl Session {
    /// Owner selects its first accepted ELF handle. Core independently checks
    /// the live ticket/name; normal queue_reply remains a two-reply operation.
    pub(crate) fn queue_elf_third_reply(&mut self,h:&RequestHandle,reply:Reply<'_>)->Result<(),Error>{
        let(context,request)=self.request(h)?;
        if context.service!=Service::ServiceRegistry
            || !matches!(request,Request::ServiceRegistry(q)if q.service_name==b"kernel/elf_loader"){
            return Err(Error::InvalidReply);
        }
        self.queue_reply_with_ipa_token(h,reply,None,true)
    }
}
