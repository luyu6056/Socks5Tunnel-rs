use chrono::prelude::*;
use chrono_tz::Tz;

//默认时区为hk
pub const LOCAL: Tz = Tz::Hongkong;

//封装的golang time
#[derive(Debug)]
pub struct Time {
    data: DateTime<Tz>,
}

pub fn now() -> Time {
    let now: DateTime<Utc> = Utc::now();
    Time {
        data: LOCAL.from_utc_datetime(&now.naive_utc()),
    }
}
/*#[allow(dead_code)]
///fmt = "%Y-%m-%d %H:%M:%S";正常情况，必须补全
pub fn parse_in_location(fmt: &str, value: &str, loc: Tz) -> Result<Time, Error> {
    let tz = match loc.datetime_from_str(value, fmt) {
        Ok(t) => t,
        Err(e) if e.kind() == ParseErrorKind::NotEnough => loc.datetime_from_str(
            &(value.to_string() + " 00:00:00"),
            &(fmt.to_string() + " %H:%M:%S"),
        )?,
        other => other?,
    };
    Ok(Time { data: tz })
}*/
#[allow(dead_code)]
impl Time {
    pub fn unix(&self) -> i64 {
        self.data.timestamp()
    }
    pub fn unix_millis(&self) -> i64 {
        self.data.timestamp_millis()
    }
    /// %Y-%m-%d %H:%M:%S
    pub fn format(&self, fmt: &str) -> String {
        self.data.format(fmt).to_string()
    }
    /// 2006-01-02 15:04:05,并不支持所有方式
    pub fn go_format(&self, fmt: &str) -> String {
        let fmt = fmt
            .replace("2006", "%Y")
            .replace("01", "%m")
            .replace("02", "%d")
            .replace("15", "%H")
            .replace("04", "%M")
            .replace("05", "%S");
        self.data.format(fmt.as_str()).to_string()
    }
}
#[tokio::test]
pub async fn test_time() {}
