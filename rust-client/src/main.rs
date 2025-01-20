extern crate core;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::cmd::Cmd;
use net::Server;
use net::buffer::MsgBuffer;
use net::err::NetError;
use static_init::dynamic;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll::{Pending, Ready};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time;
use tokio::time::{sleep};
use tokio_native_tls::TlsStream;
use tokio_native_tls::native_tls::{Identity, TlsConnector};
mod utils;
use crate::socks5::{FD_M, Socks5Agent, Socks5Operation, SocksAuth};
pub use utils::*;

mod cmd;
mod socks5;
const SERVER_NUM: usize = 4;
const LISTEN_ADDR: &str = "0.0.0.0:10800";
const ADDR: &str = "127.0.0.1:3304";
const FD_LEN: usize = 2;
const DEBUG_LEN: usize = 0;
const LENGTH_LEN: usize = 2;
//2length+1cmd+2fd+debug
const HEAD_LEN: usize = LENGTH_LEN + 1 + FD_LEN + DEBUG_LEN;
const UPDATE_SIZE_LEN: usize = 8;
//max_len: HEAD_LEN+data.len()
const MAX_PLAINTEXT: usize = 16384;
const MAX_SOCKS5MSG_LEN: usize = MAX_PLAINTEXT - HEAD_LEN - UPDATE_SIZE_LEN;
const WINDOWS_UPDATE_SIZE: i64 = MAX_PLAINTEXT as i64 * 20;
#[derive(Debug)]
struct Data<'a> {
    cmd: Cmd,
    fd: [u8; FD_LEN],
    body: &'a [u8],
    timestamp: Option<i64>,
}
pub(crate) struct OperationData {
    cmd: Cmd,
    fd: [u8; FD_LEN],
    body: Vec<u8>,
    timestamp: Option<i64>,
}
#[derive(Clone)]
pub(crate) struct OperationChannelTx {
    tx: Sender<ReactOperation>,
    status: Arc<AtomicBool>,
}
pub(crate) enum ReactOperation {
    ReConnect,
    SendData(OperationData),
}
#[dynamic]
pub(crate) static SERVER_TX: Arc<Mutex<Vec<OperationChannelTx>>> = Arc::new(Mutex::new(Vec::new()));

struct ServerConnection {
    addr: String,
    operation_channel_tx: OperationChannelTx,
    ping_time: i64,
    pong_time: i64,
    write_buf: [u8; MAX_PLAINTEXT],
    is_first_delete: bool,
}
#[tokio::main]
async fn main() {
    fastrand::seed(now().unix_millis() as u64);
    let  ifs = netdev::get_default_interface().unwrap();
    let mac_addr = hex::encode(&ifs.mac_addr.expect("Mac address not found").octets());
    let cert = include_bytes!(".././config/cert");
    let key = include_bytes!(".././config/pkcs8");
    //let key = openssl::pkey::PKey::private_key_from_pem(key).unwrap();
    //let pkcs8 = key.private_key_to_pem_pkcs8().unwrap();

    let identity = Identity::from_pkcs8(cert, key).unwrap();
    let connector: Arc<tokio_native_tls::TlsConnector> = Arc::new(
        TlsConnector::builder()
            .identity(identity)
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap()
            .into(),
    );
    for _ in 0..SERVER_NUM {
        let connector = connector.clone();
        let mac_addr = mac_addr.clone();
        tokio::spawn(async move {
            ServerConnection::start(ADDR, connector, mac_addr)
                .await;
        });
    }
    Server::new()
        .start_server(LISTEN_ADDR, Socks5Agent {
            conn: Default::default(),
            //windows_size: 0,
            //remote: 0,
            auth: SocksAuth::None,
            tx: None,
        })
        .await
        .expect("主server启动失败");
}
impl ServerConnection {
    pub async fn start(
        addr: &str,
        connector: Arc<tokio_native_tls::TlsConnector>,
        mac_addr: String,
    )  {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1000);
        let mut conn = ServerConnection {
            addr: addr.to_string(),
            operation_channel_tx: OperationChannelTx {
                tx,
                status: Arc::new(Default::default()),
            },
            ping_time: 0,
            pong_time: 0,
            write_buf: [0; MAX_PLAINTEXT],
            is_first_delete: true,
        };

        SERVER_TX
            .lock()
            .await
            .push(conn.operation_channel_tx.clone());
        let mut server_stream = conn
            .reconnect(addr, mac_addr.as_bytes(), connector.clone())
            .await;

        #[doc(hidden)]
        mod __tokio_select_util {
            pub(super) enum Out<_0, _1,_2> {
                Read(_0),
                Op(_1),
                PingInterval(_2)
            }
        }
        let mut readbuf = MsgBuffer::new();
        let mut ping_interval = time::interval(Duration::from_secs(200));
        loop {
            unsafe {
                let output = {
                    ::tokio::macros::support::poll_fn(|cx| {
                        let f2 = &mut rx.recv();
                        if let Ready(out) = Future::poll(Pin::new_unchecked(f2), cx) {
                            return Ready(__tokio_select_util::Out::Op(out));
                        }
                        let f1 = &mut do_read_data(&mut readbuf, &mut server_stream);
                        if let Ready(out) = Future::poll(Pin::new_unchecked(f1), cx) {
                            return Ready(__tokio_select_util::Out::Read(out));
                        }

                        if let Ready(out) = ping_interval.poll_tick(cx) {
                            return Ready(__tokio_select_util::Out::PingInterval(out));
                        }
                        Pending
                    })
                    .await
                };

                match output {
                    __tokio_select_util::Out::Op(op) => {
                        let op = op.expect("channel 错误");
                        match op {
                            ReactOperation::ReConnect => {
                                server_stream = conn
                                    .reconnect(addr, mac_addr.as_bytes(), connector.clone())
                                    .await;
                                readbuf.reset();
                            }
                            ReactOperation::SendData(data) => {
                                if let Err(e)=conn.write(Data::from(&data), &mut server_stream).await{
                                    #[cfg(debug_assertions)]
                                    println!("write 错误 {}",e);

                                    conn.operation_channel_tx.tx.send(ReactOperation::ReConnect).await.unwrap();
                                };
                            }
                        }
                    }
                    __tokio_select_util::Out::Read(res) => {
                        if let Err(e)=res{
                            #[cfg(debug_assertions)]
                            println!("Read 错误 {}",e);

                            conn.operation_channel_tx.tx.send(ReactOperation::ReConnect).await.unwrap();
                            continue;
                        };
                        //消息in
                        while   readbuf.len() > 2 {
                            let msglen = (readbuf[0] as usize) | (readbuf[1] as usize) << 8;

                            if readbuf.len() < msglen {
                                break;
                            }
                            let data :Data =readbuf[..msglen].into() ;
                            #[cfg(debug_assertions)]
                            //println!("收到消息 {:?}",data.cmd);

                            if let Err(e) = conn.handle(data).await {
                                #[cfg(debug_assertions)]
                                {
                                    let data:Data =readbuf[..msglen].into() ;
                                    println!("handle {:?} 错误 {}", data, e);
                                }
                            };
                            #[cfg(debug_assertions)]
                            //let data:Data =readbuf[..msglen].into() ;
                            //println!("消息结束 {:?}",data.cmd);


                            readbuf.shift(msglen);
                        }


                    }
                    __tokio_select_util::Out::PingInterval(_)=>{
                        let t = now().unix();
                        let data=OperationData{
                            cmd: Cmd::Ping,
                            fd: [0,0],
                            body: vec![t as u8,(t>>8) as u8, (t>>16) as u8,(t>>24) as u8,(t>>32) as u8,(t>>40) as u8,(t>>48) as u8,(t>>56) as u8],
                            timestamp:if DEBUG_LEN==8{
                                Some(now().unix_millis())
                            }else { None },
                        };
                        conn.operation_channel_tx.tx.send(ReactOperation::SendData(data)).await.unwrap();
                    }
                }
            }
        }
    }
    async fn handle(&mut self, msg: Data<'_>) -> Result<(), NetError> {
        //println!("cmd {:?} len {}", msg.cmd, msg.body.len());

        match msg.cmd {
            Cmd::DeleteFd => {
                if let Some(socks5conn) = FD_M.lock().await.get(&fd_to_u16(msg.fd)) {
                    socks5conn
                        .operation_tx
                        .send(Socks5Operation::Exit(Some(
                            "服务器要求远程关闭".to_string(),
                        )))
                        .await.map_err(|e|NetError::Channel(e.to_string()))?;
                };
            }
            Cmd::Msg => {
                if let Some(socks5conn) = FD_M.lock().await.get(&fd_to_u16(msg.fd)) {
                    socks5conn
                        .operation_tx
                        .send(Socks5Operation::SendData(msg.body.to_vec()))
                        .await.map_err(|e|NetError::Channel(e.to_string()))?;
                    let windows_size = socks5conn
                        .windows_size
                        .fetch_add(-1 * (msg.body.len() as i64), Ordering::Relaxed);

                    if windows_size < WINDOWS_UPDATE_SIZE / 2 {
                        //扩大窗口
                        let size = WINDOWS_UPDATE_SIZE - windows_size;
                        socks5conn.windows_size.fetch_add(size, Ordering::Relaxed);
                        let data = OperationData {
                            cmd: Cmd::WindowsUpdate,
                            fd: msg.fd,
                            body: vec![
                                size as u8,
                                (size >> 8) as u8,
                                (size >> 16) as u8,
                                (size >> 24) as u8,
                                (size >> 32) as u8,
                                (size >> 40) as u8,
                                (size >> 48) as u8,
                                (size >> 56) as u8,
                            ],
                            timestamp: if DEBUG_LEN==8 {
                                Some(now().unix_millis())
                            }else { None },
                        };
                        self.operation_channel_tx
                            .tx
                            .send(ReactOperation::SendData(data))
                            .await.map_err(|e|NetError::Channel(e.to_string()))?;
                    }
                };
            }
            Cmd::Pong => {
                let ping_time = (msg.body[0] as i64)
                    | (msg.body[1] as i64) << 8
                    | (msg.body[2] as i64) << 16
                    | (msg.body[3] as i64) << 24
                    | (msg.body[4] as i64) << 32
                    | (msg.body[5] as i64) << 40
                    | (msg.body[6] as i64) << 48
                    | (msg.body[7] as i64) << 56;

                if ping_time != self.ping_time {
                    return Ok(());
                }
                self.pong_time = now().unix();
            }
            Cmd::DeleteIp => {
                if !self.is_first_delete {
                    self.is_first_delete = true;
                    return Ok(());
                }
                let mut fd_m = FD_M.lock().await;
                for (_, conn) in fd_m.iter() {
                    conn.operation_tx
                        .send(Socks5Operation::Exit(Some("服务器重连删除".to_string())))
                        .await.map_err(|e|NetError::Channel(e.to_string()))?;
                }
                fd_m.clear();
            }
            other => {
                println!("错误cmd {:?}", other)
            }
        }
        Ok(())
    }
    pub fn reset(&mut self) {
        self.pong_time = 0;
        self.ping_time = 0;
    }
    pub async fn reconnect(
        &mut self,
        addr: &str,
        mac: &[u8],
        connector: Arc<tokio_native_tls::TlsConnector>,
    ) -> TlsStream<TcpStream> {
        loop {
            match self.do_reconnect(addr, mac, connector.clone()).await {
                Ok(stream) => {
                    println!("连接成功  {}", addr);
                    return stream;
                }
                Err(e) => {
                    #[cfg(debug_assertions)]
                    println!("连接失败 err {}", e.to_string());

                    sleep(Duration::from_secs(3)).await;
                }
            }
        }
    }
    pub async fn do_reconnect(
        &mut self,
        addr: &str,
        mac: &[u8],
        connector: Arc<tokio_native_tls::TlsConnector>,
    ) -> Result<TlsStream<TcpStream>, NetError> {
        let mut head_buf = [0u8; 4];
        let mut conn = TcpStream::connect(addr).await?;

        let n = conn.read_exact(&mut head_buf).await?;

        if n < 4 || head_buf[3] != 0 || head_buf[0] == 0 {
            return Err(NetError::ProtocolErr("消息太短或协议错误1".to_string()));
        }
        let msglen =
            (head_buf[0] as usize) | (head_buf[1] as usize) << 8 | (head_buf[2] as usize) << 16;
        let mut buf = vec![0; msglen];
        let n = conn.read_exact(&mut buf).await?;
        if n != msglen && buf[0] != 10 {
            return Err(NetError::ProtocolErr("消息太短或协议错误2".to_string()));
        }
        let handshake_buf = [
            32, 0, 0, 1, 8, 138, 8, 0, 255, 255, 255, 0, 33, 53, 45, 49, 48, 46, 53, 46, 49, 45,
            77, 97, 114, 105, 97, 68, 66, 0, 64, 1, 0, 0, 67, 66,
        ];
        conn.write_all(&handshake_buf).await?;
        self.reset();
        let mut stream = connector.connect("", conn).await?;
        self.write(
            Data {
                cmd: Cmd::Reg,
                fd: [0, 0],
                body: mac,
                timestamp: None,
            },
            &mut stream,
        )
        .await?;
        self.operation_channel_tx
            .status
            .store(true, Ordering::Relaxed);
        Ok(stream)
    }
    pub async fn write(
        &mut self,
        data: Data<'_>,
        stream: &mut TlsStream<TcpStream>,
    ) -> Result<(), NetError> {
        if data.body.len() + HEAD_LEN > MAX_PLAINTEXT {
            return Err(NetError::ProtocolErr(format!(
                "data长度 {} 大于 {}",
                data.body.len(),
                MAX_PLAINTEXT - HEAD_LEN
            )));
        }
        let msglen = HEAD_LEN + data.body.len();
        self.write_buf[0] = msglen as u8;
        self.write_buf[1] = (msglen >> 8) as u8;
        self.write_buf[2] = data.cmd as u8;
        self.write_buf[3] = data.fd[0];
        self.write_buf[4] = data.fd[1];
        if DEBUG_LEN == 8 {
            let timestamp = if data.timestamp.is_none() {
                now().unix_millis()
            } else {
                data.timestamp.unwrap()
            };
            self.write_buf[5] = timestamp as u8;
            self.write_buf[6] = (timestamp >> 8) as u8;
            self.write_buf[7] = (timestamp >> 16) as u8;
            self.write_buf[8] = (timestamp >> 24) as u8;
            self.write_buf[9] = (timestamp >> 32) as u8;
            self.write_buf[10] = (timestamp >> 40) as u8;
            self.write_buf[11] = (timestamp >> 48) as u8;
            self.write_buf[12] = (timestamp >> 56) as u8;
        }
        self.write_buf[HEAD_LEN..msglen].copy_from_slice(data.body);
        stream.write_all(self.write_buf[..msglen].as_ref()).await?;
        Ok(())
    }
}
async fn do_read_data(
    readbuf: &mut MsgBuffer,
    stream: &mut TlsStream<TcpStream>,
) -> Result<(), NetError> {
    let buffer = readbuf.spare(8192);
    let size = stream.read(buffer).await?;
    if size == 0 {
        return Err(NetError::TcpDisconnected);
    }
    readbuf.truncate(readbuf.len() + size);
    Ok(())
}
impl<'a> Data<'a> {
    fn from(data: &'a OperationData) -> Self {
        Data {
            cmd: data.cmd.clone(),
            fd: data.fd,
            body: data.body.as_slice(),
            timestamp: data.timestamp,
        }
    }
}
fn u16_to_fd(fd: u16) -> [u8; 2] {
    [fd as u8, (fd >> 8) as u8]
}
fn fd_to_u16(fd: [u8; 2]) -> u16 {
    fd[0] as u16 | ((fd[1] as u16) << 8)
}
impl<'a> From<&'a [u8]> for Data<'a> {
    fn from(data: &'a [u8]) -> Self {
        Self{
            cmd: Cmd::try_from(data[2]).unwrap(),
            fd: [data[3], data[4]],
            body: &data[HEAD_LEN..],
            timestamp: if DEBUG_LEN == 8 {
                Some(
                    data[5] as i64
                        | (data[6] as i64) << 8
                        | (data[7] as i64) << 16
                        | (data[8] as i64) << 24
                    | (data[9] as i64) << 32
                    | (data[10] as i64) << 40
                    | (data[11] as i64) << 48
                    | (data[12] as i64) << 56,
                )
            } else {
                None
            },
        }
    }
}
