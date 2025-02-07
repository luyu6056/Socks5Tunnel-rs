//封装一个buffer接口，api来自我的go项目

pub(crate) const DEFAULT_RE_SIZE_LEN: usize = 1024 * 64; //清理失效数据阈值
pub(crate) const DEFAULT_BUF_SIZE: usize = 1024 * 8; //初始化大小

use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::fmt::Debug;
use std::marker::PhantomPinned;
use std::ops::{Index, Range, RangeFrom, RangeTo};

#[derive(Debug)]
pub struct Buffer<T> {
    b: T,
    max_buf_size: usize,
    re_size_len: usize,
    pos: usize, //起点位置
    l: usize,   //终点,长度以b.len()替代
    _pin: PhantomPinned,
}
//重新整理内存
pub trait ReSize {
    fn re_size(&mut self, start: usize, end: usize, re_size_len: usize);
    fn grow_to_with_max(&mut self, new_len: usize, max_len: usize) -> Result<(), Box<dyn Error>> {
        if new_len > max_len {
            return Err(format!("buf长度{}超过最大长度{}", new_len, max_len).into());
        }
        self._grow_to(new_len);
        Ok(())
    }
    fn bytes(&self) -> &[u8];
    fn as_mut_slice(&mut self) -> &mut [u8];
    fn len(&self) -> usize {
        self.bytes().len()
    }
    //不要直接调用此接口
    fn _grow_to(&mut self, new_len: usize);
}

impl ReSize for Vec<u8> {
    fn re_size(&mut self, start: usize, end: usize, re_size_len: usize) {
        unsafe {
            let len = end - start; //原始长度
            let newlen = if len > re_size_len { len } else { re_size_len };
            self.copy_within(start..end, 0);
            self.set_len(newlen);
            self.shrink_to(newlen);
            #[cfg(test)]
            println!("re_size ,capacity:{}", self.capacity());
        }
    }

    fn bytes(&self) -> &[u8] {
        self.as_slice()
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }

    fn _grow_to(&mut self, new_len: usize) {
        unsafe {
            self.reserve(new_len);
            self.set_len(new_len);
            #[cfg(test)]
            println!("new_len:{:?},capacity:{:?}", new_len, self.capacity());
        }
    }
}
impl ReSize for [u8; DEFAULT_BUF_SIZE] {
    fn re_size(&mut self, start: usize, end: usize, _re_size_len: usize) {
        //自我拷贝
        self.copy_within(start..end, 0);
    }

    fn bytes(&self) -> &[u8] {
        self.as_slice()
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
    fn _grow_to(&mut self, n: usize) {
        //定长无法grow
        assert!(n > DEFAULT_BUF_SIZE);
    }
}
pub fn new_u8() -> Buffer<[u8; DEFAULT_BUF_SIZE]> {
    let buf = Buffer {
        b: [0; DEFAULT_BUF_SIZE],
        max_buf_size: DEFAULT_BUF_SIZE,
        re_size_len: DEFAULT_BUF_SIZE / 4,
        pos: 0,
        l: 0,
        _pin: PhantomPinned,
    };
    buf
}
pub fn new_vec() -> Buffer<Vec<u8>> {
    let v: Vec<u8> = vec![0; DEFAULT_BUF_SIZE];
    let buf = Buffer {
        b: v,
        max_buf_size: usize::MAX,
        re_size_len: DEFAULT_RE_SIZE_LEN,
        pos: 0,
        l: 0,
        _pin: PhantomPinned,
    };
    buf
}
#[allow(dead_code)]
impl<T: ReSize> Buffer<T> {
    fn re_size_len(&mut self, new: usize) {
        self.re_size_len = new;
    }
    fn re_size(&mut self) {
        //自我拷贝，pos归零
        self.b.re_size(self.pos, self.l, self.re_size_len);
        self.l -= self.pos;
        self.pos = 0;
    }

    pub fn reset(&mut self) {
        if self.pos >= self.re_size_len {
            #[cfg(test)]
            println!("reset re_size");
            self.re_size();
        }
        self.pos = 0;
        self.l = 0;
    }

    pub fn make(&mut self, l: usize) -> Result<&mut [u8], Box<dyn Error>> {
        if self.pos >= self.re_size_len {
            self.re_size();
        }
        self.l += l;
        if self.l > self.b.len() {
            self.b.grow_to_with_max(self.l, self.max_buf_size)?;
        }
        let start = self.l - l;
        let end = self.l;
        Ok(&mut self.b.as_mut_slice()[start..end])
    }
    pub fn spare(&mut self, l: usize) -> Result<&mut [u8], Box<dyn Error>> {
        if self.pos >= self.re_size_len {
            self.re_size();
        }
        let newlen = self.l + l;

        if newlen > self.b.len() {
            self.b.grow_to_with_max(newlen, self.max_buf_size)?;
        }
        let start = self.l;
        Ok(&mut self.b.as_mut_slice()[start..newlen])
    }
    pub fn string(&self) -> Cow<str> {
        String::from_utf8_lossy(self.b.bytes())
    }

    pub fn write(&mut self, b: &[u8]) -> Result<(), Box<dyn Error>> {
        if self.pos >= self.re_size_len {
            self.re_size()
        }

        self.l += b.len();

        unsafe {
            if self.l > self.b.len() {
                #[cfg(test)]
                println!("grow_to_with_max");

                self.b.grow_to_with_max(self.l, self.max_buf_size)?;
            }
            let start = self.l - b.len();
            std::ptr::copy(
                b.as_ptr(),
                self.b.as_mut_slice()[start..].as_mut_ptr(),
                b.len(),
            );
        }
        Ok(())
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
            if self.pos >= self.re_size_len {
                self.re_size();
            }
            //println!("{}",self.pos);
        } else {
            self.reset();
        }
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.b.bytes()[self.pos..self.l]
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.b.as_mut_slice()[self.pos..self.l]
    }
    pub fn bytes(&self) -> &[u8] {
        &self.b.bytes()[self.pos..self.l]
    }

    pub fn write_string(&mut self, s: &str) -> Result<(), Box<dyn Error>> {
        self.write(s.as_bytes())
    }
    pub fn write_byte(&mut self, s: u8) -> Result<(), Box<dyn Error>> {
        self.write(&[s])
    }
    pub fn next_n(&mut self, n: usize) -> Option<&[u8]> {
        let data = if self.pos + n > self.l {
            return None;
        } else {
            &self.b.bytes()[self.pos..self.pos + n]
        };
        self.pos += n;
        Some(data)
    }
    pub fn truncate(&mut self, len: usize) {
        let new = self.pos + len;
        if new <= self.b.len() {
            self.l = new;
        } else {
            self.l = self.b.len()
        }
    }
    pub fn read_byte(&mut self) -> Option<u8> {
        match self.next_n(1) {
            None => None,
            Some(data) => Some(data[0]),
        }
    }
}
impl From<Vec<u8>> for MsgBuffer {
    fn from(value: Vec<u8>) -> Self {
        let l = value.len();
        MsgBuffer(Buffer {
            b: value,
            max_buf_size: usize::MAX,
            re_size_len: DEFAULT_RE_SIZE_LEN,
            pos: 0,
            l: l,
            _pin: Default::default(),
        })
    }
}

impl<T: ReSize> Iterator for Buffer<T> {
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
        Some(self.b.bytes()[self.pos - 1])
    }
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        if self.pos + n > self.l {
            self.reset();
            return None;
        };
        self.pos += n + 1;
        Some(self.b.bytes()[self.pos - 1])
    }
}
impl<T: ReSize> Index<usize> for Buffer<T> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.b.bytes()[self.pos + index]
    }
}

impl<T: ReSize> Index<Range<usize>> for Buffer<T> {
    type Output = [u8];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        &self.b.bytes()[self.pos + index.start..self.pos + index.end]
    }
}
impl<T: ReSize> Index<RangeFrom<usize>> for Buffer<T> {
    type Output = [u8];

    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        &self.b.bytes()[self.pos + index.start..self.l]
    }
}
impl<T: ReSize> Index<RangeTo<usize>> for Buffer<T> {
    type Output = [u8];

    fn index(&self, index: RangeTo<usize>) -> &Self::Output {
        &self.b.bytes()[self.pos..self.pos + index.end]
    }
}
impl std::io::Write for MsgBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .write(buf)
            .or(Err(std::io::ErrorKind::FileTooLarge))?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub struct MsgBuffer(pub Buffer<Vec<u8>>);
impl Default for MsgBuffer {
    fn default() -> Self {
        Self::new()
    }
}
impl Debug for MsgBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bytes:{:?}", self.bytes())
    }
}
impl MsgBuffer {
    pub fn new() -> MsgBuffer {
        MsgBuffer(new_vec())
    }
    pub fn reset(&mut self) {
        self.0.reset();
    }

    pub fn make(&mut self, l: usize) -> Result<&mut [u8], Box<dyn Error>> {
        self.0.make(l)
    }
    pub fn spare(&mut self, l: usize) -> Result<&mut [u8], Box<dyn Error>> {
        self.0.spare(l)
    }
    pub fn string(&self) -> Cow<str> {
        self.0.string()
    }

    pub fn write(&mut self, b: &[u8]) -> Result<(), Box<dyn Error>> {
        self.0.write(b)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn shift(&mut self, len: usize) {
        self.0.shift(len)
    }
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.0.as_mut_slice()
    }
    pub fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }

    pub fn write_string(&mut self, s: &str) -> Result<(), Box<dyn Error>> {
        self.0.write_string(s)
    }
    pub fn write_byte(&mut self, s: u8) -> Result<(), Box<dyn Error>> {
        self.0.write_byte(s)
    }
    pub fn next_n(&mut self, n: usize) -> Option<&[u8]> {
        self.0.next_n(n)
    }
    pub fn truncate(&mut self, len: usize) {
        self.0.truncate(len)
    }
    pub fn read_byte(&mut self) -> Option<u8> {
        self.0.read_byte()
    }
}
#[cfg(test)]
fn new_vec_size(size: usize) -> Buffer<Vec<u8>> {
    let v: Vec<u8> = vec![0; size];
    let buf = Buffer {
        b: v,
        max_buf_size: usize::MAX,
        re_size_len: DEFAULT_RE_SIZE_LEN,
        pos: 0,
        l: 0,
        _pin: PhantomPinned,
    };
    buf
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msgbuffer() {
        let mut msg = new_vec_size(5);
        //let mut msg = new_u8();
        let b = msg.make(1).expect("make error");
        assert_eq!(1, b.len());
        let b = msg.make(8).expect("make error");
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
        ])
        .expect("write error");
        msg.re_size_len(30);
        println!("pos {}", msg.pos);
        msg.shift(30);
        assert_eq!(0, msg.pos);
        assert_eq!(&[31], msg.bytes());
        msg.write(&[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31,
        ])
        .expect("write error");
        msg.truncate(2);
        println!("{:?}", msg.bytes());
        msg.write_byte(1).expect("write error");
        msg.write_byte(2).expect("write error");
        assert_eq!(&[31, 1, 1, 2], msg.as_mut_slice());
        msg.truncate(4);
        assert_eq!(&[31, 1, 1, 2], msg.as_slice());
        msg.write_string("123").expect("write error");
        assert_eq!(&[31, 1, 1, 2, 49, 50, 51], msg.as_slice());
        let b = msg.next_n(4).expect("next_n error");
        assert_eq!(&[31, 1, 1, 2], b);
        assert_eq!(&[49, 50, 51], msg.as_slice());
    }
}
