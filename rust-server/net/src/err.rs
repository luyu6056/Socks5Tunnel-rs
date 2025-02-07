use num_enum::*;
use std::time::SystemTimeError;
use thiserror::Error;
use tokio::time::error::Elapsed;

#[derive(Error, Debug, Clone)]
pub enum NetError {
    #[error("IoError: {0}")]
    IoError(String),

    #[error("{0}")]
    Custom(String),

    #[error("tcp read timeout")]
    TcpReadTimeout,

    #[error("tcp write timeout")]
    TcpWriteTimeout,

    #[error("tcp disconnected")]
    TcpDisconnected,

    #[error("UknowError")]
    UknowError,

    #[error("{0} Address error")]
    AddressError(String),

    #[error("package is too large")]
    LargePackage,

    #[error("ShutdownServer,reason: {0}")]
    ShutdownServer(String),

    #[error("no error")]
    None,

    #[error("close no error")]
    Close,

    #[error("ProtocolErr {0}")]
    ProtocolErr(String),

    #[error("channel {0}")]
    Channel(String),

    #[error("{0}")]
    FormatErr(String),

    #[error("json转化失败 {0}")]
    JsonErr(String),
    #[error("tls 错误 {0}")]
    Tls(String),
}

impl NetError {
    pub fn new_with_string(s: String) -> NetError {
        NetError::Custom(s)
    }
}
impl std::convert::From<std::io::Error> for NetError {
    fn from(err: std::io::Error) -> Self {
        NetError::IoError(err.to_string())
    }
}
impl std::convert::From<Elapsed> for NetError {
    fn from(err: Elapsed) -> Self {
        NetError::Custom(err.to_string())
    }
}

impl std::convert::From<SystemTimeError> for NetError {
    fn from(err: SystemTimeError) -> Self {
        NetError::Custom(err.to_string())
    }
}

impl<MessageType: num_enum::TryFromPrimitive> std::convert::From<TryFromPrimitiveError<MessageType>>
    for NetError
{
    fn from(_err: TryFromPrimitiveError<MessageType>) -> Self {
        NetError::ProtocolErr("error MessageType".to_string())
    }
}
use std::string::FromUtf8Error;
impl std::convert::From<FromUtf8Error> for NetError {
    fn from(err: FromUtf8Error) -> Self {
        NetError::FormatErr(format!("byte转utf8错误,{}", err.to_string()))
    }
}

impl std::convert::From<serde_json::Error> for NetError {
    fn from(err: serde_json::Error) -> Self {
        NetError::JsonErr(err.to_string())
    }
}

impl std::convert::From<std::num::ParseIntError> for NetError {
    fn from(err: std::num::ParseIntError) -> Self {
        NetError::FormatErr(format!("数字转换失败{}", err.to_string()))
    }
}
impl std::convert::From<std::num::ParseFloatError> for NetError {
    fn from(err: std::num::ParseFloatError) -> Self {
        NetError::FormatErr(format!("数字转换失败{}", err.to_string()))
    }
}
impl std::convert::From<url::ParseError> for NetError {
    fn from(err: url::ParseError) -> Self {
        NetError::FormatErr(format!("url转换失败{}", err.to_string()))
    }
}
impl std::convert::From<hyper::Error> for NetError {
    fn from(err: hyper::Error) -> Self {
        NetError::FormatErr(format!("hyper请求错误{}", err.to_string()))
    }
}
impl std::convert::From<hyper::http::Error> for NetError {
    fn from(err: hyper::http::Error) -> Self {
        NetError::FormatErr(format!("hyper请求错误{}", err.to_string()))
    }
}
impl From<tokio_native_tls::native_tls::Error> for NetError {
    fn from(err: tokio_native_tls::native_tls::Error) -> Self {
        NetError::IoError(format!("tls 错误{}", err.to_string()))
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for NetError {
    fn from(err: tokio::sync::mpsc::error::SendError<T>) -> Self {
        NetError::Channel(err.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::TrySendError<T>> for NetError {
    fn from(err: tokio::sync::mpsc::error::TrySendError<T>) -> Self {
        NetError::Channel(err.to_string())
    }
}
impl From<Box<dyn std::error::Error>> for NetError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        NetError::Custom(err.to_string())
    }
}
