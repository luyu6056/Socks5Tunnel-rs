use crate::err::NetError;
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::ops::Add;
use std::pin::Pin;
use std::time;
use std::time::Duration;
use tokio::time::Instant;
pub type ConnAsyncFn<T> =
    Box<dyn for<'a> FnOnce(&'a mut T) -> ConnAsyncResult<'a> + Send + Sync + 'static>;
pub type ConnAsyncResult<'a> = Pin<Box<dyn Future<Output = Result<(), NetError>> + Send + 'a>>;

pub struct AfterFn<T: ?Sized> {
    no: u64,
    task_time_id: BTreeMap<Instant, u64>, //待处理的task，时间=>id
    pub task_id_fn: BTreeMap<u64, (SocketAddr, ConnAsyncFn<T>)>, //待处理task的 id=>
    task_wait_delete: BTreeMap<Instant, u64>, //如果没有conn把 task_id_fn 取出来，就需要定时删除以释放内存
    timeout: time::Duration,                  //任务激活后，超时未处理就会删除踢出队列的超时
}
impl<T> fmt::Debug for AfterFn<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AfterFn").finish()
    }
}
impl<T> AfterFn<T> {
    pub fn new() -> Self {
        AfterFn {
            no: 0,
            task_time_id: Default::default(),
            task_id_fn: Default::default(),
            task_wait_delete: Default::default(),
            timeout: Duration::from_secs(60),
        }
    }
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn add_fn<F>(&mut self, addr: SocketAddr, delay: time::Duration, f: F)
    where
        F: for<'b> FnOnce(&'b mut T) -> ConnAsyncResult<'b> + Send + Sync + 'static,
    {
        self.no += 1;
        let id = self.no;
        self.task_time_id.insert(Instant::now().add(delay), id);
        self.task_id_fn.insert(id, (addr, Box::new(f)));
    }
    pub fn get_taskaddr_from_time(&mut self, now: Instant) -> Option<(SocketAddr, u64)> {
        if let Some((k, _)) = &self.task_time_id.first_key_value() {
            if now > **k {
                if let Some((_, id)) = self.task_time_id.pop_first() {
                    self.task_wait_delete
                        .insert(Instant::now().add(self.timeout), id);
                    if let Some((addr, _)) = self.task_id_fn.get(&id) {
                        return Some((*addr, id));
                    }
                }
            }
        }
        None
    }
    pub fn check_delete(&mut self, now: Instant) {
        if let Some((k, _)) = &self.task_wait_delete.first_key_value() {
            if now > **k {
                if let Some((_, id)) = self.task_wait_delete.pop_first() {
                    self.task_id_fn.remove(&id);
                }
            }
        }
    }
    pub fn remove_task(&mut self, id: u64) -> Option<ConnAsyncFn<T>> {
        if let Some((_, f)) = self.task_id_fn.remove(&id) {
            return Some(f);
        }
        None
    }
}
