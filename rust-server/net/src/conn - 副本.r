
use crate::afterfunc::AfterFn;
use crate::buffer::MsgBufferStatic;
use crate::AsyncWriteExt;
use crate::Duration;
use crate::EventHandler;
use crate::Instant;
use crate::NetError;
use crate::{Receiver, Sender};
use ::tokio::macros::support::Poll::{Pending, Ready};
use std::fmt;
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

pub type ConnAsyncFn<T> =
    Box<dyn for<'a> FnOnce(&'a mut T) -> ConnAsyncResult<'a> + Send + Sync + 'static>;
pub type ConnAsyncResult<'a> = Pin<Box<dyn Future<Output = Result<(), NetError>> + Send + 'a>>;

pub struct Conn<T: ?Sized, S: ConnBaseTrait> {
    inner: ConnBase<S>,
    pub(crate) id: u64,
    pub(crate) react_tx: Sender<ReactOperationChannel>,
    after_fn: Arc<Mutex<AfterFn<T, S>>>,
}
#[derive(Debug, Clone)]
pub struct ConnWrite {
    react_tx: Sender<ReactOperationChannel>,
}
impl ConnWrite {
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
unsafe impl<T, S: ConnBaseTrait> Send for Conn<T, S> {}
unsafe impl<T, S: ConnBaseTrait> Sync for Conn<T, S> {}

impl<T, S: ConnBaseTrait> fmt::Debug for Conn<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, " id:{},addr:{}", self.id, self.inner.addr)
    }
}

static mut CONN_ID: AtomicU64 = AtomicU64::new(0);
impl<T, S> Conn<T, S>
where
    S: ConnBaseTrait,
{
    pub fn new(
        addr: SocketAddr,
        react_tx: Sender<ReactOperationChannel>,
        after_fn: Arc<Mutex<AfterFn<T, S>>>,
        readtimeout: Duration,
        stream: Stream<S>,
        max_package_size: usize,
    ) -> Self {
        unsafe {
            let mut c = Conn {
                inner: ConnBase::new(addr, stream),
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
    }

    pub async fn write_data(&mut self, data: Vec<u8>) -> Result<(), NetError> {
        self.inner.write_data(data).await?;
        Ok(())
    }
    pub async fn write_byte(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.inner.write_byte(data).await?;
        Ok(())
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

    //延迟执行，最低单位 秒,编写例子
    //conn.after_fn(Duration::from_secs(10),|conn:&mut ConnRead<S,T>| ->ConnAsyncResult{Box::pin (async move{
    //    Ok(())
    //})});
    pub async fn after_fn<F>(&mut self, delay: time::Duration, f: F)
    where
        F: for<'b> FnOnce(&'b mut Conn<T, S>) -> ConnAsyncResult<'b> + Send + Sync + 'static,
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
    pub fn get_write_conn(&self) -> ConnWrite {
        ConnWrite {
            react_tx: self.react_tx.clone(),
        }
    }
    pub fn get_inner(&mut self) -> &mut ConnBase<S> {
        &mut self.inner
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

pub(crate) async fn handler_reactreadwrite<E, T, S>(
    mut conn: Conn<T, S>,
    mut event_handler: E,
    mut react_rx: Receiver<ReactOperationChannel>,
    callback: Option<Box<dyn FnOnce(T) -> Result<T, NetError> + Send>>,
) -> Result<Option<String>, NetError>
where
    E: EventHandler<T, S> + Send + Copy + Sync,
    T: Send,
    S: ConnBaseTrait + 'static,
{
    let mut ctx = match callback {
        Some(f) => f.call_once((event_handler.on_opened(&mut conn.inner).await?,))?,
        None => event_handler.on_opened(&mut conn.inner).await?,
    };

    //ctx.set_conn(conn.clone());
    #[doc(hidden)]
    mod __tokio_select_util {
        #[derive(Debug)]
        pub(super) enum Out<_0, _1> {
            Read(_0),
            ReactOp(_1),
        }
    }
    let mut exit_reason: Option<String> = None;
    let after_fn = conn.after_fn.clone();
    loop {
        unsafe {
            let output = {
                ::tokio::macros::support::poll_fn(|cx| {
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
                __tokio_select_util::Out::Read(res) => {
                    //println!("ok read");
                    res?;
                    while conn.buffer_len() > 0 && event_handler.react(&mut conn.inner, &mut ctx).await? {
                        //println!("处理下一条")
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
                                    f.call_once((&mut conn,)).await?;
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
pub enum Stream<S> {
    Tcp(TcpStream),
    Ssl(S),
}

impl<S> Stream<S>
where
    S: ConnBaseTrait,
{
    pub async fn write_all(&mut self, src: &[u8]) -> Result<(), NetError> {
        match self {
            Stream::Tcp(s) => Ok(s.write_all(src).await?),
            Stream::Ssl(s) => Ok(s.write_all(src).await?),
        }
    }
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, NetError> {
        match self {
            Stream::Tcp(s) => Ok(s.read(buf).await?),
            Stream::Ssl(s) => Ok(s.read(buf).await?),
        }
    }
}
pub trait ConnBaseTrait = AsyncReadExt + AsyncWriteExt + std::marker::Unpin + Send + Sync;
pub struct ConnBase<S: ConnBaseTrait> {
    addr: SocketAddr,
    stream: Stream<S>,
    pub(crate) readbuf: MsgBufferStatic,
    readtimeout: Duration,
    max_package_size: usize,
    react_tx: Option<Sender<ReactOperationChannel>>,
}
impl<S: ConnBaseTrait> ConnBase<S> {
    pub fn new(addr: SocketAddr, stream: Stream<S>) -> Self {
        Self {
            addr: addr,
            stream,
            readbuf: MsgBufferStatic::new().into(),
            readtimeout: Duration::from_secs(60),
            max_package_size: crate::MAX as usize,
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
    pub async fn write_data(&mut self, data: Vec<u8>) -> Result<(), NetError> {
        self.stream.write_all(data.as_slice()).await?;
        Ok(())
    }
    pub async fn write_byte(&mut self, data: &[u8]) -> Result<(), NetError> {
        self.stream.write_all(data).await?;
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
        if newlen > self.max_package_size {
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
    pub fn buffer_next(&self) -> Option<u8> {
        unsafe { (*self.readbuf.ptr).next() }
    }
    pub fn buffer_nth(&self, u: usize) -> Option<u8> {
        unsafe { (*self.readbuf.ptr).nth(u) }
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
    pub fn get_write_conn(&self) -> Result<ConnWrite, NetError> {
        if let Some(tx) = &self.react_tx {
            return Ok(ConnWrite {
                react_tx: tx.clone(),
            });
        };
        Err(NetError::Custom(
            "需要从server启动 Conn > ConnBase > ConnWrite".to_string(),
        ))
    }
    pub fn buffer_len(&self) -> usize {
        self.readbuf.len()
    }
    pub fn addr(&self) -> SocketAddr {
        self.addr
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
