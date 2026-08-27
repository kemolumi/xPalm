use tokio::net::UdpSocket;
use std::{ io, net::{ IpAddr, Ipv4Addr, SocketAddr } };

pub async fn start(
    host_addr: IpAddr,
    host_v4: Ipv4Addr,
    hostname: String
) -> io::Result<()> {
    let multicast_v4 = Ipv4Addr::new(224, 3, 29, 115);
    let multicast_addr = IpAddr::V4(multicast_v4);
    let multicast_target = SocketAddr::new(multicast_addr, 45783);

    let sock = UdpSocket::bind(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 45783)
    ).await?;
    sock.join_multicast_v4(multicast_v4, host_v4).unwrap();

    let mut message = hostname.as_bytes().to_owned();
    message.insert(0, 1);

    loop {
        let mut buf = [0; 1];

        let receiver = if let Ok(result) = sock.recv_from(&mut buf).await {
            result
        } else {
            continue;
        };

        if receiver.1.ip() != host_addr && buf[0] == 0 {
            sock.send_to(&message, multicast_target).await?;
        }
    }
}
