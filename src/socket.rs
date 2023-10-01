use crate::proto::{RecvMeta, SocketType, Transmit, UdpCapabilities};
use async_io::Async;
use std::io::{IoSliceMut, Result};
use std::net::SocketAddr;

#[cfg(unix)]
use crate::unix as platform;
#[cfg(not(unix))]
use fallback as platform;

#[derive(Debug)]
pub struct UdpSocket {
    inner: Async<std::net::UdpSocket>,
    ty: SocketType,
}

impl UdpSocket {
    pub fn capabilities() -> Result<UdpCapabilities> {
        Ok(UdpCapabilities {
            max_gso_segments: platform::max_gso_segments()?,
        })
    }

    pub fn bind(addr: SocketAddr) -> Result<Self> {
        let socket = std::net::UdpSocket::bind(addr)?;
        let ty = platform::init(&socket)?;
        Ok(Self {
            inner: Async::new(socket)?,
            ty,
        })
    }

    pub fn socket_type(&self) -> SocketType {
        self.ty
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.inner.get_ref().local_addr()
    }

    pub fn ttl(&self) -> Result<u8> {
        let ttl = self.inner.get_ref().ttl()?;
        Ok(ttl as u8)
    }

    pub fn set_ttl(&self, ttl: u8) -> Result<()> {
        self.inner.get_ref().set_ttl(ttl as u32)
    }

    pub async fn send(&self, transmits: &[Transmit]) -> Result<usize> {
        let mut i = 0;
        while i < transmits.len() {
            self.inner.writable().await?;
            let socket = self.inner.as_ref();
            i += platform::send(socket, &transmits[i..])?;
        }
        Ok(i)
    }

    pub async fn recv(
        &self,
        buffers: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Result<usize> {
        self.inner.readable().await?;
        let socket = self.inner.get_ref();
        platform::recv(socket, buffers, meta)
    }
}

#[cfg(not(unix))]
mod fallback {
    use super::*;

    pub fn max_gso_segments() -> Result<usize> {
        Ok(1)
    }

    pub fn init(socket: &std::net::UdpSocket) -> Result<SocketType> {
        Ok(if socket.local_addr()?.is_ipv4() {
            SocketType::Ipv4
        } else {
            SocketType::Ipv6Only
        })
    }

    pub fn send(socket: &std::net::UdpSocket, transmits: &[Transmit]) -> Result<usize> {
        let mut sent = 0;
        for transmit in transmits {
            match socket.send_to(&transmit.contents, &transmit.destination) {
                Ok(_) => {
                    sent += 1;
                }
                Err(_) if sent != 0 => {
                    // We need to report that some packets were sent in this case, so we rely on
                    // errors being either harmlessly transient (in the case of WouldBlock) or
                    // recurring on the next call.
                    return Ok(sent);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok(sent)
    }

    pub fn recv(
        socket: &std::net::UdpSocket,
        buffers: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Result<usize> {
        let (len, source) = socket.recv_from(&mut buffers[0])?;
        meta[0] = RecvMeta {
            source,
            len,
            ecn: None,
            dst_ip: None,
        };
        Ok(1)
    }
}
