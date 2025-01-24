use crate::afterfunc::AfterFn;
use crate::AsyncWriteExt;
use crate::Duration;
use crate::Instant;
use crate::NetError;
use crate::{Receiver, Sender};
use ::tokio::macros::support::Poll::{Pending, Ready};
use async_trait::async_trait;
use openssl::ssl::{Ssl, SslAcceptor};
use static_init::dynamic;
use std::{fmt, mem};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time;
use tokio::time::timeout_at;
use tokio_native_tls::TlsAcceptor;
use tokio_openssl::SslStream;
use crate::buffer::MsgBuffer;

pub type ConnAsyncFn<T> =
    Box<dyn for<'a> FnOnce(&'a mut T) -> ConnAsyncResult<'a> + Send + Sync + 'static>;
pub type ConnAsyncResult<'a> = Pin<Box<dyn Future<Output = Result<(), NetError>> + Send + 'a>>;

pub struct TcpConn<T: Sized> {
    inner: TcpConnBase,
    pub(crate) id: u64,
    pub(crate) react_tx: Sender<ReactOperationChannel>,
    after_fn: Arc<Mutex<AfterFn<T>>>,
}
#[derive(Debug, Clone)]
pub struct ConnWriter {
    react_tx: Sender<ReactOperationChannel>,
}
impl ConnWriter {
    pub async fn write(&self, data: Vec<u8>) -> Result<(), NetError> {
        Ok(self
            .react_tx
            .send(ReactOperationChannel {
                op: ReactOperation::Write(WriteData {
                    data,
                    time_out: None,
                }),
                res: None,
            })
            .await?)
    }
    pub async fn write_with_timeout(
        &self,
        data: Vec<u8>,
        time_out: Instant,
    ) -> Result<(), NetError> {
        Ok(self
            .react_tx
            .send(ReactOperationChannel {
                op: ReactOperation::Write(WriteData {
                    data,
                    time_out: Some(time_out),
                }),
                res: None,
            })
            .await?)
    }
    pub fn close(&self, _reason: Option<impl Into<String>>) -> Result<(), NetError> {
        Ok(self.react_tx.try_send(ReactOperationChannel {
            op: ReactOperation::Exit,
            res: None,
        })?)
    }
}

impl<T> fmt::Debug for TcpConn<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, " id:{},addr:{}", self.id, self.inner.addr())
    }
}
pub trait ConnWrite {
    fn write(&mut self, data: Vec<u8>) -> impl Future<Output = Result<(), NetError>> + Send;
}
impl<T> ConnWrite for TcpConn<T> {
    async fn write(&mut self, data: Vec<u8>) -> Result<(), NetError> {
        self.inner.write_data(data).await
    }
}
impl<T> ConnWrite for &mut TcpConn<T> {
    async fn write(&mut self, data: Vec<u8>) -> Result<(), NetError> {
        self.inner.write_data(data).await
    }
}
impl ConnWrite for ConnWriter {
    async fn write(&mut self, data: Vec<u8>) -> Result<(), NetError> {
        Ok(self
            .react_tx
            .send(ReactOperationChannel {
                op: ReactOperation::Write(WriteData {
                    data,
                    time_out: None,
                }),
                res: None,
            })
            .await?)
    }
}
#[dynamic]
static CONN_ID: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
impl<T> TcpConn<T> {
    pub fn new(
        addr: SocketAddr,
        react_tx: Sender<ReactOperationChannel>,
        after_fn: Arc<Mutex<AfterFn<T>>>,
        readtimeout: Duration,
        stream: TcpStream,
        max_package_size: usize,
    ) -> Self {
        let mut c = TcpConn {
            inner: TcpConnBase::new(addr, stream),
            id: CONN_ID.fetch_add(1, std::sync::atomic::Ordering::Acquire),
            react_tx: react_tx.clone(),
            after_fn: after_fn,
        };
        c.inner.set_react_tx(react_tx);
        c.inner.set_readtimeout(readtimeout);
        c.inner.set_max_package_size(max_package_size);
        //println!("新建{:}地址{:}", c.id, c.readbuf.ptr.addr());
        c
    }

    pub async fn write_data(&mut self, data: Vec<u8>) -> Result<(), NetError> {
        self.inner.write_data(data).await
    }
    pub async fn write_byte(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.inner.write_byte(data).await
    }
    pub async fn write_flush(&mut self) -> Result<(), NetError> {
        self.inner.write_flush().await
    }
    /// 写入数据到缓冲区，记得最后调用write_flush()写出数据到socket
    pub async fn buffer_write(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.inner.buffer_write(data).await
    }
    pub async fn write_data_with_timeout(
        &mut self,
        data: Vec<u8>,
        writetimeout: Duration,
    ) -> Result<(), NetError> {
        Ok(timeout_at(Instant::now() + writetimeout, self.write_data(data)).await??)
    }

    pub fn buffer_len(&self) -> usize {
        self.inner.buffer_len()
    }
    //关闭连接
    pub fn close(&self, _reason: Option<impl Into<String>>) -> Result<(), NetError> {
        Ok(self.react_tx.try_send(ReactOperationChannel {
            op: ReactOperation::Exit,
            res: None,
        })?)
    }
    //退出server
    pub fn exit_server(&self, reason: Option<String>) -> Result<(), NetError> {
        self.react_tx.try_send(ReactOperationChannel {
            op: ReactOperation::ExitServer(reason.clone()),
            res: None,
        })?;
        self.close(reason)?;

        Ok(())
    }
    pub async fn read(&mut self, b: &mut [u8]) -> Result<usize, NetError> {
        self.inner.stream.read(b).await
    }
    pub async fn read_exact(&mut self, b: &mut [u8]) -> Result<usize, NetError> {
        self.inner.stream.read_exact(b).await
    }
    pub async fn read_data(&mut self) -> Result<(), NetError> {
        self.inner.read_data().await
    }
    pub async fn shift(&mut self, n: usize) -> Result<(), NetError> {
        self.inner.shift(n).await
    }
    pub async fn readline(&mut self) -> Result<String, NetError> {
        self.inner.readline().await
    }
    pub fn buffer_data(&mut self) -> &[u8] {
        self.inner.buffer_data()
    }
    //延迟执行，最低单位 秒,编写例子
    //conn.after_fn(Duration::from_secs(10),|conn:&mut ConnRead<S,T>| ->ConnAsyncResult{Box::pin (async move{
    //    Ok(())
    //})});
    pub async fn after_fn<F>(&mut self, delay: time::Duration, f: F)
    where
        F: for<'b> FnOnce(&'b mut T,&'b mut TcpConn<T>) -> ConnAsyncResult<'b> + Send + Sync + 'static,
    {
        self.after_fn.lock().await.add_fn(self.addr(), delay, f);
    }

    pub fn channel_write(&self, data: Vec<u8>) -> Result<(), NetError> {
        Ok(self.react_tx.try_send(ReactOperationChannel {
            op: ReactOperation::Write(WriteData {
                data,
                time_out: None,
            }),
            res: None,
        })?)
    }

    pub fn addr(&self) -> SocketAddr {
        self.inner.addr.clone()
    }
    /// 获取一个写出用的conn
    pub fn get_writer_conn(&self) -> ConnWriter {
        ConnWriter {
            react_tx: self.react_tx.clone(),
        }
    }
    pub fn get_inner(&mut self) -> &mut TcpConnBase {
        &mut self.inner
    }
    pub fn reset_buffer(&mut self) {
        self.inner.readbuf.reset();
    }
}

#[derive(Debug)]
pub(crate) struct WriteData {
    data: Vec<u8>,
    time_out: Option<Instant>,
}

#[derive(Debug)]
pub struct ReactOperationChannel {
    pub(crate) op: ReactOperation,
    pub(crate) res: Option<Sender<ReactOperationResult>>,
}
#[derive(Debug)]
pub(crate) enum ReactOperation {
    Exit, //退出handler
    ExitServer(Option<String>),
    Write(WriteData),
    Afterfn(u64),
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum ReactOperationResult {
    Exit,         //write协程退出了
    WriteOk,      //
    WriteTimeOut, //
}
//主协程

pub(crate) async fn handler_reactreadwrite<A>(
    conn: &mut TcpConn<A>,
    mut react_rx: Receiver<ReactOperationChannel>,
    agent: &mut A,
) -> Result<Option<String>, NetError>
where
    A: Agent + 'static,
{
    let after_fn = conn.after_fn.clone();
    agent.on_opened(conn).await?;
    let mut exit_reason: Option<String> = None;
    //ctx.set_conn(conn.clone());
    #[doc(hidden)]
    mod __tokio_select_util {
        #[derive(Debug)]
        pub(super) enum Out<_0, _1> {
            Read(_0),
            Write(),
            ReactOp(_1),
        }
    }

    loop {
        unsafe {
            let output = {
                ::tokio::macros::support::poll_fn(|cx| {
                    if conn.inner.writebuf.len() > 0 {
                        return Ready(__tokio_select_util::Out::Write());
                    }
                    let f = &mut conn.inner.do_read_data();
                    if let Ready(out) = Future::poll(Pin::new_unchecked(f), cx) {
                        return Ready(__tokio_select_util::Out::Read(out));
                    }
                    let f1 = &mut react_rx.recv();
                    if let Ready(out) = Future::poll(Pin::new_unchecked(f1), cx) {
                        return Ready(__tokio_select_util::Out::ReactOp(out));
                    }
                    Pending
                })
                .await
            };

            match output {
                __tokio_select_util::Out::Write() => {
                    conn.write_flush().await?;
                }
                __tokio_select_util::Out::Read(res) => {
                    res?;

                    while conn.buffer_len() > 0 && agent.react(conn).await? {

                        //     //println!("处理下一条")
                    }
                }
                __tokio_select_util::Out::ReactOp(data) => {
                    if let Some(r) = data {
                        match r.op {
                            ReactOperation::Write(data) => match data.time_out {
                                None => {
                                    conn.inner.stream.write_all(data.data.as_slice()).await?;
                                }
                                Some(deadline) => {
                                    //let outdata = codec.encode(data.data, &mut ctx).await?;
                                    let outdata = data.data;
                                    match timeout_at(
                                        deadline,
                                        conn.inner.stream.write_all(outdata.as_slice()),
                                    )
                                    .await
                                    {
                                        Ok(_) => {
                                            if let Some(recv) = &r.res {
                                                recv.send(ReactOperationResult::WriteOk).await?;
                                            }
                                        }
                                        Err(_e) => {
                                            if let Some(recv) = &r.res {
                                                recv.send(ReactOperationResult::WriteTimeOut)
                                                    .await?;
                                            }
                                        }
                                    }
                                }
                            },
                            ReactOperation::Exit => {
                                break;
                            }
                            ReactOperation::ExitServer(reason) => {
                                exit_reason = reason;
                                break;
                            }
                            ReactOperation::Afterfn(id) => {
                                if let Some(f) = after_fn.lock().await.remove_task(id) {
                                    f.call_once((agent,conn)).await?;
                                };
                            }
                        }
                    } else {
                        return Ok(Some("channel none exit".to_string()));
                    };
                }
            }
        }
        //println!("react{:}地址{:}", conn.id, conn.readbuf.ptr.addr());
    }

    Ok(exit_reason)
}

pub enum Stream {
    Tcp(TcpStream),
    ServerSsl(tokio_native_tls::TlsStream<TcpStream>),
    Openssl(tokio_openssl::SslStream<TcpStream>),
    None,
}

impl Stream {
    pub async fn write_all(&mut self, src: &[u8]) -> Result<(), NetError> {
        match self {
            Stream::Tcp(s) => Ok(s.write_all(src).await?),
            Stream::ServerSsl(s) => Ok(s.write_all(src).await?),
            Stream::Openssl(s) => Ok(s.write_all(src).await?),
            _ => Err(NetError::Custom("write_all 不支持的操作".to_string())),
        }
    }
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, NetError> {
        match self {
            Stream::Tcp(s) => Ok(s.read(buf).await?),
            Stream::ServerSsl(s) => Ok(s.read(buf).await?),
            Stream::Openssl(s) => Ok(s.read(buf).await?),
            _ => Err(NetError::Custom("read 不支持的操作".to_string())),
        }
    }
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<usize, NetError> {
        match self {
            Stream::Tcp(s) => Ok(s.read_exact(buf).await?),
            Stream::ServerSsl(s) => Ok(s.read_exact(buf).await?),
            Stream::Openssl(s) => Ok(s.read_exact(buf).await?),
            _ => Err(NetError::Custom("不支持的操作".to_string())),
        }
    }
}

pub struct TcpConnBase {
    addr: SocketAddr,
    stream: Stream,
    pub(crate) readbuf: MsgBuffer,
    pub(crate) writebuf: MsgBuffer,
    readtimeout: Duration,
    max_package_size: usize,
    react_tx: Option<Sender<ReactOperationChannel>>,
}
impl TcpConnBase {
    pub fn set_buf(&mut self, b: &[u8]) {
        self.readbuf.reset();
        self.readbuf.write(b);
    }
    pub fn new(addr: SocketAddr, stream: TcpStream) -> Self {
        Self {
            addr,
            stream: Stream::Tcp(stream),
            readbuf: MsgBuffer::new(),
            writebuf: MsgBuffer::new(),
            readtimeout: Duration::from_secs(60),
            max_package_size: 0,
            react_tx: None,
        }
    }
    pub fn set_react_tx(&mut self, react_tx: Sender<ReactOperationChannel>) {
        self.react_tx = Some(react_tx)
    }
    pub fn set_readtimeout(&mut self, timeout: Duration) {
        self.readtimeout += timeout;
    }
    pub fn set_max_package_size(&mut self, size: usize) {
        self.max_package_size = size;
    }

    pub async fn write_flush(&mut self) -> Result<(), NetError> {
        if self.writebuf.len() > 0 {
            self.stream.write_all(self.writebuf.as_slice()).await?;
            self.writebuf.reset();
        }
        Ok(())
    }
    pub async fn write_data(&mut self, data: Vec<u8>) -> Result<(), NetError> {
        self.write_byte(data.as_slice()).await
    }
    pub async fn write_byte(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.stream.write_all(data).await?;
        Ok(())
    }
    /// 写入数据到缓冲区，记得最后调用write_flush()写出数据到socket
    pub async fn buffer_write(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.writebuf.write(data);
        //暂定大于16384就先写出
        if self.writebuf.len() > 16384 {
            self.write_flush().await?;
        }
        Ok(())
    }
    pub async fn read_data(&mut self) -> Result<(), NetError> {
        Ok(timeout_at(Instant::now() + self.readtimeout, self.do_read_data()).await??)
    }
    async fn do_read_data(&mut self) -> Result<(), NetError> {
        let buffer = self.readbuf.spare(8192);

        let size = self.stream.read(buffer).await?;

        if size == 0 {
            return Err(NetError::TcpDisconnected);
        }
        let newlen = self.readbuf.len() + size;
        if self.max_package_size > 0 && newlen > self.max_package_size {
            return Err(NetError::LargePackage);
        }
        self.readbuf.truncate(newlen);
        Ok(())
    }
    pub async fn shift(&mut self, mut n: usize) -> Result<(), NetError> {
        if self.readbuf.len() >= n {
            self.doshift(n);
            return Ok(());
        }
        let timeout = self.readtimeout;
        let mut feature = async move || -> Result<(), NetError> {
            while n > 0 {
                self.read_data().await?;
                let l = self.readbuf.len();
                if l >= n {
                    self.doshift(n);
                    return Ok(());
                } else {
                    self.doshift(l);
                    n -= l
                }
            }

            Ok(())
        };
        Ok(timeout_at(Instant::now() + timeout, feature()).await??)
    }
    fn doshift(&mut self, n: usize) {
        self.readbuf.shift(n);
    }
    pub fn buffer_data(&self) -> &[u8] {
        self.readbuf.as_slice()
    }
    pub fn buffer_next(&mut self) -> Option<u8> {
        self.readbuf.next()
    }
    pub fn buffer_nth(&mut self, u: usize) -> Option<u8> {
        self.readbuf.nth(u)
    }
    pub async fn readline(&self) -> Result<String, NetError> {
        Err(NetError::Custom("readline未处理".to_string()))
    }
    pub fn close(&self, _reason: Option<impl Into<String>>) -> Result<(), NetError> {
        if let Some(tx) = &self.react_tx {
            return Ok(tx.try_send(ReactOperationChannel {
                op: ReactOperation::Exit,
                res: None,
            })?);
        };
        Ok(())
    }
    pub fn get_writer_conn(&self) -> Result<ConnWriter, NetError> {
        if let Some(tx) = &self.react_tx {
            return Ok(ConnWriter {
                react_tx: tx.clone(),
            });
        };
        Err(NetError::Custom(
            "需要从server启动 Conn > ConnBase > ConnWriter".to_string(),
        ))
    }
    pub fn buffer_len(&self) -> usize {
        self.readbuf.len()
    }
    pub fn buffer_clear(&mut self) {
        self.readbuf.reset();
    }
    pub fn reset_buffer(&mut self) {
        self.readbuf.reset();
    }
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
    fn take_stream(&mut self)->Stream {
       mem::replace(&mut self.stream, Stream::None)
    }
    pub async fn upgrade_ssl(&mut self, acceptor: Arc<TlsAcceptor>) -> Result<(), NetError> {
        if let Stream::Tcp(tcp_stream) = self.take_stream() {
            let tls_stream = acceptor.accept(tcp_stream).await?;
            self.stream = Stream::ServerSsl(tls_stream);
        }else {
            return Err(NetError::Tls("It can only be upgraded from TcpStream to SSL".to_string()))
        }
        Ok(())
    }
    pub async fn upgrade_openssl(&mut self, acceptor: Arc<SslAcceptor>) -> Result<(), NetError> {
        if let Stream::Tcp(tcp_stream) = self.take_stream() {
            let ssl = Ssl::new(acceptor.context()).unwrap();
            let mut stream = SslStream::new(ssl, tcp_stream).unwrap();
            Pin::new(&mut stream)
                .accept()
                .await
                .or_else(|e| Err(NetError::Tls(e.to_string())))?;
            self.stream = Stream::Openssl(stream);
        }else {
            return Err(NetError::Tls("It can only be upgraded from TcpStream to SSL".to_string()))
        }
        Ok(())
    }
}
pub struct NoSSL {}

impl AsyncRead for NoSSL {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        todo!()
    }
}
impl AsyncWrite for NoSSL {
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        todo!()
    }
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        todo!()
    }
    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        todo!()
    }
}

//Agent 每个conn伴随一个agent
#[async_trait]
pub trait Agent: Sized {
    //tcp连上的时候会调用此方法
    fn on_opened(
        &mut self,
        conn: &mut TcpConn<Self>,
    ) -> impl Future<Output = Result<(), NetError>> + Send;

    /// 返回true，则表示continue，处理下一个消息；false为不是一个完整消息
    /// 例如：收到一个web请求，但是消息未接收完整，返回false，等待下次接收更多消息重新处理
    fn react(
        &mut self,
        conn: &mut TcpConn<Self>,
    ) -> impl Future<Output = Result<bool, NetError>> + Send;

    fn on_closed(
        &mut self,
        conn: &mut TcpConn<Self>,
        reasion: Option<String>,
    ) -> impl Future<Output = Result<(), NetError>> + Send;
}
