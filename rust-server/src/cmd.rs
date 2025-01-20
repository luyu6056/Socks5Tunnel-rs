use num_enum::*;
#[derive(IntoPrimitive, Debug, TryFromPrimitive,PartialEq,Clone,PartialOrd)]
#[repr(u8)]
pub(crate) enum Cmd {
    None,
    GetFd,       //请求fd
    Fd,          //返回fd
    Msg,         //发送消息
    MsgEnd,      //消息尾
    MsgRec,      //确认消息
    MsgResend,   //重发消息
    DeleteFd,    //删除fd资源
    MsgResendNo, //重发整条msgno
    WindowsUpdate,
    Ping, //请求ping
    Pong, //返回pong
    UdpCheckIn,
    UdpCheckOut,
    UdpCheckMsg,
    UdpCheckRecNo,
    DeleteIp, //重启要求删除远程资源
    Reg,
}