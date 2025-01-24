use crate::error::HttpError;
use crate::http1::ContentLen;

use net::conn::TcpConn;
const CONTENT_TYPE: &str = "Content-Type";

pub enum ResponseBody {
    Empty,
    Vec(Vec<u8>),
    Body(Box<dyn Body>),
}
impl ResponseBody {
    fn len(&self) -> Result<usize> {
        match self {
            ResponseBody::Empty => Ok(0),
            ResponseBody::Vec(b) => Ok(b.len()),
            ResponseBody::Body(b) => b.len(),
        }
    }
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        match self {
            ResponseBody::Empty => Err(HttpError::IoError("ResponseBody is empty".into()).into()),
            ResponseBody::Vec(b) => {
                if buf.len() >= b.len() {
                    buf[..b.len()].copy_from_slice(b);
                    let len = b.len();
                    b.clear();
                    return Ok(len);
                } else {
                    let s = b.split_off(buf.len());
                    buf.copy_from_slice(b);
                    *self = ResponseBody::Vec(s);
                    return Ok(buf.len());
                }
            }
            ResponseBody::Body(b) => b.read(buf),
        }
    }
}
pub trait Body: Send {
    fn len(&self) -> Result<usize>;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
}
#[allow(dead_code)]
pub struct Response {
    //pub(crate) method: Method,
    //pub(crate) host: String,
    //pub(crate) path: String,
    //pub(crate) uri: String,
    pub(crate) content_length: ContentLen,
    pub(crate) code: u16,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) is_nocache: bool,
    body: ResponseBody,
}
use crate::Result;
use net::err::NetError;
use std::collections::HashMap;
use std::{fmt, usize};

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut f = f.debug_struct("Request");
        //f.field("uri", &self.uri);
        if self.code > 0 {
            f.field("code", &self.code);
        }
        f.field("content_length", &self.content_length);
        f.field("headers", &self.headers);
        //f.field("host", &self.host);
        //f.field("path", &self.path);
        //f.field("method", &self.method);
        f.finish()
    }
}
#[allow(dead_code)]
impl Response {
    pub(crate) fn new() -> Self {
        Self {
            content_length: ContentLen::None,
            code: 0,
            headers: Default::default(),
            is_nocache: false,
            body: ResponseBody::Empty,
        }
    }

    pub async fn read_by_maxsize(&mut self, _maxsize: usize) -> Result<Vec<u8>> {
        /*let res = match self.body_length {
            ContentType::Length(size) if size<=maxsize => {
                self.stream.next(size).await?
            },
            ContentType::Chunked => {
                let mut res=Vec::new();
                loop {
                    let buf =  self.stream.readline().await?;
                    let len = u32::from_str_radix(String::from_utf8(buf.clone())?.as_str(), 16)?;
                    if len == 0 {
                        self.stream.readline().await?;
                        break
                    } else {
                        res.append(&mut self.stream.next(len as usize).await?);
                        self.stream.shift(2).await?;
                    }
                }
                res
            }
            _ => {
                return Err(HttpError::Custom("err content_length".to_string()));
            }
        };

        match self.herder.get("content-encoding") {
            Some(code)=>{
                match code.as_str() {
                    "gzip"=>{
                        let d = GzDecoder::new(&res[..]);
                        let mut r = io::BufReader::new(d);
                        let mut buffer = Vec::new();

                        // read the whole file
                        r.read_to_end(&mut buffer)?;
                        return Ok(buffer)
                    },
                    s=>{
                        return Err(HttpError::Custom(format!("未支持的 content-encoding {}",s)))
                    }
                }
            },
            None=> return Ok(res)
        }*/
        Err(HttpError::UknowError.into())
    }
    pub async fn writestring(&mut self, _str: &str) -> Result<()> {
        /*let r=self.stream;
        r.out.Reset()
        if r.outCode != 0 && httpCode(r.outCode).Bytes() != nil {
            r.out.Write(httpCode(r.outCode).Bytes())
        } else {
            r.out.Write(http1head200)
        }
        r.out.Write(http1nocache)
        if r.outContentType != "" {
            r.out.WriteString("content-Type: ")
            r.out.WriteString(r.outContentType)
            r.out.WriteString("\r\n")
        } else {
            r.out.Write([]byte("content-Type: text/html;charset=utf-8\r\n"))
        }
        r.out1.Reset()
        if len(b) > 9192 && strings.Contains(r.Header("accept-encoding"), "deflate") {
            w := CompressNoContextTakeover(r.out1, 6)
            w.Write(b)
            w.Close()
            r.out.Write(http1deflate)
        } else {
            r.out1.Write(b)
        }
        r.data = r.out1
        r.dataSize = r.out1.Len()
        r.Status = requestStatusNormalWrite
        r.finish()*/
        Ok(())
    }
    pub fn ok(body: impl Into<String>) -> Result<Self> {
        let data = body.into().into_bytes();
        Ok(Self {
            content_length: ContentLen::Length(data.len().clone()),
            code: 200,
            headers: Default::default(),
            is_nocache: false,
            body: ResponseBody::Vec(data),
        })
    }
    pub fn from_body(body: Box<dyn Body>) -> Result<Self> {
        Ok(Self {
            content_length: ContentLen::Length(body.len()?),
            code: 200,
            headers: Default::default(),
            is_nocache: false,
            body: ResponseBody::Body(body),
        })
    }
    pub fn from_html(html: impl AsRef<str>) -> Result<Self> {
        let mut headers = HashMap::new();
        headers.insert(CONTENT_TYPE.into(), "text/html".into());
        let data = html.as_ref().as_bytes().to_vec();
        Ok(Self {
            headers,
            code: 200,
            content_length: ContentLen::Length(data.len().clone()),
            body: ResponseBody::Vec(data),
            is_nocache: false,
        })
    }
    pub fn error(msg: impl Into<String>, status: u16) -> Result<Self> {
        let data = msg.into().into_bytes();
        let mut headers = HashMap::new();
        headers.insert(CONTENT_TYPE.into(), "text/html; charset=UTF-8".into());
        Ok(Self {
            content_length: ContentLen::Length(data.len().clone()),
            body: ResponseBody::Vec(data),
            code: status,
            headers: headers,
            is_nocache: false,
        })
    }
    pub fn byte(data: impl Into<Vec<u8>>) -> Result<Self> {
        let data = data.into();
        Ok(Self {
            content_length: ContentLen::Length(data.len().clone()),
            body: ResponseBody::Vec(data),
            code: 200,
            headers: Default::default(),
            is_nocache: false,
        })
    }
    pub(crate) async fn write<T>(mut self, conn: &mut TcpConn<T>, keep_alive: bool) -> Result<()> {
        if self.code == 0 {
            conn.buffer_write(crate::http1::http1code(200)?).await?;
        } else {
            conn.buffer_write(crate::http1::http1code(self.code)?)
                .await?;
        }

        if self.is_nocache {
            conn.buffer_write(crate::http1::HTTP1NOCACHE).await?;
        }

        let size = self.body.len()?;

        for (k, v) in &self.headers {
            conn.buffer_write(urlencoding::encode(k).as_bytes()).await?;
            conn.buffer_write(b": ").await?;
            conn.buffer_write(v.as_bytes()).await?;
            conn.buffer_write(b"\r\n").await?;
        }
        // for (k, v) in &self.out_cookie {
        //     req.out.write_string("Set-Cookie: ");
        //     req.out.write_string(&urlencoding::encode(&k));
        //     req.out.write_string("=");
        //     req.out.write_string(&urlencoding::encode(&v.value));
        //     if v.max_age > 0 {
        //         req.out.write_string("; Max-age=");
        //         req.out.write_string(v.max_age.to_string().as_str());
        //     }
        //     req.out.write_string("; path=/\r\n");
        // }
        if keep_alive {
            conn.buffer_write(b"connection: keep-alive").await?;
        } else {
            conn.buffer_write(b"connection: close").await?;
        }
        conn.buffer_write(b"\r\nContent-Length: ").await?;
        conn.buffer_write(&size.to_string().as_bytes()).await?;
        if size > 0 {
            conn.buffer_write(b"\r\n\r\n").await?;
            let mut data = [0; 163840];
            while self.body.len()? > 0 {
                let n = self.body.read(&mut data)?;
                conn.buffer_write(&data[..n]).await?;
            }
        } else {
            conn.buffer_write(b"\r\n\r\n").await?;
        }
        conn.write_flush().await?;
        Ok(())
    }
    pub fn redirect_with_status(url: impl Into<String>, status_code: u16) -> Result<Self> {
        if !(300..=399).contains(&status_code) {
            return Err(HttpError::Internal(
                "redirect status codes must be in the 300-399 range! Please checkout https://developer.mozilla.org/en-US/docs/Web/HTTP/Status#redirection_messages for more".into(),
            ).into());
        }
        let mut headers = HashMap::new();
        headers.insert("Location".into(), url.into());
        Ok(Self {
            content_length: ContentLen::None,
            body: ResponseBody::Empty,
            code: status_code,
            headers: headers,
            is_nocache: false,
        })
    }
}
