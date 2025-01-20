use openssl::ssl::SslAcceptor;
use crate::http1::Request;
use crate::response::Response;
use crate::router::Router;
use net::conn::TcpConn;
use net::err::NetError;
use net::{Agent, Server};
use std::sync::Arc;
use std::time::Duration;
use tokio_native_tls::TlsAcceptor;

#[allow(dead_code)]
pub async fn start<T: Clone + Send + 'static>(addr: &str, agent: Http1Agent<T>) {
    let mut server: Server = Server::new().with_writetimeout(Duration::from_secs(10));
    server.start_server(addr, agent).await.unwrap();
}
///T建议为Arc<Mutex<Session>>
#[derive(Clone)]
pub struct Http1Agent<T> {
    pub is_https:bool,
    pub router: Router<T>,
    pub session: T,
    pub openssl_acceptor:Arc<SslAcceptor>,
    pub tls_acceptor:Arc<TlsAcceptor>
}

unsafe impl<T> Send for Http1Agent<T> {}
impl<T: Clone + Send + 'static> Agent for Http1Agent<T> {
    async fn on_opened(&mut self, conn: &mut TcpConn<Self>) -> Result<(), NetError> {
        if self.is_https {
            conn.get_inner().upgrade_ssl(self.tls_acceptor.clone()).await?;
        }

        Ok(())
    }

    async fn react(&mut self, conn: &mut TcpConn<Self>) -> Result<bool, NetError> {
        if let Some(req) = Request::parse_req_server(conn,0).await? {
            let keep_alive = req.keep_alive;
            let unread_content_length = req.unread_content_length;
            let resp = self.router.run(req, self.session.clone()).await?;
            Request::read_finish(conn, unread_content_length).await?;

            //req.write_string("hello world")?;
            /*if let Err(e) = ret {
                println!("{},{}", req.path(), e);
                let _ = req.out_404(Some(e.to_string()))?;
            };*/

            if let Err(e) = resp.write(conn, keep_alive).await {
                //出错后，尝试写502错误
                Response::error(e.to_string(), 502)?
                    .write(conn, keep_alive)
                    .await?;
            };
            return Ok(true);
        }
        return Ok(false);

        //println!("{},{:?}", req.path(), now.elapsed());
    }
    async fn on_closed(
        &mut self,
        _conn: &mut TcpConn<Self>,
        _reasion: Option<String>,
    ) -> Result<(), NetError> {
        Ok(())
    }
}
