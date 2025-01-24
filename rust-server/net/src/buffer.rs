//封装一个buffer接口，api来自我的go项目

#[cfg(test)]
const TEST_MAX_BUF_SIZE: usize = 1 * 16; //清理失效数据阈值
#[cfg(test)]
const TEST_DEFAULT_BUF_SIZE: usize = 1 * 8; //初始化大小

pub(crate) const MAX_BUF_SIZE: usize = 1024 * 64; //清理失效数据阈值
pub(crate) const DEFAULT_BUF_SIZE: usize = 1024 * 8; //初始化大小

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomPinned;
use std::ops::{Index, Range, RangeFrom, RangeTo};
use std::pin::Pin;




/*impl MsgBufferPoolStatic {
    fn new()->Arc<Pin<Box<MsgBufferPoolStatic>>>{
        let mut pool=MsgBufferPoolStatic{
            vec_pool:  Mutex::new(vec![]),
            max_buf_num: 64,
            max_buf_size: MAX_BUF_SIZE,
            default_buf_size: DEFAULT_BUF_SIZE,
            status: Mutex::new(vec![]),
            index: Default::default(),
            out_pool: Default::default(),
            out_pool_index: Default::default(),
            _pin:PhantomPinned,
        };
        for i in 0..64{
            pool.vec_pool.lock().unwrap().insert(i,Box::pin(MsgBuffer::new()));
            pool.status.lock().unwrap().insert(i,false);
        }
        Arc::new(Box::pin(pool))
    }
    //只能长不能缩
    pub fn set_max_buf_num(&mut self,size:usize){
        if size> self.max_buf_num {
            for i in self.max_buf_num..size{
                self.vec_pool.lock().unwrap().insert(i,Box::pin(MsgBuffer::new()));
                self.status.lock().unwrap().insert(i,false);
            }
            self.max_buf_num=size;

        }
    }
    pub fn set_max_buf_size(&mut self,size:usize){
        self.max_buf_size=size;

    }
    pub fn set_default_buf_size(&mut self,size:usize){
        self.default_buf_size=size;

    }
    pub fn get(&self) -> MsgBufferStatic {
        unsafe {
            for _ in 0..self.max_buf_num{
                let i=self.index.load(Ordering::Relaxed);
                let i=i%self.max_buf_num;
                if !self.status.lock().unwrap().get(i).unwrap(){
                    let mut buf=MsgBufferStatic{
                        ptr:self.vec_pool.lock().unwrap().get_mut(i).unwrap().as_mut().get_unchecked_mut(),
                        index:MsgBufferStaticIndex::Index(i),
                    };

                    *self.status.lock().unwrap().get_mut(i).unwrap()=true;
                    unsafe {
                        (*buf.inner).max_buf_size=self.max_buf_num;
                        (*buf.inner).default_buf_size=self.default_buf_size;
                    }
                    return buf
                }
                self.index.fetch_add(1,Ordering::Relaxed);
            }
            let mut index=self.out_pool_index.load(Ordering::Relaxed);
            while let Some(_) =self.out_pool.lock().unwrap().get(&index) {
                index=self.out_pool_index.fetch_add(1,Ordering::Relaxed);
            }
            let buf=MsgBuffer::new_with_param(self.default_buf_size,self.max_buf_size);
            self.out_pool.lock().unwrap().insert(index,Box::pin(buf));
            let mut buf=MsgBufferStatic{
                inner:self.out_pool.lock().unwrap().get_mut(&index).unwrap().as_mut().get_unchecked_mut() as *mut MsgBuffer,
                index:MsgBufferStaticIndex::OutPoolIndex(index),
            };
            unsafe {
                (*buf.inner).max_buf_size=self.max_buf_num;
                (*buf.inner).default_buf_size=self.default_buf_size;
            }
            buf
        }
    }
    fn put(&self,buf:&mut MsgBufferStatic){
        match buf.index {
            MsgBufferStaticIndex::Index(i)=>{
                unsafe{(*buf.inner).reset();}
                *self.status.lock().unwrap().get_mut(i).unwrap()=false;
            }
            MsgBufferStaticIndex::OutPoolIndex(i)=>{
                self.out_pool.lock().unwrap().remove(&i);
            },
            MsgBufferStaticIndex::None=>{

            }
        }
    }


}*/

/*##[derive(Debug, Clone)]
pub struct MsgBuffer { //纯Vec实现
    max_buf_size: usize,
    default_buf_size: usize,
    b: Vec<u8>,
    pos: usize, //起点位置
                //l: usize,   //长度以b.len()替代
}

impl Default for MsgBuffer {
    fn default() -> Self {
        new()
    }
}
pub fn new() -> MsgBuffer {
    MsgBuffer {
        max_buf_size: MAX_BUF_SIZE,
        default_buf_size: DEFAULT_BUF_SIZE,
        b: Vec::with_capacity(DEFAULT_BUF_SIZE),
        pos: 0,
    }
}
impl MsgBuffer {

    //从一个vec创建
    pub fn new_from_vec(b: Vec<u8>,max_buf_size:usize,default_buf_size:usize) -> Self {
        MsgBuffer {
            max_buf_size: max_buf_size,
            default_buf_size: default_buf_size,
            b: b,
            pos: 0,
        }
    }

    #[cfg(test)]
    fn test_new() -> Self {
        MsgBuffer {
            max_buf_size: TEST_MAX_BUF_SIZE,
            default_buf_size: TEST_DEFAULT_BUF_SIZE,
            b: Vec::with_capacity(TEST_DEFAULT_BUF_SIZE),
            pos: 0,
        }
    }

    pub fn reset(&mut self) {
        if self.pos > self.max_buf_size {
            #[cfg(test)]
            println!("reset re_size");
            self.re_size();
        }
        self.pos = 0;
        unsafe {
            self.b.set_len(0);
        }
    }

    pub fn make(&mut self, l: usize) -> &mut [u8] {
        if self.pos > self.max_buf_size {
            #[cfg(test)]
            println!("make re_size");
            self.re_size();
        }
        let newlen = self.b.len() + l;

        if self.b.capacity() < newlen {
            self.b.reserve(l);
        }
        unsafe { self.b.set_len(newlen) }
        return &mut self.b[newlen - l..];
    }

    //重新整理内存
    fn re_size(&mut self) {
        unsafe {
            if self.pos >= self.max_buf_size {
                let oldlen = self.b.len() - self.pos; //原始长度
                let dst = self.b.as_mut_ptr();
                std::ptr::copy(self.b[self.pos..].as_ptr(), dst, self.b.len() - self.pos);
                self.b.set_len(oldlen);
                self.b.shrink_to(self.max_buf_size);
                self.pos = 0;
            }
        }
    }

    pub fn write(&mut self, b: &[u8]) {
        self.b.extend_from_slice(b);
    }

    pub fn len(&self) -> usize {
        return self.b.len() - self.pos;
    }
    pub fn shift(&mut self, len: usize) {
        if len <= 0 {
            return;
        }
        //#[cfg(test)]
        //println!("before shift,{},{},{},{}",len,self.pos,self.len(),self.b.capacity());
        if len < self.len() {
            self.pos += len;
            if self.pos > self.max_buf_size {
                #[cfg(test)]
                println!("shift re_size");
                self.re_size();
            }
        } else {
            self.reset();
        }
        //#[cfg(test)]
        //println!(" after shift,{},{},{},{}",len,self.pos,self.len(),self.b.capacity());
    }
    pub fn as_slice(&self) -> &[u8] {
        return &self.b[self.pos..];
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        return &mut self.b[self.pos..];
    }
    pub fn bytes(&self) -> &[u8] {
        return &self.b[self.pos..];
    }
    pub fn write_string(&mut self, s: &str) {
        self.write(s.as_bytes())
    }
    pub fn write_byte(&mut self, s: u8) {
        if self.pos > self.max_buf_size {
            self.re_size();
        }
        self.b.push(s);
    }
    pub fn truncate(&mut self, len: usize) {
        self.b.truncate(self.pos + len);
    }
}*/
//Box<[u8]>实现
#[derive(Debug)]
pub struct MsgBuffer {
    b: Box<[u8]>, //指向raw或者其他地方的裸指针
    max_buf_size: usize,
    //default_buf_size: usize,
    pos: usize, //起点位置
    l: usize,   //终点,长度以b.len()替代
    _pin: PhantomPinned,
}
impl Default for MsgBuffer {
    fn default() -> Self {
        MsgBuffer::new()
    }
}


impl MsgBuffer {
    pub fn new() -> MsgBuffer {
        let v: Vec<u8> = vec![0; DEFAULT_BUF_SIZE];
        let buf = MsgBuffer {
            b: v.into_boxed_slice(),
            max_buf_size: MAX_BUF_SIZE,
            //default_buf_size: DEFAULT_BUF_SIZE,
            pos: 0,
            l: 0,
            _pin: PhantomPinned,
        };
        buf
    }
    pub fn new_with_param(default_buf_size: usize, _max_buf_size: usize) -> MsgBuffer {
        let v: Vec<u8> = vec![0; default_buf_size];
        let buf = MsgBuffer {
            b: v.into_boxed_slice(),
            max_buf_size: default_buf_size,
            //default_buf_size: max_buf_size,
            pos: 0,
            l: 0,
            _pin: PhantomPinned,
        };
        buf
    }

    #[cfg(test)]
    fn test_new() -> Self {
        let v: Vec<u8> = vec![0; TEST_DEFAULT_BUF_SIZE];
        let buf = MsgBuffer {
            b: v.into_boxed_slice(),
            max_buf_size: TEST_MAX_BUF_SIZE,
            default_buf_size: TEST_DEFAULT_BUF_SIZE,
            pos: 0,
            l: 0,

            _pin: Default::default(),
        };
        buf
    }
    fn grow(&mut self, mut n: usize) {
        unsafe {
            n += self.b.len();
            let len = self.b.len() * 2;
            if len > n {
                let mut v = vec![0; len];
                std::ptr::copy(
                    self.b[self.pos..].as_ptr(),
                    v.as_mut_ptr(),
                    self.b.len() - self.pos,
                );
                self.b = v.into_boxed_slice();
            } else {
                let mut v = vec![0; n];
                std::ptr::copy(
                    self.b[self.pos..].as_ptr(),
                    v.as_mut_ptr(),
                    self.b.len() - self.pos,
                );
                self.b = v.into_boxed_slice();
            }
            self.l -= self.pos;
            self.pos = 0;
        }
    }
    pub fn reset(&mut self) {
        if self.pos > self.max_buf_size {
            #[cfg(test)]
            println!("reset re_size");
            self.re_size();
        }
        self.pos = 0;
        self.l = 0;
    }

    pub fn make(&mut self, l: usize) -> &mut [u8] {
        if self.pos > self.max_buf_size {
            self.re_size();
        }
        self.l += l;

        if self.l > self.b.len() {
            self.grow(l);
            //self.b.reserve(l);
        }

        //unsafe { self.b.set_len(newlen) }

        return &mut self.b[self.l - l..self.l];
    }
    pub fn spare(&mut self, l: usize) -> &mut [u8] {
        if self.pos > self.max_buf_size {
            self.re_size();
        }
        let newlen = self.l + l;

        if newlen > self.b.len() {
            self.grow(l);
        }
        return &mut self.b[self.l..newlen];
    }
    pub fn string(&self) -> Cow<str> {
        String::from_utf8_lossy(self.bytes())
    }
    //重新整理内存
    fn re_size(&mut self) {
        unsafe {
            if self.pos >= self.max_buf_size {
                let len = self.l - self.pos; //原始长度
                let newlen = if len > self.max_buf_size {
                    len
                } else {
                    self.max_buf_size
                };
                let mut v = vec![0; newlen];
                let dst = v.as_mut_ptr();
                std::ptr::copy(self.b[self.pos..].as_ptr(), dst, self.b.len() - self.pos);
                self.l = len;

                self.b = v.into_boxed_slice();
                self.pos = 0;
            }
        }
    }

    pub fn write(&mut self, b: &[u8]) {
        if self.pos > self.max_buf_size {
            self.re_size()
        }

        self.l += b.len();
        unsafe {
            if self.l > self.b.len() {
                self.grow(b.len());
            }

            std::ptr::copy(b.as_ptr(), self.b[self.l - b.len()..].as_mut_ptr(), b.len());
        }

        //self.b.extend_from_slice(b);
    }

    pub fn len(&self) -> usize {
        return self.l - self.pos;
    }
    pub fn shift(&mut self, len: usize) {
        if len <= 0 {
            return;
        }

        if len < self.len() {
            self.pos += len;
            if self.pos > self.max_buf_size {
                self.re_size();
            }
            //println!("{}",self.pos);
        } else {
            self.reset();
        }
    }
    pub fn as_slice(&self) -> &[u8] {
        return &self.b[self.pos..self.l];
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        return &mut self.b[self.pos..self.l];
    }
    pub fn bytes(&self) -> &[u8] {
        return &self.b[self.pos..self.l];
    }

    pub fn write_string(&mut self, s: &str) {
        self.write(s.as_bytes())
    }
    pub fn write_byte(&mut self, s: u8) {
        if self.pos > self.max_buf_size {
            self.re_size();
        }

        self.l += 1;
        if self.l > self.b.len() {
            self.grow(1)
        }

        self.b[self.l - 1] = s;
    }
    pub fn truncate(&mut self, len: usize) {
        let new = self.pos + len;
        if new <= self.b.len() {
            self.l = new;
        } else {
            self.l = self.b.len()
        }
    }
}
impl From<Vec<u8>> for MsgBuffer {
    fn from(value: Vec<u8>) -> Self {
        let l = value.len();
        MsgBuffer {
            b: value.into_boxed_slice(),
            max_buf_size: MAX_BUF_SIZE,
            pos: 0,
            l: l,
            _pin: Default::default(),
        }
    }
}
pub struct MsgBufferPool {
    vec_pool: Vec<Pin<Box<MsgBuffer>>>,
    max_buf_num: usize,
    max_buf_size: usize,
    default_buf_size: usize,
}

impl MsgBufferPool {
    pub fn new() -> Self {
        MsgBufferPool {
            vec_pool: vec![],
            max_buf_num: 64,
            max_buf_size: MAX_BUF_SIZE,
            default_buf_size: DEFAULT_BUF_SIZE,
        }
    }
    pub fn with_max_buf_num(mut self, size: usize) -> Self {
        self.max_buf_num = size;
        self
    }
    pub fn with_max_buf_size(mut self, size: usize) -> Self {
        self.max_buf_size = size;
        self
    }
    pub fn with_default_buf_size(mut self, size: usize) -> Self {
        self.default_buf_size = size;
        self
    }
    pub fn get(&mut self) -> Pin<Box<MsgBuffer>> {
        match self.vec_pool.pop() {
            Some(inner) => inner,
            None => {
                let buf = MsgBuffer::new_with_param(self.default_buf_size, self.max_buf_size);
                Box::pin(buf)
            }
        }
    }
    pub fn put(&mut self, inner: Pin<Box<MsgBuffer>>) {
        if self.vec_pool.len() < self.max_buf_num {
            self.vec_pool.push(inner)
        }
    }
}
impl Iterator for MsgBuffer {
    // we will be counting with usize
    type Item = u8;

    // next() is the only required method
    fn next(&mut self) -> Option<Self::Item> {
        // Increment our count. This is why we started at zero.
        if self.pos == self.l {
            self.reset();
            return None;
        };
        self.pos += 1;
        Some(self.b[self.pos - 1])
    }
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        if self.pos + n > self.l {
            self.reset();
            return None;
        };
        self.pos += n + 1;
        Some(self.b[self.pos - 1])
    }
}
impl Index<usize> for MsgBuffer {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.b[self.pos+index]
    }
}

impl Index<Range<usize>> for MsgBuffer {
    type Output = [u8];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        &self.b[self.pos+index.start..self.pos+index.end]
    }
}
impl Index<RangeFrom<usize>> for MsgBuffer {
    type Output = [u8];

    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        &self.b[self.pos+index.start..self.l]
    }
}
impl Index<RangeTo<usize>> for MsgBuffer {
    type Output = [u8];

    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        &self.b[self.pos..self.pos + index.end]
    }
}
impl std::io::Write for MsgBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msgbuffer() {
        let mut msg = MsgBuffer::test_new();

        let b = msg.make(1);
        assert_eq!(1, b.len());
        let b = msg.make(8);
        b[7] = 0;
        assert_eq!(8, b.len());
        assert_eq!(9, msg.len());
        msg.shift(1);
        assert_eq!(1, msg.pos);
        assert_eq!(8, msg.len());
        msg.shift(7);
        msg.write(&[1, 2, 3]);
        assert_eq!(8, msg.pos);
        assert_eq!(&[0, 1, 2, 3], msg.bytes());
        msg.shift(5);
        assert_eq!(0, msg.pos);
        assert_eq!(msg.bytes(), &[]);
        msg.write(&[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31,
        ]);
        msg.shift(30);
        assert_eq!(0, msg.pos);
        assert_eq!(&[31], msg.bytes());
        msg.write(&[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31,
        ]);
        msg.truncate(2);
        println!("{:?}", msg.bytes());
        msg.write_byte(1);
        msg.write_byte(2);
        assert_eq!(&[31, 1, 1, 2], msg.as_mut_slice());
        msg.truncate(4);
        assert_eq!(&[31, 1, 1, 2], msg.as_slice());
        msg.write_string("123");
        assert_eq!(&[31, 1, 1, 2, 49, 50, 51], msg.as_slice());
    }
}
