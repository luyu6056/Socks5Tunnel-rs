#![feature(let_chains)]
#![feature(const_trait_impl)]
#![feature(slice_split_once)]
#![feature(str_as_str)]
#![feature(fn_traits)]
extern crate core;
use net::err::NetError;
#[doc(hidden)]
use std::result::Result as StdResult;

pub mod error;
pub mod http1;
pub mod http1example;
pub mod response;
pub mod router;
pub mod websocket;

pub type Result<T> = StdResult<T, NetError>;
