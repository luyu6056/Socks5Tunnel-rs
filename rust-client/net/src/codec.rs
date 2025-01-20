//取消codec
//Codec后面接具体的ctx类型
/*#[async_trait]
pub trait CodeC<Ctx:ConnTraitT>:Copy {
    async fn encode(&mut self, indata: Vec<u8>,ctx: &mut Ctx) -> Result<Vec<u8>, NetError>;

    async fn decode(&mut self, ctx: &mut Ctx) -> Result<Option<Vec<u8>>, NetError>;
}*/

/*#[derive(Copy, Clone)]
pub struct DefaultCodec {}
//测试用codec，ctx类型为String
#[async_trait]
impl CodeC<String> for DefaultCodec {
    async fn encode(&mut self, indata: Vec<u8>,conn: &mut ConnRead< String>) -> Result<Option<Vec<u8>>, NetError> {
        Ok(Some(indata))
    }
    async fn decode(&mut self, conn: & mut String) -> Result<Option<Vec<u8>>, NetError> {

        /*match conn.bufferlen() {
            0 => Ok(None),
            n => Ok(Some(conn.next(n).await?)),
        }*/
        Ok(None)
    }
}*/
