#![feature(mem_copy_fn)]

use crate::cmd::Cmd;
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use codec::http1::Request;
use codec::http1example::Http1Agent;
use codec::response::Response;
use codec::router::Router;
use dns_lookup::lookup_addr;
use net::afterfunc::ConnAsyncResult;
use net::buffer::MsgBuffer;
use net::conn::{ConnWrite, ConnWriter, TcpConn};
use net::err::NetError;
use net::{Agent, Server};
use openssl::pkey::PKey;
use openssl::ssl::{SslAcceptor, SslMethod};
use openssl::x509::X509;
use static_init::dynamic;
use std::collections::HashMap;
use std::iter::repeat_with;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::Mutex;
use tokio::sync::mpsc::*;
use tokio::time::{Instant, timeout, timeout_at};
use tokio_native_tls::native_tls::Identity;

mod cmd;
pub mod utils;
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

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let http_addr = "0.0.0.0:8080";
    let addr = "0.0.0.0:3304";
    let cert = include_bytes!(".././config/cert");
    let key = include_bytes!(".././config/key");

    let key = PKey::private_key_from_pem(key).unwrap();
    let pkcs8 = key.private_key_to_pem_pkcs8().unwrap();
    let identity = Identity::from_pkcs8(cert, pkcs8.as_slice()).unwrap();
    let cert = X509::from_pem(cert).unwrap();

    let tls_acceptor = native_tls::TlsAcceptor::new(identity).unwrap();
    let tls_acceptor: Arc<TlsAcceptor> = Arc::new(tls_acceptor.into());
    let tls_acceptor2 = tls_acceptor.clone();
    // 创建 SSL/TLS 上下文
    let mut openssl_acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    openssl_acceptor.set_private_key(key.as_ref()).unwrap();
    openssl_acceptor.set_certificate(cert.as_ref()).unwrap();
    let openssl_acceptor = Arc::new(openssl_acceptor.build());
    let openssl_acceptor2 = openssl_acceptor.clone();

    tokio::spawn(async move {
        let router = Router::new()
            .get_async("/", |_, _| async move {
                Response::redirect_with_status("/index.html", 302)
            })
            .get_async("/empty", |_, _| async move { Response::ok("") })
            .get_async("/garbage", garbage)
            .get_async("/getIP", get_ip)
            .get_async("/{*path}", |req, _| async move {
                let path = format!("./static/{}", req.path());
                let file = tokio::fs::read(path).await?;
                Response::from_html(String::from_utf8_lossy(file.as_slice()))
            });
        println!("启动http {}", http_addr);
        Server::new()
            .start_server(http_addr, Http1Agent {
                is_https: false,
                router,
                session: (),
                openssl_acceptor,
                tls_acceptor: tls_acceptor,
            })
            .await
            .expect("http启动失败");
    });
    println!("启动代理 {}", addr);
    Server::new()
        .start_server(addr, ServerAgent {
            client: None,
            tls_acceptor: tls_acceptor2,
            openssl_acceptor: openssl_acceptor2,
        })
        .await
        .expect("主server启动失败");
    Ok(())
}

async fn get_ip(req: Request, _: ()) -> Result<Response, NetError> {
    Response::from_html(format!(r#"{{"processedString":"{}"}}"#, req.remote_addr()))
}
async fn garbage(_req: Request, _: ()) -> Result<Response, NetError> {
    unsafe {
        let vec_u64: Vec<u64> = repeat_with(|| fastrand::u64(..))
            .take(1024 * 1024 * 10 / 8)
            .collect();
        let len = vec_u64.len();
        let ptr = vec_u64.as_ptr() as *mut u8;
        let capacity = vec_u64.capacity() * std::mem::size_of::<u64>();

        // 从原始指针创建一个新的 Vec<u8>
        let vec_u8 = Vec::from_raw_parts(ptr, len * 8, capacity);

        // 释放原始的 Vec<u64>，避免内存泄漏
        std::mem::forget(vec_u64);
        Response::byte(vec_u8)
    }
}
//---------------非http部分--------------------------------------------------
#[dynamic]
static CONNECTION_ID: AtomicU32 = AtomicU32::new(1);
static CAPABILITY_RESERVED: &[u8] = &[
    254, 255, 45, 2, 0, 255, 193, 21, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0,
];
static BAD_HANDSHAKE: &[u8] = &[
    255, 19, 4, 66, 97, 100, 32, 104, 97, 110, 100, 115, 104, 97, 107, 101,
];
const CLIENT_SSL: u32 = 0x00000800;
const CLIENT_SECURE_CONNECTION: u32 = 0x00008000;

#[dynamic]
static CLIENT_M: Arc<Mutex<HashMap<String, Arc<Client>>>> = Arc::new(Mutex::new(HashMap::new()));
use crate::utils::now;
use tokio_native_tls::{TlsAcceptor, native_tls};

#[derive(Clone)]
pub struct ServerAgent {
    client: Option<Arc<Client>>,
    tls_acceptor: Arc<TlsAcceptor>,
    openssl_acceptor: Arc<SslAcceptor>,
}

struct Client {
    //conn_m: Mutex<HashMap<String, Arc<Client>>>,
    fd_m: Mutex<HashMap<[u8; 2], Arc<Box<Conn>>>>,
    key: String,
}
impl Client {
    async fn get_fd_conn(&self, fd: &[u8; 2]) -> Option<Arc<Box<Conn>>> {
        if let Some(conn) = self.fd_m.lock().await.get(fd) {
            let conn = conn.clone();
            Some(conn)
        } else {
            None
        }
    }
}
struct Conn {
    windows_size: AtomicI64,
    //ctx: Arc<Ctx>,
    //remote_conn: Option<TcpStream>, //代理请求的远程ip创建的conn
    client_conn: ConnWriter, //可以写回客户端的write接口
    is_close: AtomicBool,
    address: String,
    fd: [u8; 2],
    write: Sender<Option<Vec<u8>>>,
    //rec_no: u32,
    //msg_no: u32,
    wait_tx: Sender<bool>,
    //send: u64,
    //close_reason: String,
    client: Arc<Client>,
}
unsafe impl Send for ServerAgent {}
impl Agent for ServerAgent {
    async fn on_opened(&mut self, conn: &mut TcpConn<Self>) -> Result<(), NetError> {
        conn.after_fn(
            Duration::from_secs(2),
            |agent: &mut Self, conn: &mut TcpConn<Self>| -> ConnAsyncResult {
                Box::pin(async move {
                    //握手失败关闭连接
                    if agent.client.is_none(){
                        conn.close(None::<String>)?;
                    };
                    Ok(())
                })
            },
        )
        .await;
        let mut mysqlbuf = MsgBuffer::new();
        fastrand::seed(utils::now().unix() as u64);
        mysqlbuf.make(4); //生成4位头
        mysqlbuf.write_byte(10);
        mysqlbuf.write_string("5.5.5-10.5.1-MariaDB");
        mysqlbuf.write_byte(0);
        let id = CONNECTION_ID.fetch_add(fastrand::u32(1..=5), Ordering::Relaxed);

        LittleEndian::write_u32(mysqlbuf.make(4), id);
        let b = mysqlbuf.make(9);
        LittleEndian::write_u64(b, fastrand::u64(..)); //产生个随机数，用于校验//auth-plugin-data-part-1
        b[8] = 0; //[00] filler
        mysqlbuf.write(CAPABILITY_RESERVED); //
        let b = mysqlbuf.make(13);
        LittleEndian::write_u64(b, fastrand::u64(..)); //挑战随机数的9-16位
        LittleEndian::write_u32(&mut b[8..], fastrand::u32(..)); //挑战随机数的16-20位
        b[12] = 0;
        mysqlbuf.write_string("mysql_native_password"); //密码套件
        mysqlbuf.write_byte(0); //结束
        let len = mysqlbuf.len();
        LittleEndian::write_u32(&mut mysqlbuf.as_mut_slice()[..4], len as u32 - 4); //写入长度
        conn.write_byte(mysqlbuf.bytes()).await?;
        self.handshake(conn).await?;
        Ok(())
    }

    async fn react(&mut self, tcp_conn: &mut TcpConn<Self>) -> Result<bool, NetError> {
        let begen = now();
        let indata = tcp_conn.buffer_data();

        let length = (indata[0] as usize) + ((indata[1] as usize) << 8);
        if indata.len() < length {

            return Ok(false);
        }
        let indata = indata[..length].to_vec();
        let mut data: Data = indata.as_slice().into();
        let cmd = data.cmd.clone();
        tcp_conn.shift(length).await?;
        //主逻辑
        #[cfg(debug_assertions)]
        if DEBUG_LEN == 8 {
            println!(
                "cmd: {:?} 开始 耗时{}ms",
                data.cmd,
                now().unix_millis() - data.timestamp.unwrap()
            );
        }

        match &data.cmd {
            Cmd::GetFd => {
                if self.client.is_none() {
                    return Err(NetError::Custom("cmd 错误？？".to_string()));
                }
                let port = BigEndian::read_u16(&data.body[data.body.len() - 2..]);
                let addr = String::from_utf8_lossy(&data.body[..data.body.len() - 2]).to_string()
                    + ":"
                    + port.to_string().as_str();
                let (tx, mut rx) = channel(1000);
                let (wait_tx, wait_rx) = channel(1);
                let conn_writer = tcp_conn.get_writer_conn();
                let conn = Arc::new(Box::new(Conn {
                    windows_size: AtomicI64::new(WINDOWS_UPDATE_SIZE),
                    client_conn: conn_writer.clone(),
                    is_close: AtomicBool::new(false),
                    address: addr.clone(),
                    fd: data.fd,
                    write: tx,
                    wait_tx,
                    client: self.client.as_ref().unwrap().clone(),
                }));
                conn.client.fd_m.lock().await.insert(data.fd, conn.clone());
                tokio::spawn(async move {
                    println!("connect addr {}",addr);
                    match TcpStream::connect(addr.clone()).await {
                        Ok(mut netconn) => {
                            if !conn.is_close.load(Ordering::Relaxed) {
                                let (read, mut write) = netconn.into_split();

                                tokio::spawn(async move {
                                    if let Err(e) =
                                        handle_socks(conn.clone(), read, conn_writer, wait_rx).await
                                    {
                                        conn.close(
                                            conn.address.to_string()
                                                + " 网站读取出错"
                                                + &e.to_string(),
                                        )
                                        .await;
                                        return;
                                    };
                                    #[cfg(debug_assertions)]
                                    println!("{:?} 网站连接断开", conn.fd);

                                });
                                while let Some(b) = rx.recv().await {
                                    if let Some(b) = b {
                                        if write.write_all(b.as_slice()).await.is_err() {
                                            rx.close();
                                            return;
                                        };
                                    } else {
                                        rx.close();
                                        return;
                                    }
                                }
                            } else {
                                let _ = netconn.shutdown().await;
                            }
                        }
                        Err(e) => {
                            conn.close(format!("fd拨号失败,addr: {} err: {}",addr,e.to_string())).await;

                            return;
                        }
                    }
                });
            }
            Cmd::Reg => {
                let key = String::from_utf8_lossy(&data.body).to_string();

                let mut map = CLIENT_M.lock().await;
                if let Some(client) = map.get(&key) {
                    self.client = Some(client.clone());
                } else {
                    self.client = Some(Arc::new(Client {
                        //conn_m: Default::default(),
                        fd_m: Default::default(),
                        key: key.clone(),
                    }));
                    let client = self.client.as_ref().unwrap().clone();
                    map.insert(key, client);
                    data.cmd = Cmd::DeleteIp;
                    write_by_conn(tcp_conn, data).await?; //清空客户端的fd
                }
                //let mut data=vec![255; 65535 ];
                //data[2]=0;
                //tcp_conn.write_data(data).await?; //消灭分包
            }
            Cmd::Ping => {
                data.cmd = Cmd::Pong;
                write_by_conn(tcp_conn, data).await?;
            }
            Cmd::Msg => {
                if let Some(client) = &self.client {
                    if let Some(conn) = client.get_fd_conn(&data.fd).await {
                        conn.windows_update(data.body).await;
                        let data = data.body[UPDATE_SIZE_LEN..].to_vec();
                        //println!("send之前 {}",String::from_utf8_lossy(data.as_slice()));
                        conn.write.send(Some(data)).await?;
                    } else {
                        let out_data = Data {
                            cmd: Cmd::DeleteFd,
                            fd: data.fd,
                            body: &[],
                            timestamp: None,
                        };
                        write_by_conn(tcp_conn, out_data).await?;
                        return Ok(true);
                    }
                }
            }
            Cmd::DeleteFd => {
                if let Some(client) = &self.client {
                    if let Some(conn) = client.get_fd_conn(&data.fd).await {
                        conn.close("客户端要求关闭").await;
                    }
                }
            }
            Cmd::WindowsUpdate => {
                if let Some(client) = &self.client {
                    if let Some(conn) = client.get_fd_conn(&data.fd).await {
                        conn.windows_update(data.body).await;
                    } else {
                        let out_data = Data {
                            cmd: Cmd::DeleteFd,
                            fd: data.fd,
                            body: &[],
                            timestamp: None,
                        };
                        write_by_conn(tcp_conn, out_data).await?;
                    }
                }
            }
            Cmd::None => {}
            n => {
                println!("未处理 cmd:{:?} ", n);
            }
        }
        #[cfg(debug_assertions)]
        println!(
            "cmd: {:?} 结束 耗时{}ms",
            cmd,
            now().unix_millis() - begen.unix_millis()
        );

        return Ok(true);
    }
    async fn on_closed(
        &mut self,
        _conn: &mut TcpConn<Self>,
        _reasion: Option<String>,
    ) -> Result<(), NetError> {
        if let Some(client) = &self.client {
            let key = client.key.clone();
            CLIENT_M.lock().await.remove(&key);
        }
        Ok(())
    }
}
impl ServerAgent {
    async fn handshake(&mut self, conn: &mut TcpConn<Self>) -> Result<(), NetError> {
        let mut head: [u8; 3] = [0; 3];
        let size = conn.read_exact(&mut head).await?;
        if size < 3 {
            return Err(NetError::Custom("数据包长度不对".to_string()));
        }
        //检查mysql握手
        let mut msglen = (head[0] as usize) | (head[1] as usize) << 8 | (head[2] as usize) << 16;
        let mut data = vec![0; msglen + 1];
        let size = conn.read_exact(&mut data).await?;
        if size < data.len() {
            return Err(NetError::Custom("数据包长度不对".to_string()));
        }
        if data[0] != 1 {
            conn.write_byte(
                vec![
                    33, 0, 0, 2, 255, 132, 4, 35, 48, 56, 83, 48, 49, 71, 111, 116, 32, 112, 97,
                    99, 107, 101, 116, 115, 32, 111, 117, 116, 32, 111, 102, 32, 111, 114, 100,
                    101, 114,
                ]
                .as_slice(),
            )
            .await?;
            return Err(NetError::Custom("老子就是要关闭".to_string()));
        }
        //读取flag

        let flag = LittleEndian::read_u32(&data[1..5]);
        if flag & CLIENT_SSL != 0 && msglen < 32 {
            conn.write_byte(BAD_HANDSHAKE).await?;
            return Err(NetError::Close);
        }
        if flag & CLIENT_SSL != 0 {
            //设置tls
            conn.get_inner()
                .upgrade_openssl(self.openssl_acceptor.clone())
                .await?;
            return Ok(());
        } else if msglen > 36 {
            let mut mysqlbuf = MsgBuffer::new();
            mysqlbuf.write(&data[33..]);
            let username = read_null_terminated_string(&mut mysqlbuf)?;
            let password = if flag & CLIENT_SECURE_CONNECTION != 0 {
                let size = mysqlbuf.spare(1)[0] as usize;
                String::from_utf8_lossy(&mysqlbuf.spare(size)).to_string()
            } else {
                read_null_terminated_string(&mut mysqlbuf)?
            };
            //构建errpaket
            mysqlbuf.reset();
            let b = mysqlbuf.make(13);
            b[3] = 2;
            b[4] = 0xff;
            LittleEndian::write_u16(&mut b[5..7], 1045);
            let data: &mut [u8] = &mut [35, 50, 56, 48, 48, 48];
            unsafe {
                std::ptr::copy(data.as_ptr(), b[7..].as_mut_ptr(), 6);
            }
            mysqlbuf.write_string("Access denied for user '");
            mysqlbuf.write_string(username.as_str());
            mysqlbuf.write_string("'@'");
            let addr = conn.addr().to_string();
            let ip = addr.split(":").next().unwrap();
            match lookup_addr(&IpAddr::from_str(ip).unwrap()) {
                Ok(host) => mysqlbuf.write_string(host.as_str()),
                Err(_) => mysqlbuf.write_string(ip),
            };

            mysqlbuf.write_string("' (using password: ");
            if password == "" {
                mysqlbuf.write_string("NO))");
            } else {
                mysqlbuf.write_string("YES))");
            }
            msglen = mysqlbuf.len() - 4;
            let b = &mut mysqlbuf.as_mut_slice()[..3];
            b[0] = msglen as u8;
            b[1] = (msglen >> 8) as u8;
            b[2] = (msglen >> 16) as u8;
            conn.write_byte(mysqlbuf.bytes()).await?;
            return Ok(());
        }
        Ok(())
    }
}
fn read_null_terminated_string(msg: &mut MsgBuffer) -> Result<String, NetError> {
    for (k, v) in msg.bytes().iter().enumerate() {
        if v == &0 {
            let out = String::from_utf8_lossy(&msg.bytes()[0..k]).to_string();
            msg.shift(k + 1);
            return Ok(out);
        }
    }
    Ok("".to_string())
}
async fn handle_socks(
    conn: Arc<Box<Conn>>,
    mut read: OwnedReadHalf,
    mut conn_writer: ConnWriter,
    mut wait_rx: Receiver<bool>,
) -> Result<(), NetError> {
    let mut buf: Vec<u8> = vec![0; MAX_PLAINTEXT - HEAD_LEN];

    while !conn.is_close.load(Ordering::Relaxed) {
        let n = match timeout(Duration::from_secs(10), read.read(&mut buf)).await {
            Err(_) => {
                //#[cfg(debug_assertions)]
                //println!("{:?} 超时了吗? {:?}",now(), e);
                continue;
            }
            Ok(n) => n?,
        };
        if n==0{
            return Err(NetError::TcpDisconnected);
        }else{
            let data = Data {
                cmd: Cmd::Msg,
                fd: conn.fd,
                body: &buf[..n],
                timestamp: None,
            };
            write_by_conn(&mut conn_writer, data).await?;
            conn.windows_size
                .fetch_add(-1 * (n as i64), Ordering::Relaxed);
            while !conn.is_close.load(Ordering::Relaxed)
                && conn.windows_size.load(Ordering::Relaxed) <= 0
            {
                let flag =
                    timeout_at(Instant::now() + Duration::from_secs(1), wait_rx.recv()).await?;
                if flag.is_some() && !flag.unwrap() {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}
async fn write_by_conn<T>(stream: &mut T, data: Data<'_>) -> Result<(), NetError>
where
    T: ConnWrite,
{
    if data.body.len() + HEAD_LEN > MAX_PLAINTEXT {
        return Err(NetError::ProtocolErr(format!(
            "data长度 {} 大于 {}",
            data.body.len(),
            MAX_PLAINTEXT - HEAD_LEN
        )));
    }
    let msglen = HEAD_LEN + data.body.len();
    let mut write_buf = vec![0; msglen];
    write_buf[0] = msglen as u8;
    write_buf[1] = (msglen >> 8) as u8;
    write_buf[2] = data.cmd as u8;
    write_buf[3] = data.fd[0];
    write_buf[4] = data.fd[1];
    if DEBUG_LEN == 8 {
        let timestamp = if data.timestamp.is_none() {
            now().unix()
        } else {
            data.timestamp.unwrap()
        };
        write_buf[5] = timestamp as u8;
        write_buf[6] = (timestamp >> 8) as u8;
        write_buf[7] = (timestamp >> 16) as u8;
        write_buf[8] = (timestamp >> 24) as u8;
    }

    write_buf[HEAD_LEN..msglen].copy_from_slice(data.body);
    stream.write(write_buf).await?;

    Ok(())
}
impl Conn {
    async fn close(&self, reason: impl Into<String>) {
        if self
            .is_close
            .compare_exchange_weak(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            let _ = self.write.send(None).await;
            #[cfg(debug_assertions)]
            println!("conn fd {:?} 关闭了 原因 {}", self.fd, reason.into());

            self.client.fd_m.lock().await.remove(&self.fd);

            let data = Data {
                cmd: Cmd::DeleteFd,
                fd: self.fd,
                body: &[],
                timestamp: None,
            };
            let mut write = self.client_conn.clone();
            let _ = write_by_conn(&mut write, data).await;
            let wait_tx = self.wait_tx.clone();
            tokio::spawn(async move {
                let _ = timeout(Duration::from_secs(10), wait_tx.send(false)).await;
            });
        }
    }
    async fn windows_update(&self, data: &[u8]) {
        let windows_update_size = (data[0] as i64)
            | (data[1] as i64) << 8
            | (data[2] as i64) << 16
            | (data[3] as i64) << 24
            | (data[4] as i64) << 32
            | (data[5] as i64) << 40
            | (data[6] as i64) << 48
            | (data[7] as i64) << 56;

        if windows_update_size != 0 {
            let old = self
                .windows_size
                .fetch_add(windows_update_size, Ordering::Relaxed);
            if old < 0 {
                let tx = self.wait_tx.clone();
                tokio::spawn(async move {
                    let res = timeout(Duration::from_secs(1), tx.send(true)).await;
                    #[cfg(debug_assertions)]
                    if res.is_err() {
                        println!("{:?}", res)
                    } else {
                        let res = res.unwrap();
                        if res.is_err() {
                            println!("{:?}", res)
                        }
                    }

                });
            }
        }
    }
}
impl<'a> From<&'a [u8]> for Data<'a> {
    fn from(data: &'a [u8]) -> Self {
        Self {
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
