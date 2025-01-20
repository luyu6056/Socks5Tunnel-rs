use crate::error::HttpError;
use crate::response::Response;
use base64::{engine::general_purpose, Engine as _};

use net::buffer::MsgBuffer;
use net::conn::TcpConn;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use sha1::{Digest, Sha1};
use std::borrow::{Cow, ToOwned};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

pub(crate) const HTTP1NOCACHE:&[u8] = b"Cache-Control: no-store, no-cache, must-revalidate, max-age=0, s-maxage=0\r\npragma: no-cache\r\n";

#[dynamic]
pub static DEFAULT_STRING: String = "".to_owned();

pub static USER_AGENT:&str="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.1 Safari/605.1.15";

#[allow(dead_code)]
#[derive(IntoPrimitive, TryFromPrimitive, PartialEq, Clone)]
#[repr(u8)]
pub(crate) enum RequestStatus {
    None,
    ChunkWrite,  //Chunk输出
    NormalWrite, //正常输出
    Finish,      //输出完毕
}
#[derive(PartialEq, Clone)]
enum Http1Protocol {
    Http11, //1.1
    Http10, //1.0
    None,
}
#[derive(PartialEq, Debug, Clone, Copy)]
pub enum ContentLen {
    Length(usize),
    Chunked,
    None,
}
#[derive(Default, Debug, Clone, PartialEq, Hash, Eq)]
pub enum Method {
    Head = 0,
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Connect,
    Trace,
    #[default]
    None,
}

impl From<Cow<'_, str>> for Method {
    fn from(str: Cow<'_, str>) -> Self {
        match str.as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            other => {
                panic!("未处理 Method 类型 {}", other);
            }
        }
    }
}
#[derive(Clone)]
pub struct Request {
    proto: Http1Protocol, //协议
    uri: String,
    pub(crate) method: Method,
    pub keep_alive: bool,
    pub(crate) content_length: ContentLen,
    pub(crate) header: HashMap<String, String>,
    pub(crate) code: u16,
    //max_redirects: u8, //重定向次数，默认5，最大100
    //redirects: u8,
    max_package_size: usize, //最大数据包
    //is_https: bool,
    content_start: usize,                  //content起始位
    pub unread_content_length: ContentLen, //剩余content
    //pub(crate) status: RequestStatus,
    //pub(crate) out: MsgBuffer,
    query_data: Option<HashMap<String, Vec<String>>>,
    remote_addr: String,
}

use static_init::dynamic;
use std::fmt;
use std::future::Future;

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut f = f.debug_struct("Request");
        f.field("uri", &self.uri);
        if self.code > 0 {
            f.field("code", &self.code);
        }
        f.field("content_length", &self.content_length);
        f.field("headers", &self.header);
        f.field("path", &self.path());
        f.field("method", &self.method);
        f.finish()
    }
}

#[allow(dead_code)]
impl Request {
    pub fn new() -> Request {
        let req = Request {
            content_start: 0,
            proto: Http1Protocol::None,
            method: Method::None,
            uri: DEFAULT_STRING.to_owned(),
            keep_alive: false,
            content_length: ContentLen::None,
            header: Default::default(),
            code: 0,
            max_package_size: 10 * 1024 * 1024,
            //status: RequestStatus::None,
            query_data: Default::default(),
            unread_content_length: ContentLen::None,
            remote_addr: DEFAULT_STRING.to_owned(),
        };
        req
    }

    /*pub async fn do_http1raw_request<S: ConnS, T: ConnT + 'a>(
        &'a mut self,
        rawdata: &[u8],
        mut stream: &'a mut ConnRead<'a, S, T>,
    ) -> Result<Response<'a, S, T>, HttpError> {
        stream.write(rawdata.to_vec()).await?;
        Request::parsereq_from_stream(self, stream).await?;
        Response::from_request(self.clone(), stream)
    }*/

    async fn parse_resp(self, _conn: TcpConn<Self>) -> Result<(), HttpError> {
        return Err(HttpError::Custom("parse_resp未处理".to_string()));
        /*loop {
            self.next_req().await?;
            match self.code {
                301 | 302 | 307 => {
                    self.redirects += 1;
                    self.read_finish().await?;
                    match self.max_redirects {
                        0 => return Ok(Response::from_request(self)),
                        _ => {
                            if self.redirects >= self.max_redirects {
                                return Err(HttpError::TooManyRedirects);
                            }
                        }
                    }

                    match self.header.get("Location") {
                        Some(a) => {
                            let rawdata = format!("{method} {path} HTTP/1.1\r\nAccept-Language: zh-CN,zh-Hans;q=0.9\r\nAccept-Encoding: gzip, deflate\r\nHost: {host}\r\nConnection: keep-alive\r\n\r\n",method=self.client_method,path=a.to_string(),host=self.host);
                            self.stream.write_data(rawdata.as_bytes().to_vec()).await?;
                        }
                        None => {
                            return Err(HttpError::Custom("Location url not found".to_string()));
                        }
                    }
                }
                _ => {
                    self.stream.shift(self.body_start).await?;
                    return Ok(Response::from_request(self));
                }
            }
        }*/
    }

    pub async fn read_finish<T>(
        conn: &mut TcpConn<T>,
        unread_content_length: ContentLen,
    ) -> Result<(), HttpError> {
        match unread_content_length {
            ContentLen::Length(size) => {
                if size>0{
                    conn.shift(size).await?
                }
            },
            ContentLen::Chunked => {
                loop {
                    let buf = conn.readline().await?;
                    let len = u32::from_str_radix(buf.as_str(), 16)?;
                    if len == 0 {
                        conn.readline().await?;
                        //stream.buffer=Vec::new();
                        break;
                    } else {
                        conn.shift((len + 2) as usize).await?
                    }
                }
            }
            _ => {
                return Err(HttpError::Custom("err content_length".to_string()));
            }
        }

        Ok(())
    }

    pub async fn parse_req_server<T>(conn: &mut TcpConn<T>,max_package_size:usize) -> Result<Option<Self>, HttpError> {
        //测试
        //match self.do_parse_split().await? {

        match Self::do_parse(conn,max_package_size).await? {
            Some(mut req) => {
                req.after_req(conn)?;
                Ok(Some(req))
            }
            None => Ok(None),
        }
    }

    async fn do_parse<T>(conn: &mut TcpConn<T>,max_package_size:usize) -> Result<Option<Self>, HttpError> {
        let data = conn.buffer_data();
        let mut iter = data.iter().enumerate();
        let mut pos = 0;
        loop {
            match iter.next() {
                Some((k, v1)) => {
                    if *v1 == 13
                        && let Some((_, v2)) = iter.next()
                        && *v2 == 10
                    {
                        //先处理到第一行
                        //println!("{}",String::from_utf8_lossy(&data[pos..k]));
                        let mut strs = data[pos..k].split(|num| *num == 32);
                        pos = k + 2;
                        if let (Some(data1), Some(data2), Some(data3)) =
                            (strs.next(), strs.next(), strs.next())
                        {
                            let mut req = Request {
                                proto: match String::from_utf8_lossy(data3).as_str() {
                                    "HTTP/1.1" => Http1Protocol::Http11,
                                    _ => Http1Protocol::Http10,
                                },
                                uri: String::from_utf8_lossy(data2).to_string(),
                                method: String::from_utf8_lossy(data1).into(),
                                keep_alive: true,
                                content_length: ContentLen::None,
                                header: Default::default(),
                                code: 0,
                                max_package_size,
                                content_start: 0,
                                unread_content_length: ContentLen::None,
                                query_data: None,
                                remote_addr: "".to_string(),
                            };

                            //self.header.clear();
                            loop {
                                match iter.next() {
                                    Some((k, v1)) => {
                                        if req.max_package_size>0 && k>req.max_package_size{
                                            return Err(HttpError::LargePackage)
                                        }
                                        if *v1 == 13
                                            && let Some((_, v2)) = iter.next()
                                            && *v2 == 10
                                        {
                                            if pos == k {
                                                req.content_start = k + 2;
                                                conn.shift(req.content_start).await?;
                                                return Ok(Some(req));
                                            } else {
                                                let mut strs = data[pos..k].split(|num| *num == 58);
                                                if let (Some(key), Some(val)) =
                                                    (strs.next(), strs.next())
                                                {
                                                    unsafe {
                                                        req.header.insert(
                                                            String::from_utf8_lossy(key)
                                                                .to_lowercase(),
                                                            String::from_utf8_unchecked(
                                                                val[1..].to_vec(),
                                                            ),
                                                        );
                                                    }
                                                }

                                                pos = k + 2;
                                            }
                                        }
                                    }
                                    None => return Ok(None),
                                }
                            }
                        }
                    }
                }
                None => return Ok(None),
            }
        }
    }

    fn after_req<T>(&mut self, conn:&mut TcpConn<T>) -> Result<(), HttpError> {
        match self.header.get("connection") {
            Some(c) => {
                self.keep_alive = c == "Keep-Alive" || c == "keep-alive";
            }
            None => {
                self.keep_alive = self.proto == Http1Protocol::Http11;
            }
        }
        if let Some(data) = self.header.get("transfer-encoding")
            && data == "chunked"
        {
            self.content_length = ContentLen::Chunked;
            self.unread_content_length = ContentLen::Chunked;
        } else if let Some(len) = self.header.get("content-length") {
            self.content_length = ContentLen::Length(len.parse::<usize>()?);
            self.unread_content_length = ContentLen::Length(len.parse::<usize>()?);
        } else {
            //此时应该是/r/n结束后面没有任何数据
            self.unread_content_length = ContentLen::Length(0);
        };
        self.remote_addr=conn.get_inner().addr().to_string();
        //self.query_data.clear();
        //self.begin = time.Now();

        /*if self.method == "POST" {
            if let Some(s)=self.header.get("content-type") && s.contains("application/x-www-form-urlencoded") {

                for _, str := range strings.Split(string(req.body), "&") {
                    if i := strings.Index(str, "="); i > 0 {
                        k, err1 := url.QueryUnescape(str[:i])
                        v, err2 := url.QueryUnescape(str[i+1:])
                        if err1 == nil && err2 == nil {
                            req.addpost(k, v)
                        }

                    }
                }
            }
            if strings.Contains(req.Header("content-type"), "multipart/form-data") {
                if i := strings.Index(req.Header("content-type"), "boundary="); i > -1 {
                    for _, str := range strings.Split(string(req.body), "--"+req.Header("content-type")[i+9:])[1:] {
                        str = str[2:]
                        i := strings.Index(str, "\r\n")
                        if i > -1 && i+4 <= len(str)-2 {
                            if strings.Contains(str[:i], "Content-Disposition: form-data;") {
                                var key, value string
                                if j := strings.Index(str[:i], `name="`); j > -1 {
                            key, _ = url.QueryUnescape(str[j+6 : i-1])
                        }
                        value = str[i+4 : len(str)-2]
                        if key != "" {
                            req.addpost(key, value)
                        }
                    }

                }

            }
        }*/
        Ok(())
    }
    pub fn path(&self) -> &str {
        match self.uri.contains("?") {
            false => self.uri.as_str(),
            true => self.uri.split_once("?").unwrap().0,
        }
    }
    pub fn get_query_str(&self) -> &str {
        match self.uri.contains("?") {
            false => DEFAULT_STRING.as_str(),
            true => self.uri.split_once("?").unwrap().1,
        }
    }
    pub fn uri(&self) -> &str {
        self.uri.as_str()
    }
    pub fn query(&mut self, key: &str) -> Result<Option<&String>, HttpError> {
        if self.query_data == None {
            self.query_data = Some(HashMap::new());
            let query = self.get_query_str().to_string();
            if query != "" {
                let a = query.split(|x| x == '&').collect::<Vec<&str>>();
                for str in a {
                    let s = str.split(|x| x == '=').collect::<Vec<&str>>();
                    if s.len() == 2 {
                        let k = urlencoding::decode(s[0])?;
                        let v = urlencoding::decode(s[1])?;
                        self.add_query(k.to_string(), v.to_string());
                    }
                }
            }
        }
        Ok(match self.query_data.as_ref().unwrap().get(key) {
            None => None,
            Some(v) => v.get(0),
        })
    }
    // pub async fn upgrade_to_ws(mut self) -> Result<WSconn, HttpError> {
    //     if self.status == RequestStatus::None {
    //         self.status = RequestStatus::Finish;
    //         self.out.reset();
    //
    //         if self.method != Method::Get {
    //             self.out.write_string("HTTP/1.1 403 Error\r\nContent-Type: text/plain\r\nContent-Length: 11\r\nConnection: close\r\n\r\nUnknonw MSG");
    //             return Err(HttpError::WSProtocolErr("ws协议不使用get".to_string()));
    //         }
    //
    //         if self.header.remove("upgrade").unwrap_or_default() != "websocket" {
    //             self.out.write_string("HTTP/1.1 403 Error\r\nContent-Type: text/plain\r\nContent-Length: 11\r\nConnection: close\r\n\r\nUnknonw MSG");
    //             return Err(HttpError::WSProtocolErr("ws协议没有upgrade".to_string()));
    //         }
    //         if self
    //             .header
    //             .remove("sec-websocket-version")
    //             .unwrap_or_default()
    //             != "13"
    //         {
    //             self.out.write_string("HTTP/1.1 403 Error\r\nContent-Type: text/plain\r\nContent-Length: 11\r\nConnection: close\r\n\r\nUnknonw MSG");
    //             return Err(HttpError::WSProtocolErr("ws协议没有Extensions".to_string()));
    //         }
    //         let challenge_key = self.header.remove("sec-websocket-key").unwrap_or_default();
    //         if challenge_key == "" {
    //             self.out.write_string("HTTP/1.1 403 Error\r\nContent-Type: text/plain\r\nContent-Length: 11\r\nConnection: close\r\n\r\nUnknonw MSG");
    //             return Err(HttpError::WSProtocolErr(
    //                 "ws协议没有challenge_key".to_string(),
    //             ));
    //         }
    //         self.out.write_string("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ");
    //         self.out
    //             .write_string(&compute_acceptkey(challenge_key.to_string()));
    //         self.out.write_string("\r\n");
    //         let mut is_compress = false;
    //         if self
    //             .header
    //             .remove("sec-webSocket-extensions")
    //             .unwrap_or_default()
    //             == "permessage-deflate"
    //         {
    //             is_compress = true;
    //         }
    //         if is_compress {
    //             self.out.write_string("Sec-Websocket-Extensions: permessage-deflate; server_no_context_takeover; client_no_context_takeover\r\n");
    //         }
    //         self.out.write_string("\r\n");
    //         self.conn.write_byte(self.out.as_slice()).await?;
    //         let ws = WSconn::new_server(self.conn.get_write_conn(), is_compress);
    //
    //         return Ok(ws);
    //     }
    //     Err(HttpError::None)
    // }

    fn add_query(&mut self, name: String, value: String) {
        if self.query_data.is_none() {
            self.query_data = Some(HashMap::new());
        }
        let query_data = self.query_data.as_mut().unwrap();
        if let Some(v) = query_data.get_mut(&name) {
            v.push(value);
        } else {
            query_data.insert(name, vec![value]);
        };
    }
    pub fn out_404(&mut self, msg: Option<String>) -> Result<(), HttpError> {
        panic!("{:?}", msg)
        // if self.status == RequestStatus::None {
        //     self.status = RequestStatus::NormalWrite;
        //     self.out1.reset();
        //     self.out_code = 404;
        //     if let Some(msg) = msg {
        //         self.out1.write_string(&msg)
        //     };
        //     return Ok(());
        // } else {
        //     return Err(HttpError::AlreadyWrite);
        // }
    }
    pub async fn out_err(&mut self, msg: Option<String>) -> Result<(), HttpError> {
        panic!("{:?}", msg)
        // if self.status == RequestStatus::None {
        //     self.status = RequestStatus::NormalWrite;
        //     self.out1.reset();
        //     self.out_code = 502;
        //     if let Some(msg) = msg {
        //         self.out1.write_string(&msg)
        //     };
        //     self.write_finish().await?;
        //     return Ok(());
        // } else {
        //     return Err(HttpError::AlreadyWrite);
        // }
    }
    pub fn builder() -> Builder {
        Builder::new()
    }
    pub fn all_header(&self) -> HashMap<String, String> {
        self.header.clone()
    }
    pub fn header(&self, key: &str) -> Option<&String> {
        self.header.get(key)
    }
pub fn remote_addr(&self) -> &str {
    self.remote_addr.as_str()
}
    // pub async fn txt(&mut self) -> Result<String, HttpError> {
    //     Ok(String::from_utf8(self.bytes().await?)?)
    // }
    // pub async fn bytes(&mut self) -> Result<Vec<u8>, HttpError> {
    //     match self.content_length {
    //         ContentLen::Length(l) => {
    //             while self.conn.buffer_len() < l {
    //                 self.conn.read_data().await?;
    //             }
    //             let data = self.conn.buffer_data()[..l].to_vec();
    //             self.conn.shift(l).await?;
    //             return Ok(data);
    //         }
    //         ContentLen::Chunked => {
    //             let mut ret = vec![];
    //             loop {
    //                 let mut data = get_one_chucnk_data(&mut self.conn).await?;
    //                 if data.len() == 0 {
    //                     break;
    //                 }
    //                 ret.append(&mut data);
    //             }
    //             Ok(ret)
    //         }
    //         ContentLen::None => Ok(Default::default()),
    //     }
    // }
}
#[allow(dead_code)]
fn compute_acceptkey(challenge_key: String) -> String {
    let mut hasher = Sha1::new();
    hasher.update(challenge_key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let result = hasher.finalize();
    general_purpose::STANDARD.encode(result)
}
#[allow(dead_code)]
#[derive(Debug)]
pub struct HttpCookie {
    value: String,
    max_age: u32,
}
#[allow(dead_code)]
pub struct Builder {
    uri: String,
    keep_alive: bool,
    read_timeout: Duration,
    method: String,
    out_header: HashMap<String, String>,
}

impl Builder {
    pub fn new() -> Self {
        let ret = Self {
            uri: DEFAULT_STRING.to_owned(),
            keep_alive: true,
            read_timeout: Duration::from_secs(10),
            method: DEFAULT_STRING.to_owned(),
            out_header: Default::default(),
        };
        ret
    }
    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.out_header.insert(k.into(), v.into());
        self
    }
    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into();
        self
    }

    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = uri.into();
        self
    }
    pub fn with_readtimeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }
    pub async fn excute_body(self, _body: impl Body) -> Result<Response, HttpError> {
        /* use std::sync::Arc;
        use tokio::net::TcpStream;
        use tokio_rustls::rustls::{ClientConfig, OwnedTrustAnchor, RootCertStore, ServerName};
        use tokio_rustls::TlsConnector;
        let mut root_cert_store = RootCertStore::empty();
        root_cert_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));
        let config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));

        let u = url::Url::parse(self.uri.as_str())?;
        let ishttps = u.scheme() == "https";
        let path = u.path().to_string();
        let port = match u.port() {
            Some(p) => p,
            None => {
                if ishttps {
                    443
                } else {
                    80
                }
            }
        };

        let host = match u.host_str() {
            Some(host) => host,
            None => return Err(HttpError::Custom("uri错误，输入的uri不含host".to_string())),
        };
        let domain = u.domain();

        let (domain, addr) = if domain.is_some() {
            let domain = domain.unwrap();
            use dns_lookup::lookup_host;
            let ips: Vec<std::net::IpAddr> = lookup_host(domain).unwrap();
            if ips.len() == 0 {
                return Err(HttpError::Custom(format!("DNS {} 解析失败", domain)));
            }
            (host, ips[0].to_string())
        } else {
            (host, host.to_string())
        };

        let dnsname = ServerName::try_from(domain).unwrap();

        let stream = TcpStream::connect(format!("{}:{}", addr, port)).await?;
        let addr = stream.peer_addr()?;
        let mut resp: Response<T,tokio_rustls::client::TlsStream<tokio::net::TcpStream>> =
            match ishttps {
                true => {
                    let stream = connector.connect(dnsname, stream).await?;
                    Response::new(ConnBase::new(addr, net::conn::Stream::Ssl(stream)))
                }
                _ => Response::new(ConnBase::new(addr, net::conn::Stream::Tcp(stream))),
            };

        resp.uri = self.uri.clone();
        resp.method = self.method.clone();
        resp.out.reset();
        let outpath = match u.query() {
            Some(u) => path + "?" + u,
            None => path,
        };
        resp.out.write_string(&format!(
            "{method} {outpath} HTTP/1.1\r\n",
            method = resp.method,
        ));

        if !self.keep_alive {
            resp.out.write_string("Connection: close\r\n");
        }
        for (k, v) in &self.out_header {
            resp.out.write_string(&urlencoding::encode(k));
            resp.out.write_string(": ");
            resp.out.write_string(v);
            resp.out.write_string("\r\n")
        }
        let len = body.len();
        resp.out.write_string("Content-Length: ");
        resp.out.write_string(&len.to_string());
        resp.out.write_string("\r\n\r\n");
        resp.stream.write_byte(resp.out.bytes()).await?;
        body.send(&mut resp.stream).await?;
        let mut req = Request::new(&mut resp.stream);
        req.next_req().await?;
        resp.code = req.code;
        resp.host = req.host.clone();
        resp.path = req.path().to_string();
        resp.method = req.method.clone();
        resp.content_length = req.content_length;
        resp.header = req.header;*/
        panic!("未处理")
    }
}
//封装一个简单的body
pub trait Body: Send {
    fn len(&self) -> usize;
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, HttpError>> + Send;
    fn send<T>(
        &mut self,
        stream: &mut TcpConn<T>,
    ) -> impl Future<Output = Result<(), HttpError>> + Send {
        async {
            let mut len = self.len();
            if len > 0 {
                let mut buf = [0; 16384];
                while len > 0 {
                    let size = self.read(&mut buf).await?;
                    len -= size;
                    stream.write_byte(&buf[..size]).await?;
                }
            }
            Ok(())
        }
    }
}
impl Body for MsgBuffer {
    fn len(&self) -> usize {
        self.len()
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, HttpError> {
        if self.len() < buf.len() {
            let outlen = self.len();
            buf[..self.len()].copy_from_slice(self.bytes());
            self.reset();
            Ok(outlen)
        } else {
            buf.copy_from_slice(self.bytes());
            self.shift(buf.len());
            Ok(buf.len())
        }
    }
}
pub struct NoneBody {}
impl Body for NoneBody {
    fn len(&self) -> usize {
        0
    }
    async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, HttpError> {
        return Ok(0);
    }
}
#[allow(unused_assignments)]
#[allow(dead_code)]
pub(crate) async fn get_one_chucnk_data<T>(conn: &mut TcpConn<T>) -> Result<Vec<u8>, HttpError> {
    let mut length = 0;
    //获取长度
    loop {
        let buf = conn.buffer_data();
        if let Some(i) = buf.windows(2).position(|x| x == &[13, 10])
            && i > 0
        {
            let hexlen = String::from_utf8_lossy(&buf[..i]);
            length = usize::from_str_radix(hexlen.as_ref(), 16)?;
            conn.shift(i + 2).await?;

            break;
        };
        conn.read_data().await?;
    }
    while conn.buffer_len() < length + 2 {
        conn.read_data().await?;
    }
    let outdata = conn.buffer_data()[..length].to_vec();
    conn.shift(length + 2).await?;
    Ok(outdata)
}
pub(crate) fn http1code(code: u16) -> Result<&'static [u8], HttpError> {
    let ret: &[u8] = match code {
        100 => b"HTTP/1.1 100 Continue\r\n",
        101 => b"HTTP/1.1 101 Switching Protocols\r\n",
        102 => b"HTTP/1.1 102 Processing\r\n",
        200 => b"HTTP/1.1 200 OK\r\n",
        201 => b"HTTP/1.1 201 Created\r\n",
        202 => b"HTTP/1.1 202 Accepted\r\n",
        203 => b"HTTP/1.1 203 Non-Authoritative Information\r\n",
        204 => b"HTTP/1.1 204 No Content\r\n",
        205 => b"HTTP/1.1 205 Reset Content\r\n",
        206 => b"HTTP/1.1 206 Partial Content\r\n",
        207 => b"HTTP/1.1 207 Multi-Status\r\n",
        300 => b"HTTP/1.1 300 Multiple Choices\r\n",
        301 => b"HTTP/1.1 301 Moved Permanently\r\n",
        302 => b"HTTP/1.1 302 Move Temporarily\r\n",
        303 => b"HTTP/1.1 303 See Other\r\n",
        304 => b"HTTP/1.1 304 Not Modified\r\n",
        305 => b"HTTP/1.1 305 Use Proxy\r\n",
        306 => b"HTTP/1.1 306 Switch Proxy\r\n",
        307 => b"HTTP/1.1 307 Temporary Redirect\r\n",
        400 => b"HTTP/1.1 400 Bad http1request\r\n",
        401 => b"HTTP/1.1 401 Unauthorized\r\n",
        402 => b"HTTP/1.1 402 Payment Required\r\n",
        403 => b"HTTP/1.1 403 Forbidden\r\n",
        404 => b"HTTP/1.1 404 Not Found\r\n",
        405 => b"HTTP/1.1 405 Method Not Allowed\r\n",
        406 => b"HTTP/1.1 406 Not Acceptable\r\n",
        407 => b"HTTP/1.1 407 Proxy Authentication Required\r\n",
        408 => b"HTTP/1.1 408 http1request Timeout\r\n",
        409 => b"HTTP/1.1 409 Conflict\r\n",
        410 => b"HTTP/1.1 410 Gone\r\n",
        411 => b"HTTP/1.1 411 Length Required\r\n",
        412 => b"HTTP/1.1 412 Precondition Failed\r\n",
        413 => b"HTTP/1.1 413 http1request Entity Too Large\r\n",
        414 => b"HTTP/1.1 414 http1request-URI Too Long\r\n",
        415 => b"HTTP/1.1 415 Unsupported Media Type\r\n",
        416 => b"HTTP/1.1 416 Requested Range Not Satisfiable\r\n",
        417 => b"HTTP/1.1 417 Expectation Failed\r\n",
        418 => b"HTTP/1.1 418 I'm a teapot\r\n",
        421 => b"HTTP/1.1 421 Misdirected http1request\r\n",
        422 => b"HTTP/1.1 422 Unprocessable Entity\r\n",
        423 => b"HTTP/1.1 423 Locked\r\n",
        424 => b"HTTP/1.1 424 Failed Dependency\r\n",
        425 => b"HTTP/1.1 425 Too Early\r\n",
        426 => b"HTTP/1.1 426 Upgrade Required\r\n",
        449 => b"HTTP/1.1 449 Retry With\r\n",
        451 => b"HTTP/1.1 451 Unavailable For Legal Reasons\r\n",
        500 => b"HTTP/1.1 500 Internal Server Error\r\n",
        501 => b"HTTP/1.1 501 Not Implemented\r\n",
        502 => b"HTTP/1.1 502 Bad Gateway\r\n",
        503 => b"HTTP/1.1 503 Service Unavailable\r\n",
        504 => b"HTTP/1.1 504 Gateway Timeout\r\n",
        505 => b"HTTP/1.1 505 HTTP Version Not Supported\r\n",
        506 => b"HTTP/1.1 506 Variant Also Negotiates\r\n",
        507 => b"HTTP/1.1 507 Insufficient Storage\r\n",
        509 => b"HTTP/1.1 509 Bandwidth Limit Exceeded\r\n",
        510 => b"HTTP/1.1 510 Not Extended\r\n",
        600 => b"HTTP/1.1 600 Unparseable Response Headers\r\n",
        other => return Err(HttpError::Custom(format!("unkown http1 code {}", other))),
    };
    Ok(ret)
}
