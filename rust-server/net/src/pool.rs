//pub type PoolNewFn<T> = Box<dyn FnMut() -> PoolNewResult<T> + Send + Sync>;
//pub type PoolNewResult<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub struct Pool<T> {
    vec_pool: Vec<T>,
    max_buf_num: usize, //最大容量
}
pub fn new<T: Default>() -> Pool<T> {
    let pool = Pool {
        vec_pool: Vec::new(),
        max_buf_num: 4096,
    };
    pool
}
impl<T: Default> Pool<T> {
    pub fn with_max_buf_num(mut self, size: usize) -> Self {
        self.max_buf_num = size;
        self
    }
    pub fn get(&mut self) -> T {
        match self.vec_pool.pop() {
            Some(inner) => inner,
            None => T::default(),
        }
    }
    pub fn put(&mut self, inner: T) {
        if self.vec_pool.len() < self.max_buf_num {
            self.vec_pool.push(inner)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_test() {
        do_test()
    }

    fn do_test() {
        let mut pool = new::<i32>();
        let i = pool.get();
        pool.put(i);
    }
}
