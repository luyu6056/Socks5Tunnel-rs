use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use tokio::sync::mpsc::{*};

///AsyncWaitGroup channel实现

pub struct AsyncWaitGroup{
    work_num: Arc<AtomicIsize>,
    wait_tx:Sender<()>,
    wait_rx:Receiver<()>,
}
pub struct AsyncWaitGroupCopy{
    wait_tx:Sender<()>,
}
impl AsyncWaitGroup{
    pub fn new()->Self{
        let (tx,rx)=channel(100);
        AsyncWaitGroup{
            work_num:Arc::new(AtomicIsize::new(0)),
            wait_tx:tx,
            wait_rx:rx,
        }
    }
    pub fn add(&self,num:usize)->AsyncWaitGroupCopy{
        self.work_num.fetch_add(num as isize,Ordering::Relaxed);
        AsyncWaitGroupCopy{
            wait_tx: self.wait_tx.clone(),
        }
    }
    pub async fn done(&self){
        let _=self.wait_tx.send(());
    }
    pub async fn wait(&mut self){
        if self.work_num.load(Ordering::Relaxed) > 0 {
            while let Some(_) = self.wait_rx.recv().await{
                if self.work_num.fetch_add(-1,Ordering::Relaxed)==0{
                    return;
                };
            }
        }
    }
}
impl AsyncWaitGroupCopy{
    pub async fn done(&self){
        let _=self.wait_tx.send(()).await;
    }
}