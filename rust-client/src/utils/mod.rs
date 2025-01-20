mod time;
pub use time::*;
pub fn md5_s(s: impl Into<String>) -> String {
    format!("{:x}", md5::compute(s.into()))
}
