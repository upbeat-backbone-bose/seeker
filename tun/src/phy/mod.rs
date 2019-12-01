use std::io;
use std::task::{Context, Poll};

use async_std::net::driver::Watcher;

mod sys;

pub(crate) struct TunSocket {
    mtu: usize,
    name: String,
    watcher: Watcher<sys::TunSocket>,
}

#[allow(dead_code)]
impl TunSocket {
    pub fn new(name: &str) -> TunSocket {
        let watcher = Watcher::new(sys::TunSocket::new(name).expect("TunSocket::new"));
        TunSocket {
            name: watcher.get_ref().name().expect("get name"),
            mtu: watcher.get_ref().mtu().expect("get mut"),
            watcher,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mtu(&self) -> usize {
        self.mtu
    }

    pub fn poll_recvmsg(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        self.watcher.poll_read_with(cx, |inner| inner.recvmsg(buf))
    }

    pub fn poll_recvmmsg(
        &self,
        cx: &mut Context<'_>,
        bufs: &[&mut [u8]],
    ) -> Poll<io::Result<Vec<usize>>> {
        self.watcher
            .poll_read_with(cx, |inner| inner.recvmmsg(bufs))
    }

    pub fn poll_sendmsg(&self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.watcher.poll_write_with(cx, |inner| inner.sendmsg(buf))
    }

    pub fn poll_sendmmsg(
        &self,
        cx: &mut Context<'_>,
        bufs: &[&[u8]],
    ) -> Poll<io::Result<Vec<usize>>> {
        self.watcher
            .poll_write_with(cx, |inner| inner.sendmmsg(bufs))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use async_std::future;
    use async_std::io::timeout;
    use async_std::net::UdpSocket;
    use async_std::task;
    use async_std::task::block_on;
    use smoltcp::phy::ChecksumCapabilities;
    use smoltcp::wire::*;

    use sysconfig::setup_ip;

    use super::*;

    #[test]
    fn test_recvmsg() {
        let tun_socket = TunSocket::new("utun");
        let tun_name = tun_socket.name();
        if cfg!(target_os = "macos") {
            setup_ip(tun_name, "10.0.1.1", "10.0.1.0/24");
        } else {
            setup_ip(tun_name, "10.0.1.1/24", "10.0.1.0/24");
        }

        const DATA: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8];

        block_on(async move {
            task::spawn(async {
                task::sleep(Duration::from_secs(1)).await;
                let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
                socket.send_to(&DATA, ("10.0.1.2", 9090)).await.unwrap();
            });

            let (buf, size) = loop {
                let mut buf = vec![0; 1024];
                let size = future::poll_fn(|cx| tun_socket.poll_recvmsg(cx, &mut buf))
                    .await
                    .unwrap();
                // process ipv4 only
                if size > 0 && IpVersion::of_packet(&buf[..size]).unwrap() == IpVersion::Ipv4 {
                    break (buf, size);
                }
            };
            let ipv4_packet = Ipv4Packet::new_unchecked(&buf[..size]);
            let ipv4_repr =
                Ipv4Repr::parse(&ipv4_packet, &ChecksumCapabilities::default()).unwrap();
            assert_eq!(ipv4_repr.protocol, IpProtocol::Udp);
            let udp_packet = UdpPacket::new_unchecked(&buf[ipv4_repr.buffer_len()..size]);
            let udp_repr = UdpRepr::parse(
                &udp_packet,
                &ipv4_repr.src_addr.into(),
                &ipv4_repr.dst_addr.into(),
                &ChecksumCapabilities::default(),
            )
            .unwrap();
            assert_eq!(udp_repr.dst_port, 9090);
            assert_eq!(udp_repr.payload, DATA);
        })
    }

    #[test]
    fn test_recvmmsg() {
        let tun_socket = TunSocket::new("utun");
        let tun_name = tun_socket.name();
        if cfg!(target_os = "macos") {
            setup_ip(tun_name, "10.0.3.1", "10.0.3.0/24");
        } else {
            setup_ip(tun_name, "10.0.3.1/24", "10.0.3.0/24");
        }

        const DATA: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8];

        block_on(async move {
            task::spawn(async {
                task::sleep(Duration::from_secs(1)).await;
                let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
                socket.send_to(&DATA, ("10.0.3.2", 9090)).await.unwrap();
            });

            let (buf, size) = loop {
                let mut buf = vec![0; 1024];
                let sizes = future::poll_fn(|cx| tun_socket.poll_recvmmsg(cx, &[&mut buf]))
                    .await
                    .unwrap();
                // process ipv4 only
                if !sizes.is_empty()
                    && IpVersion::of_packet(&buf[..sizes[0]]).unwrap() == IpVersion::Ipv4
                {
                    break (buf, sizes[0]);
                }
            };
            let ipv4_packet = Ipv4Packet::new_unchecked(&buf[..size]);
            let ipv4_repr =
                Ipv4Repr::parse(&ipv4_packet, &ChecksumCapabilities::default()).unwrap();
            assert_eq!(ipv4_repr.protocol, IpProtocol::Udp);
            let udp_packet = UdpPacket::new_unchecked(&buf[ipv4_repr.buffer_len()..size]);
            let udp_repr = UdpRepr::parse(
                &udp_packet,
                &ipv4_repr.src_addr.into(),
                &ipv4_repr.dst_addr.into(),
                &ChecksumCapabilities::default(),
            )
            .unwrap();
            assert_eq!(udp_repr.dst_port, 9090);
            assert_eq!(udp_repr.payload, DATA);
        })
    }

    #[test]
    fn test_sendmsg() {
        let tun_socket = TunSocket::new("utun");
        let tun_name = tun_socket.name();
        if cfg!(target_os = "macos") {
            setup_ip(tun_name, "10.0.2.1", "10.0.2.0/24");
        } else {
            setup_ip(tun_name, "10.0.2.1/24", "10.0.2.0/24");
        }

        let data = "hello".as_bytes();

        block_on(async move {
            let socket = UdpSocket::bind("0.0.0.0:1234").await.unwrap();
            let handle = task::spawn(async move {
                let mut buf = vec![0; 1000];
                timeout(Duration::from_secs(10), socket.recv_from(&mut buf)).await
            });
            task::sleep(Duration::from_secs(1)).await;

            let src_addr = Ipv4Address::new(10, 0, 2, 10);
            let dst_addr = Ipv4Address::new(10, 0, 2, 1);
            let udp_repr = UdpRepr {
                src_port: 1234,
                dst_port: 1234,
                payload: &data,
            };
            let mut udp_buf = vec![0; udp_repr.buffer_len()];
            let mut udp_packet = UdpPacket::new_unchecked(&mut udp_buf);
            udp_repr.emit(
                &mut udp_packet,
                &src_addr.into(),
                &dst_addr.into(),
                &ChecksumCapabilities::default(),
            );
            let ip_repr = Ipv4Repr {
                src_addr,
                dst_addr,
                protocol: IpProtocol::Udp,
                payload_len: udp_packet.len() as usize,
                hop_limit: 64,
            };
            let mut ip_buf = vec![0; ip_repr.buffer_len() + ip_repr.payload_len];
            let mut ip_packet = Ipv4Packet::new_unchecked(&mut ip_buf);
            ip_repr.emit(&mut ip_packet, &ChecksumCapabilities::default());
            ip_buf[ip_repr.buffer_len()..].copy_from_slice(&udp_buf);
            let size = future::poll_fn(|cx| tun_socket.poll_sendmsg(cx, &ip_buf))
                .await
                .unwrap();
            assert_eq!(size, ip_buf.len());
            let (s, _src) = handle.await.unwrap();
            assert_eq!(data.len(), s);
        })
    }

    #[test]
    fn test_sendmmsg() {
        let tun_socket = TunSocket::new("utun");
        let tun_name = tun_socket.name();
        if cfg!(target_os = "macos") {
            setup_ip(tun_name, "10.0.4.1", "10.0.4.0/24");
        } else {
            setup_ip(tun_name, "10.0.4.1/24", "10.0.4.0/24");
        }

        let data = "hello".as_bytes();

        block_on(async move {
            let port = 1235;
            let socket = UdpSocket::bind(format!("0.0.0.0:{}", port)).await.unwrap();
            let handle = task::spawn(async move {
                let mut buf = vec![0; 1000];
                timeout(Duration::from_secs(10), socket.recv_from(&mut buf)).await
            });
            task::sleep(Duration::from_secs(1)).await;

            let src_addr = Ipv4Address::new(10, 0, 4, 10);
            let dst_addr = Ipv4Address::new(10, 0, 4, 1);
            let udp_repr = UdpRepr {
                src_port: port,
                dst_port: port,
                payload: &data,
            };
            let mut udp_buf = vec![0; udp_repr.buffer_len()];
            let mut udp_packet = UdpPacket::new_unchecked(&mut udp_buf);
            udp_repr.emit(
                &mut udp_packet,
                &src_addr.into(),
                &dst_addr.into(),
                &ChecksumCapabilities::default(),
            );
            let ip_repr = Ipv4Repr {
                src_addr,
                dst_addr,
                protocol: IpProtocol::Udp,
                payload_len: udp_packet.len() as usize,
                hop_limit: 64,
            };
            let mut ip_buf = vec![0; ip_repr.buffer_len() + ip_repr.payload_len];
            let mut ip_packet = Ipv4Packet::new_unchecked(&mut ip_buf);
            ip_repr.emit(&mut ip_packet, &ChecksumCapabilities::default());
            ip_buf[ip_repr.buffer_len()..].copy_from_slice(&udp_buf);
            let sizes = future::poll_fn(|cx| tun_socket.poll_sendmmsg(cx, &[&ip_buf]))
                .await
                .unwrap();
            assert_eq!(sizes[0], ip_buf.len());
            let (s, _src) = handle.await.unwrap();
            assert_eq!(data.len(), s);
        })
    }
}
