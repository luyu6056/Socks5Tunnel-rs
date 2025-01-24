#![feature(fn_traits)]
#![feature(trait_alias)]
#![feature(let_chains)]
#![feature(rt)]
#![feature(if_let_guard)]
#![feature(ptr_as_uninit)]

use crate::afterfunc::AfterFn;

use crate::conn::*;
use ::tokio::macros::support::Poll::{Pending, Ready};
use err::NetError;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{ Mutex};
use tokio::sync::mpsc::{*};
use tokio::time;
use tokio::time::{Duration, Instant};
use wg::AsyncWaitGroup;

pub mod afterfunc;
pub mod buffer;
pub mod codec;
pub mod conn;
pub mod err;
pub mod pool;
pub mod wg;
pub use crate::conn::Agent;
#[allow(dead_code)]
pub struct Server {
    max_package_size: usize,
    read_timeout: Duration,
    write_timeout: Duration,
    exit_tx: Sender<NetError>,
    exit_rx: Receiver<NetError>,
    //conn_map: Arc<Mutex<HashMap<SocketAddr, TcpConn<T>>>>,
    //afterfn: Arc<Mutex<AfterFn<T>>>,
}
unsafe impl Send for Server {}
impl Server {
    /// Server::<HttpAgent, Handler,net::conn::NoSSL>::new_with_codec(
    pub fn new() -> Server {
        let (exit_tx, exit_rx) = tokio::sync::mpsc::channel::<NetError>(10);
        Server {
            max_package_size: 0,
            read_timeout: Duration::from_secs(60),
            write_timeout: Duration::from_secs(60),
            exit_tx,
            exit_rx,
        }
    }
    //pub fn with_eventhandler(eventhandler: E) {}
    pub fn with_max_package_size(mut self, size: usize) -> Server {
        self.max_package_size = size;
        self
    }
    pub fn with_readtimeout(mut self, readtimeout: Duration) -> Server {
        self.read_timeout = readtimeout;
        self
    }
    pub fn with_writetimeout(mut self, writetimeout: Duration) -> Server {
        self.write_timeout = writetimeout;
        self
    }

    pub async fn start_server<A>(&mut self, addr: &str, agent: A) -> Result<(), NetError>
    where
        A: Agent + Send + 'static + Clone,
    {
        let conn_map: HashMap<SocketAddr, Sender<conn::ReactOperationChannel>> = Default::default();
        let conn_map = Arc::new(Mutex::new(conn_map));
        let afterfn = Arc::new(Mutex::new(AfterFn::<A>::new()));

        let listener = TcpListener::bind(&addr).await.unwrap();
        //let mut intervalsec = time::interval(Duration::from_secs(1));
        let mut intervalms = time::interval(Duration::from_millis(1));

        unsafe {
            mod __tokio_select_util {
                #[derive(Debug)]
                pub(super) enum Out<_0, _1, _2> {
                    _0(_0),
                    _1(_1),
                    _2(_2),
                    //_3(_3),
                }
            }
            loop {
                let output = {
                    ::tokio::macros::support::poll_fn(|cx| {
                        if let Ready(out) = listener.poll_accept(cx) {
                            return Ready(__tokio_select_util::Out::_0(out));
                        }
                        let f1 = &mut self.exit_rx.recv();
                        if let Ready(out) = Future::poll(Pin::new_unchecked(f1), cx) {
                            return Ready(__tokio_select_util::Out::_1(out));
                        }
                        if let Ready(out) = intervalms.poll_tick(cx) {
                            return Ready(__tokio_select_util::Out::_2(out));
                        }
                        // if let Ready(out) = intervalsec.poll_tick(cx) {
                        //     return Ready(__tokio_select_util::Out::_3(out));
                        // }
                        Pending
                    })
                    .await
                };
                let conn_map = conn_map.clone();
                let afterfn = afterfn.clone();

                match output {
                    __tokio_select_util::Out::_0(accept) => {
                        let (stream, addr) = accept?;
                        self.start_conn_handle(stream, addr, conn_map, afterfn, agent.clone())
                            .await;
                    }
                    __tokio_select_util::Out::_1(e) => match e {
                        Some(e) => return Err(e),
                        None => {
                            //println!("server exit channel err {}",e.to_string());
                            return Err(NetError::ShutdownServer("".to_string()));
                        }
                    },

                    __tokio_select_util::Out::_2(now) => {
                        let mut afterfn = afterfn.lock().await;
                        afterfn.check_delete(now);
                        if let Some((addr, id)) = afterfn.get_taskaddr_from_time(now) {
                            if let Some(react_tx) = conn_map.lock().await.get_mut(&addr) {
                                let _ =react_tx.try_send(ReactOperationChannel {
                                    op: ReactOperation::Afterfn(id),
                                    res: None,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    pub async fn start_conn_handle<A>(
        &mut self,
        stream: TcpStream,
        addr: SocketAddr,
        conn_map: Arc<Mutex<HashMap<SocketAddr, Sender<conn::ReactOperationChannel>>>>,
        after_fn: Arc<Mutex<AfterFn<A>>>,
        mut agent: A,
    ) where
        A: Agent + Send + 'static,
    {
        let read_timeout = self.read_timeout;

        let max_package_size = self.max_package_size;
        //let (write_tx, mut write_rx) = async_channel::bounded::<OperationChannel>(2048);

        let (react_tx, react_rx) = tokio::sync::mpsc::channel::<ReactOperationChannel>(99);
        let mut conn = TcpConn::<A>::new(
            addr,
            react_tx.clone(),
            after_fn,
            read_timeout,
            stream,
            max_package_size,
        );

        let mut wg = AsyncWaitGroup::new();

        //启动1read

        //let readwg = wg.add(1);
        let _react_tx = react_tx.clone();
        conn_map.lock().await.insert(addr, _react_tx);

        //启动react
        let reactwg = wg.add(1);
        tokio::spawn(async move {
            let reason = match handler_reactreadwrite(&mut conn, react_rx, &mut agent).await {
                Err(e)=>{
                let _ = conn.write_flush().await;
                    Some(e.to_string())
                }
                Ok(resion)=>resion
            };
            #[cfg(debug_assertions)]
            println!("react 退出于 {:?}", reason);

            let _ = agent.on_closed(&mut conn,reason).await;
            /*while let Ok(Some(r)) = react_rx.try_recv() {
                if let Some(res) = r.res {
                    res.try_send(ReactOperationResult::Exit);
                }
            }*/
            //_write_tx.try_send(OperationChannel {
            //     op: Operation::Exit,
            //    res: None,
            //});
            reactwg.done().await;
        });

        tokio::spawn(async move {
            wg.wait().await;
            conn_map.lock().await.remove(&addr);
        });
        /*tokio::spawn(async move {
            if let Err(NetError::ShutdownServer(e)) =
                eventhandler.on_closed(&mut ctx, reason).await
            {
                if let Err(_) = conn_exit1.send(NetError::ShutdownServer(e.clone())).await {
                    println!("Server 关闭于错误 {:?}", e)
                }
            };

            conn_exit_tx.try_send(());
        });*/
        //return connwrite;
    }
}
