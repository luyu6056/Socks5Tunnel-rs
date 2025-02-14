use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std:: fmt;
#[cfg(debug_assertions)]
use std::backtrace;

//封装一个带debug的
use tokio::sync::Mutex as TokioMutex;

#[allow(dead_code)]
#[derive(Default)]
pub struct Mutex<T: ?Sized> {
    call_num: Arc<AtomicI64>,
    call_line: Arc<TokioMutex<Option<String>>>,
    inner: TokioMutex<T>,
}

pub struct MutexGuard<'a, T: ?Sized + 'a> {
    call_num: Arc<AtomicI64>,
    inner: tokio::sync::MutexGuard<'a, T>,
}
impl<T> Mutex<T> {
    pub fn new(data: T) -> Self {
        Mutex {
            call_num: Default::default(),
            call_line: Arc::new(TokioMutex::new(None)),
            inner: TokioMutex::new(data),
        }
    }
    pub async fn lock(&self) -> MutexGuard<'_, T> {
        let _num = self
            .call_num
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(debug_assertions)]{
            let c = backtrace::Backtrace::capture();
            if c.frames().len() > 4 {
                if _num > 0 {
                    if let Some(str) = self.call_line.lock().await.as_ref() {
                        println!(
                            "The current thread wants to acquire the lock, but it is already held by: {:?}, Holder: {:?}",
                            c.frames()[3],
                            str
                        );
                    }
                }
                self.call_line
                    .lock()
                    .await
                    .replace(format!("{:?}", c.frames()[3]));
            }
        }

        let m = self.inner.lock().await;
        MutexGuard {
            call_num: self.call_num.clone(),
            inner: m,
        }
    }
}
impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.call_num
            .fetch_add(-1, std::sync::atomic::Ordering::Relaxed);
    }
}
impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
         self.inner.deref()
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}
