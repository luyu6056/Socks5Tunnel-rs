use crate::utils::md5_s;
use codec::response::Response;
use net::buffer::MsgBuffer;
use net::err::NetError;
use std::collections::HashMap;
use std::str::FromStr;
use url::{Host, Url};
#[allow(dead_code)]
pub async fn post<T>(
    uri: &str,
    body: Vec<u8>,
    content_type: &str,
    herder: Option<HashMap<String, String>>,
) -> Result<Response, NetError> {
    let url = Url::from_str(uri)?;
    let host = url.host().unwrap_or(Host::Domain(""));

    let mut build = codec::http1::Builder::new()
        .header("Host", host.to_string())
        .header("Content-Type", content_type);
    if let Some(herder) = herder {
        for (k, v) in herder {
            build = build.header(k, v);
        }
    }

    let request = build
        .method("POST")
        .uri(uri)
        .excute_body(MsgBuffer::from(body))
        .await?;

    Ok(request)
}
#[allow(dead_code)]
pub async fn get<T>(
    uri: &str,
    herder: Option<HashMap<String, String>>,
) -> Result<Response, NetError> {
    let url = Url::from_str(uri)?;
    let host = url.host().unwrap_or(Host::Domain(""));

    let mut build = codec::http1::Builder::new().header("Host", host.to_string());
    if let Some(herder) = herder {
        for (k, v) in herder {
            build = build.header(k, v);
        }
    }
    use codec::http1::NoneBody;
    let request = build
        .method("GET")
        .uri(uri)
        .excute_body(NoneBody {})
        .await?;
    Ok(request)
}

pub struct MultipartFormdata {
    inner: HashMap<String, String>,
    boundary: String,
}

#[allow(dead_code)]
impl MultipartFormdata {
    pub fn new() -> Self {

        let n = fastrand::u64(0x0   ..0xffffffff);
        MultipartFormdata {
            inner: Default::default(),
            boundary: format!("{}", md5_s(n.to_string())),
        }
    }
    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.insert(key.into(), value.into());
    }
    pub fn body(&mut self) -> Vec<u8> {
        let mut str = String::new();
        for (k, v) in &self.inner {
            str += &format!("--{}\r\n", self.boundary);
            str += &format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", k);
            str += &format!("{}\r\n", v);
        }
        str += &format!("--{}--\r\n\r\n", self.boundary);
        str.as_bytes().to_vec()
    }
    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }
}
