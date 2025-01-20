use crate::*;
use net::Agent;
use net::buffer::MsgBuffer;
use net::conn::{ConnWriter, TcpConn};
use net::err::NetError;
use static_init::dynamic;
use std::collections::HashMap;


use std::sync::Arc;
use std::sync::atomic::*;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

const RETRY_NUM: usize = 60;
#[dynamic]
pub static FD_M: Arc<Mutex<HashMap<u16, Arc<Socks5Conn>>>> = Arc::new(Mutex::new(HashMap::new()));
#[derive(Clone)]
pub(crate) struct Socks5Agent {
    pub(crate) conn: Option<Arc<Socks5Conn>>,
    //pub(crate) remote: i32,
    pub(crate) auth: SocksAuth,
    pub(crate) tx: Option<OperationChannelTx>,
}
#[derive(Clone)]
pub(crate) enum SocksAuth {
    Close,
    None,
    Pw,
    Ok,
    Message,
}
pub(crate) enum Socks5Operation {
    SendData(Vec<u8>),
    Exit(Option<String>),
}
pub struct Socks5Conn {
    fd: u16,
    remote_status: AtomicBool,
    pub windows_size: AtomicI64, //接收窗口
    write_tx: ConnWriter,
    pub operation_tx: Sender<Socks5Operation>,
}
unsafe impl Send for Socks5Agent {}
impl Agent for Socks5Agent {
    async fn on_opened(&mut self, conn: &mut TcpConn<Self>) -> Result<(), NetError> {
        for _ in 0..RETRY_NUM {
            let list = SERVER_TX.lock().await;
            let index = fastrand::usize(0..list.len());
            let tx = list[index].clone();
            if tx.status.load(Ordering::Relaxed) {
                let mut fd_m = FD_M.lock().await;
                for id in 1..65535u16 {
                    if !fd_m.contains_key(&id) {
                        let (operation_tx, operation_rx) = tokio::sync::mpsc::channel(1000);
                        let socs5conn = Arc::new(Socks5Conn {
                            fd: id,
                            remote_status: Default::default(),
                            windows_size: AtomicI64::new(WINDOWS_UPDATE_SIZE),
                            write_tx: conn.get_writer_conn(),
                            operation_tx,
                        });

                        fd_m.insert(id, socs5conn.clone());
                        self.tx = Some(tx);
                        self.conn = Some(socs5conn.clone());
                        tokio::spawn(async move {
                            let _ = socs5conn.handle(operation_rx).await;
                        });
                        return Ok(());
                    }
                }
                #[cfg(debug_assertions)]
                println!("获取fd失败");

                return Err(NetError::Custom("获取fd失败".to_string()));
            }
            sleep(Duration::from_secs(1)).await;
        }
        Err(NetError::Custom("获取远程服务器失败".to_string()))
    }

    async fn react(&mut self, conn: &mut TcpConn<Self>) -> Result<bool, NetError> {
        let mut data = conn.buffer_data();
        if data.len() > MAX_SOCKS5MSG_LEN {
            data = &data[..MAX_PLAINTEXT];
        }
        let meg_len=data.len();

        match self.auth {
            SocksAuth::Close => {
                return Err(NetError::Custom("SocksAuth::Close".to_string()));
            }
            SocksAuth::None => {
                if data.len() > 2 {
                    if data[..3] == [5, 1, 0] {
                        conn.write_byte([5, 0].as_ref()).await?;
                        self.auth = SocksAuth::Ok;
                    }else if data[..3] == [5, 1, 2] {
                        conn.write_byte([5, 2].as_ref()).await?;
                        self.auth = SocksAuth::Pw;
                    }
                }
            }
            SocksAuth::Pw => {
                //暂时不验证
                conn.write_byte([1, 0].as_ref()).await?;
                self.auth = SocksAuth::Ok
            }
            SocksAuth::Ok => {
                match data[3] {
                    1 => {
                        //ipv4
                        let mut buf = MsgBuffer::new();
                        for v in &data[4..7] {
                            buf.write_string((v.to_string() + ".").as_str());
                        }
                        buf.write_string((data[8].to_string() + ":").as_ref());
                        buf.write(&data[data.len() - 2..]);
                        self.getfd(buf.as_slice()).await?;
                        self.auth = SocksAuth::Message;
                        conn.write_byte([5, 0, 0, 1, 0, 0, 0, 0, 0, 0].as_ref())
                            .await?;
                    }

                    3 => {
                        self.getfd(&data[5..]).await?;
                        self.auth = SocksAuth::Message;
                        conn.write_byte([5, 0, 0, 1, 0, 0, 0, 0, 0, 0].as_ref())
                            .await?;
                    }
                    4 => {
                        println!("ipv6未支持");
                    }

                    _ => {}
                }
            }
            SocksAuth::Message => {
                let mut new_size = WINDOWS_UPDATE_SIZE
                    - self
                        .conn
                        .as_ref()
                        .unwrap()
                        .windows_size
                        .load(Ordering::Relaxed);
                if new_size > 0 {
                    self.conn
                        .as_ref()
                        .unwrap()
                        .windows_size
                        .fetch_add(new_size, Ordering::Relaxed);
                } else {
                    new_size = 0;
                }
                let mut buf = vec![
                    new_size as u8,
                    (new_size >> 8) as u8,
                    (new_size >> 16) as u8,
                    (new_size >> 24) as u8,
                    (new_size >> 32) as u8,
                    (new_size >> 40) as u8,
                    (new_size >> 48) as u8,
                    (new_size >> 56) as u8,
                ];
                buf.append(&mut data.to_vec());
                let msg = OperationData {
                    cmd: Cmd::Msg,
                    fd: u16_to_fd(self.conn.as_ref().unwrap().fd),
                    body: buf,
                    timestamp: if DEBUG_LEN == 8 {
                        Some(now().unix_millis())
                    } else {
                        None
                    },
                };
                self.send_data(msg).await?;
            }
        }
        conn.shift(meg_len).await?;
        Ok(true)
    }

    async fn on_closed(
        &mut self,
        _conn: &mut TcpConn<Self>,
        reasion: Option<String>,
    ) -> Result<(), NetError> {
        if let Some(socks5conn) = self.conn.as_ref() {
            let mut fd_m = FD_M.lock().await;
            fd_m.remove(&socks5conn.fd);
            socks5conn
                .operation_tx
                .try_send(Socks5Operation::Exit(reasion)).map_err(|e|NetError::Channel(e.to_string()))?;
        }

        Ok(())
    }
}
impl Socks5Agent {
    async fn getfd(&mut self, buf: &[u8]) -> Result<(), NetError> {
        let socks5conn = self.conn.as_ref().unwrap();
        socks5conn.remote_status.store(true, Ordering::Relaxed);
        let data = OperationData {
            cmd: Cmd::GetFd,
            fd: u16_to_fd(socks5conn.fd),
            body: buf.to_vec(),
            timestamp: if DEBUG_LEN == 8 {
                Some(now().unix_millis())
            } else {
                None
            },
        };

        self.send_data(data).await?;
        Ok(())
    }
    async fn send_data(&mut self, data: OperationData) -> Result<(), NetError> {
        Ok(self
            .tx
            .as_ref()
            .unwrap()
            .tx
            .send(ReactOperation::SendData(data))
            .await.map_err(|e|NetError::Channel(e.to_string()))?)
    }
}
impl Socks5Conn {
    async fn handle(&self, mut rx: Receiver<Socks5Operation>) -> Result<(), NetError> {
        while let Some(recv) = rx.recv().await {
            match recv {
                Socks5Operation::SendData(data) => {
                    self.write_tx.write(data).await?;
                }
                Socks5Operation::Exit(reason) => {
                    rx.close();
                    self.write_tx.close(reason)?;
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}
