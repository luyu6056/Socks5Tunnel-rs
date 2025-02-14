use net::buffer::MsgBuffer;
use net::conn::{ConnWrite, ConnWriter, TcpConn};
use net::err::NetError;
use num_enum::*;
use std::convert::Into;

#[derive(IntoPrimitive, Debug, TryFromPrimitive, PartialEq, Clone)]
#[repr(u8)]
pub enum MessageType {
    NoFrame = 255,
    ContinuationFrame = 0,
    TextMessage = 1,

    // BinaryMessage denotes a binary data message.
    BinaryMessage = 2,

    // CloseMessage denotes a close control message. The optional message
    // payload contains a numeric code and text. Use the FormatCloseMessage
    // function to format a close message payload.
    CloseMessage = 8,

    // PingMessage denotes a ping control message. The optional message payload
    // is UTF-8 encoded text.
    PingMessage = 9,

    // PongMessage denotes a ping control message. The optional message payload
    // is UTF-8 encoded text.
    PongMessage = 10,
}

impl MessageType {
    fn is_control(&self) -> bool {
        return self == &MessageType::CloseMessage
            || self == &MessageType::PingMessage
            || self == &MessageType::PongMessage;
    }
    fn is_data(&self) -> bool {
        return self == &MessageType::TextMessage || self == &MessageType::BinaryMessage;
    }
    fn to_int(self) -> u8 {
        self.into()
    }
}

const MASK_BIT: u8 = 1 << 7;
const FINAL_BIT: u8 = 1 << 7;
const RSV1_BIT: u8 = 1 << 6;
const RSV2_BIT: u8 = 1 << 5;
const RSV3_BIT: u8 = 1 << 4;
const MSGHEADER: usize = 2;
const MSGLENGTH8: usize = 8;
const MSGLENGTH2: usize = 2;
const MAX_CONTROL_FRAME_PAYLOAD_SIZE: usize = 125;
const WS_MAX_MSGLEN: usize = 2 << 24 - 1;
const DEFAULT_WRITE_BUFFER_SIZE: usize = 81920;
const CONTINUATION_FRAME: u8 = 0;
const CONTINUATION_FRAMEFINAL_BIT: u8 = CONTINUATION_FRAME | FINAL_BIT;
//const BINARY_MESSAGEFINAL_BIT: u8 = 2 | FINAL_BIT;
#[derive(Debug)]
pub struct WSconn {
    write: ConnWriter,
    //以下是发送接收相关
    pub(crate) is_compress: bool,
    read_remaining: usize, // bytes remaining in current frame.
    read_final: bool,      // true the current message has more frames.
    read_length: usize,    // Message size.
    read_decompress: bool,
    is_server: bool,
    //Conn           *ClientConn
    readbuf: MsgBuffer,
    message_type: MessageType,
    //fps: u32,
}
impl Clone for WSconn {
    fn clone(&self) -> Self {
        Self {
            write: self.write.clone(),
            is_compress: self.is_compress,
            read_remaining: 0,
            read_final: false,
            read_length: 0,
            read_decompress: false,
            is_server: self.is_server,
            readbuf: Default::default(),
            message_type: MessageType::NoFrame,
        }
    }
}

impl WSconn {
    pub fn new_server(write: ConnWriter, is_compress: bool) -> Self {
        let ret = WSconn {
            write,
            is_compress,
            read_remaining: 0,
            read_final: true,
            read_length: 0,
            read_decompress: false,
            is_server: true,
            readbuf: Default::default(),
            message_type: MessageType::TextMessage,
            //fps: 0,
        };

        ret
    }
    pub async fn read_message(
        &mut self,
        conn: &mut TcpConn<Self>,
    ) -> Result<Option<Vec<u8>>, NetError> {
        // Close previous reader, only relevant for decompression.
        self.read_length = 0;
        let frame_type = self.advance_frame(conn).await?;
        match frame_type {
            MessageType::TextMessage | MessageType::BinaryMessage => {
                self.message_type = frame_type;
            }
            MessageType::NoFrame => {
                return Ok(None);
            }
            MessageType::CloseMessage => {
                conn.close(Some("from websocket closeMessage".to_owned()))?;
                return Err(NetError::Close);
            }
            other => {
                println!("其他类型的type未处理 {:?}", other);
            }
        }

        if self.read_final {
            /*self.fps+=1;
            if fps := self.f; fps == 1 {
            time.AfterFunc(time.Second, func() { self.fps = 0 })
            } else if fps > wsfpslimit {
            return nil, io.EOF
            }*/
            if self.read_decompress {
                self.read_decompress = false;
                //p, err := ioutil.ReadAll(DecompressNoContextTakeover(self.readbuf))
                //return p, err
                println!("解压需要处理");
            }
            self.read_decompress = false;
            if self.readbuf.len() > WS_MAX_MSGLEN {
                return Err(NetError::LargePackage);
            }
            let ret = self.readbuf.bytes().to_vec();
            conn.shift(self.read_length).await?;
            self.readbuf.reset();
            return Ok(Some(ret));
        }
        return Ok(None);
    }
    async fn advance_frame(&mut self, conn: &mut TcpConn<Self>) -> Result<MessageType, NetError> {
        let p = &conn.buffer_data()[self.read_length..];
        let mut readlength: usize;
        if p.len() < 2 {
            return Ok(MessageType::NoFrame);
        }
        let mut p0 = p[0];
        let r#final = p0 & FINAL_BIT != 0;
        let frame_type = MessageType::try_from(p0 & 0xf)?;
        let mask = p[1] & MASK_BIT != 0;
        self.read_remaining = (p[1] & 0x7f) as usize;

        if self.is_compress && (p0 & RSV1_BIT) != 0 {
            self.read_decompress = true;
            p0 = p0 & (p0 ^ RSV1_BIT)
        }
        let rsv = p0 & (RSV1_BIT | RSV2_BIT | RSV3_BIT);
        if rsv != 0 {
            return Err(NetError::ProtocolErr(format!(
                "unexpected reserved bits 0x{}",
                rsv
            )));
        }

        // 3. Read and parse frame length.

        match self.read_remaining {
            126 => {
                if p.len() < 4 {
                    return Ok(MessageType::NoFrame);
                }
                self.read_remaining = (p[2] as usize) << 8 | (p[3] as usize);
                readlength = MSGHEADER + MSGLENGTH2;
            }

            127 => {
                if p.len() < 10 {
                    return Ok(MessageType::NoFrame);
                }
                self.read_remaining = (p[2] as usize) << 56
                    | (p[3] as usize) << 48
                    | (p[4] as usize) << 40
                    | (p[5] as usize) << 32
                    | (p[6] as usize) << 24
                    | (p[7] as usize) << 16
                    | (p[8] as usize) << 8
                    | (p[9] as usize);
                readlength = MSGHEADER + MSGLENGTH8;
            }

            _other => {
                readlength = MSGHEADER;
            }
        }

        // 4. Handle frame masking.

        if mask != self.is_server {
            return Err(NetError::ProtocolErr(format!("incorrect mask flag")));
        }

        let payload = if mask {
            if p.len() < readlength + 4 + self.read_remaining {
                return Ok(MessageType::NoFrame);
            }
            let maskkey = &p[readlength..readlength + 4];
            readlength += 4;
            let payload = &p[readlength..readlength + self.read_remaining];
            let mut ret = Vec::new();
            for (i, v) in payload.iter().enumerate() {
                ret.push(v ^ maskkey[i & 3]);
            }
            ret
        } else {
            if p.len() < self.read_remaining + readlength {
                return Ok(MessageType::NoFrame);
            }
            p[readlength..self.read_remaining + readlength].to_vec()
        };

        match frame_type {
            MessageType::CloseMessage | MessageType::PingMessage | MessageType::PongMessage => {
                if self.read_remaining > MAX_CONTROL_FRAME_PAYLOAD_SIZE {
                    return Err(NetError::ProtocolErr(format!("control frame length > 125")));
                }
                if !r#final {
                    return Err(NetError::ProtocolErr(format!("control frame not final")));
                }
            }

            MessageType::TextMessage | MessageType::BinaryMessage => {
                if !self.read_final {
                    return Err(NetError::ProtocolErr(format!(
                        "message start before final message frame"
                    )));
                }
                self.read_final = r#final
            }

            MessageType::ContinuationFrame => {
                if self.read_final {
                    return Err(NetError::ProtocolErr(format!(
                        "continuation after final message frame"
                    )));
                }
                self.read_final = r#final
            }

            other => {
                return Err(NetError::ProtocolErr(format!("unknown opcode {:?}", other)));
            }
        }
        readlength += self.read_remaining;
        self.read_length += readlength;
        // 5. For text and binary messages, enforce read limit and return.
        if frame_type == MessageType::ContinuationFrame
            || frame_type == MessageType::TextMessage
            || frame_type == MessageType::BinaryMessage
        {
            //if self.readLimit > 0 && self.ReadLength > self.readLimit {
            //	self.WriteControl(CloseMessage, FormatCloseMessage(CloseMessageTooBig, ""), time.Now().Add(writeWait))
            //	return noFrame, ErrReadLimit
            //}
            self.readbuf.write(payload.as_slice())?;
            return Ok(frame_type);
        }
        // 7. Process control frame payload.

        match frame_type {
            MessageType::PongMessage => {}
            //if err := self.handlePong(string(payload)); err != nil {
            //	return noFrame, err
            //}
            MessageType::PingMessage => {
                self.write_message(
                    MessageType::PongMessage,
                    payload.as_slice(),
                    &mut conn.get_writer_conn(),
                )
                .await?;
            }

            MessageType::CloseMessage => {
                //let mut close_code = CloseCode::CloseNoStatusReceived;
                //let close_text = "";
                if payload.len() >= 2 {
                    let close_code =
                        CloseCode::try_from((payload[0] as u16) << 8 | (payload[1] as u16))?;

                    if !is_valid_received_close_code(close_code) {
                        return Err(NetError::ProtocolErr(format!("invalid close code")));
                    }
                    let close_text = payload[2..].to_vec();
                    if let Err(_) = String::from_utf8(close_text) {
                        return Err(NetError::ProtocolErr(format!(
                            "invalid utf8 payload in close frame"
                        )));
                    }
                }

                /*message := []byte{}
                if closeCode != CloseNoStatusReceived {
                    message = FormatCloseMessage(closeCode, "")
                }
                self.WriteMessage(CloseMessage, message)
                return noFrame, &CloseError{Code: closeCode, Text: closeText}*/
            }
            other => {
                return Err(NetError::ProtocolErr(format!("error frametype{:?}", other)));
            }
        }

        Ok(frame_type)
    }
    pub async fn write_message(
        &self,
        typ: MessageType,
        data: &[u8],
        conn: &mut ConnWriter,
    ) -> Result<(), NetError> {
        if !typ.is_control() && !typ.is_data() {
            return Err(NetError::ProtocolErr(
                "websocket: bad write message type".to_string(),
            ));
        }
        if typ.is_control() && data.len() > MAX_CONTROL_FRAME_PAYLOAD_SIZE {
            return Err(NetError::ProtocolErr(
                "websocket: invalid control frame".to_string(),
            ));
        }

        let compress = false;
        //let mut writeBuf=MsgBuffer::new();
        let mut buf = vec![0; DEFAULT_WRITE_BUFFER_SIZE];
        let outbuf = buf.as_mut_slice();

        outbuf[0] = typ.clone().to_int() | FINAL_BIT;

        /*if self.is_compress && data.len() > 512 {
            compress = true;
            w := CompressNoContextTakeover(mw, 3)
            w.Write(data)
            w.Close()
        } else {*/
        //writeBuf.write(data);
        //}
        if compress {
            outbuf[0] |= RSV1_BIT;
        }
        let len = data.len();
        let mut i = 0;
        while i < len {
            let mut msgsize = len;
            if msgsize > DEFAULT_WRITE_BUFFER_SIZE - 14 {
                msgsize = DEFAULT_WRITE_BUFFER_SIZE - 14;
                outbuf[0] -= FINAL_BIT;
            }
            let mut msglen = MSGHEADER;
            if msgsize >= 65536 {
                msglen += MSGLENGTH8;
                outbuf[1] = 127;
                outbuf[2] = (msgsize >> 56) as u8;
                outbuf[3] = (msgsize >> 48) as u8;
                outbuf[4] = (msgsize >> 40) as u8;
                outbuf[5] = (msgsize >> 32) as u8;
                outbuf[6] = (msgsize >> 24) as u8;
                outbuf[7] = (msgsize >> 16) as u8;
                outbuf[8] = (msgsize >> 8) as u8;
                outbuf[9] = msgsize as u8;
            } else if msgsize >= 125 {
                msglen += MSGLENGTH2;
                outbuf[1] = 126;
                outbuf[2] = (msgsize >> 8) as u8;
                outbuf[3] = msgsize as u8;
            } else {
                outbuf[1] = msgsize as u8;
            }
            if !self.is_server {
                msglen += 4;
                unsafe {
                    std::ptr::copy(
                        data[i..i + msgsize].as_ptr(),
                        outbuf[msglen..].as_mut_ptr(),
                        msgsize,
                    );
                }
                outbuf[1] |= MASK_BIT;
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let i: u32 = rng.gen();
                let key = [(i >> 24) as u8, (i >> 16) as u8, (i >> 8) as u8, i as u8];
                outbuf[msglen - 4] = key[0];
                outbuf[msglen - 3] = key[1];
                outbuf[msglen - 2] = key[2];
                outbuf[msglen - 1] = key[3];
                Self::mask_bytes(key, &mut outbuf[msglen..msglen + msgsize])
            } else {
                unsafe {
                    std::ptr::copy(
                        data[i..i + msgsize].as_ptr(),
                        outbuf[msglen..].as_mut_ptr(),
                        msgsize,
                    );
                }
            }

            conn.write((&outbuf[..msglen + msgsize]).to_vec()).await?;
            outbuf[0] = CONTINUATION_FRAMEFINAL_BIT;
            i += msgsize;
        }

        if typ == MessageType::CloseMessage {
            conn.close(Some("websocket close with CloseMessage".to_string()))?;
        }
        Ok(())
    }
    fn mask_bytes(key: [u8; 4], b: &mut [u8]) {
        for (i, v) in b.iter_mut().enumerate() {
            *v ^= key[i & 3]
        }
    }
}
#[derive(IntoPrimitive, Debug, TryFromPrimitive, Clone)]
#[repr(u16)]
enum CloseCode {
    CloseNormalClosure = 1000,
    CloseGoingAway = 1001,
    CloseProtocolError = 1002,
    CloseUnsupportedData = 1003,
    CloseNoStatusReceived = 1005,
    CloseAbnormalClosure = 1006,
    CloseInvalidFramePayloadData = 1007,
    ClosePolicyViolation = 1008,
    CloseMessageTooBig = 1009,
    CloseMandatoryExtension = 1010,
    CloseInternalServerErr = 1011,
    CloseServiceRestart = 1012,
    CloseTryAgainLater = 1013,
    CloseTLSHandshake = 1015,
}
impl CloseCode {
    fn valid_received_close_codes(&self) -> bool {
        match self {
            CloseCode::CloseNormalClosure => true,
            CloseCode::CloseGoingAway => true,
            CloseCode::CloseProtocolError => true,
            CloseCode::CloseUnsupportedData => true,
            CloseCode::CloseNoStatusReceived => false,
            CloseCode::CloseAbnormalClosure => false,
            CloseCode::CloseInvalidFramePayloadData => true,
            CloseCode::ClosePolicyViolation => true,
            CloseCode::CloseMessageTooBig => true,
            CloseCode::CloseMandatoryExtension => true,
            CloseCode::CloseInternalServerErr => true,
            CloseCode::CloseServiceRestart => true,
            CloseCode::CloseTryAgainLater => true,
            CloseCode::CloseTLSHandshake => false,
        }
    }
    fn to_u16(&self) -> u16 {
        let c = self.clone();
        c.into()
    }
}

fn is_valid_received_close_code(code: CloseCode) -> bool {
    let c = code.to_u16();
    return code.valid_received_close_codes() || (c >= 3000 && c <= 4999);
}
