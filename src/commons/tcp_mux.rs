use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use crypto::buffer::{RefReadBuffer, RefWriteBuffer};
use crypto::rc4::Rc4;
use crypto::symmetriccipher::{Decryptor, Encryptor};
use tokio::io::{BufReader, ErrorKind, Result};
use tokio::io::Error;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::prelude::*;
use tokio::prelude::io::AsyncWriteExt;

use crate::commons::{Address, StdResAutoConvert, StdResConvert};

const CONNECT: u8 = 0x00;
const DISCONNECT: u8 = 0x01;
const DATA: u8 = 0x03;

pub enum Msg {
  CONNECT(String, Address),
  DISCONNECT(String),
  DATA(String, Bytes),
}

fn encode(msg: &Msg) -> Bytes {
  match msg {
    Msg::CONNECT(id, addr) => encode_connect_msg(addr, id),
    Msg::DISCONNECT(id) => encode_disconnect_msg(id),
    Msg::DATA(id, data) => encode_data_msg(id, data)
  }
}

fn encode_connect_msg(addr: &Address, channel_id: &str) -> Bytes {
  let mut buff = BytesMut::new();
  buff.put_u8(CONNECT);
  buff.put_slice(channel_id.as_bytes());

  let (host, port) = addr;

  buff.put_slice(host.as_bytes());
  buff.put_u16(port.clone());
  buff.freeze()
}

fn encode_disconnect_msg(channel_id: &str) -> Bytes {
  let mut buff = BytesMut::new();
  buff.put_u8(DISCONNECT);
  buff.put_slice(channel_id.as_bytes());
  buff.freeze()
}

fn encode_data_msg(channel_id: &str, data: &[u8]) -> Bytes {
  let mut buff = BytesMut::new();
  buff.put_u8(DATA);
  buff.put_slice(channel_id.as_bytes());
  buff.put_slice(data);
  buff.freeze()
}

fn decode(mut msg: Bytes) -> Result<Msg> {
  let mode = msg.get_u8();
  let mut str = vec![0u8; 4];
  msg.copy_to_slice(&mut str);
  let channel_id = String::from_utf8(str).res_auto_convert()?;

  let msg = match mode {
    CONNECT => {
      let mut host = vec![0u8; msg.len() - 2];
      msg.copy_to_slice(&mut host);
      let port = msg.get_u16();
      Msg::CONNECT(channel_id, (String::from_utf8(host).res_auto_convert()?, port))
    }
    DISCONNECT => Msg::DISCONNECT(channel_id),
    DATA => Msg::DATA(channel_id, msg),
    _ => return Err(Error::new(ErrorKind::Other, "Message error"))
  };
  Ok(msg)
}

async fn read_msg(rx: &mut BufReader<OwnedReadHalf>) -> Result<Bytes> {
  let len = rx.read_u16().await?;
  let mut msg = vec![0u8; len as usize];
  rx.read_exact(&mut msg).await?;
  Ok(Bytes::from(msg))
}

pub fn create_channel_id() -> String {
  nanoid!(4)
}

#[async_trait]
pub trait MsgWriteHandler {
  async fn write_msg(&mut self, msg: &Msg, rc4: &mut Rc4) -> Result<()>;
}

#[async_trait]
impl MsgWriteHandler for OwnedWriteHalf {
  async fn write_msg(&mut self, msg: &Msg, rc4: &mut Rc4) -> Result<()> {
    let msg = encode(msg);
    let msg = crypto(&msg, rc4, MODE::ENCRYPT)?;

    let mut data = BytesMut::new();
    data.put_u16(msg.len() as u16);
    data.put_slice(&msg);

    self.write_all(&data).await
  }
}

#[async_trait]
pub trait MsgReadHandler {
  async fn read_msg(&mut self, rc4: &mut Rc4) -> Result<Msg>;
}

#[async_trait]
impl MsgReadHandler for BufReader<OwnedReadHalf> {
  async fn read_msg(&mut self, rc4: &mut Rc4) -> Result<Msg> {
    let data = read_msg(self).await?;
    let data = crypto(&data, rc4, MODE::DECRYPT)?;
    decode(Bytes::from(data))
  }
}

enum MODE {
  ENCRYPT,
  DECRYPT,
}

fn crypto<'a>(input: &'a [u8], rc4: &'a mut Rc4, mode: MODE) -> Result<Vec<u8>> {
  let mut ref_read_buf = RefReadBuffer::new(input);
  let mut out = vec![0u8; input.len()];
  let mut ref_write_buf = RefWriteBuffer::new(&mut out);

  match mode {
    MODE::DECRYPT => {
      rc4.decrypt(&mut ref_read_buf, &mut ref_write_buf, false)
        .res_convert(|_| "Decrypt error".to_string())?;
    }
    MODE::ENCRYPT => {
      rc4.encrypt(&mut ref_read_buf, &mut ref_write_buf, false)
        .res_convert(|_| "Encrypt error".to_string())?;
    }
  };
  Ok(out)
}