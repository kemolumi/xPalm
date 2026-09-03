use tokio::net::UdpSocket;
use std::{ io, net::{ IpAddr, Ipv4Addr, SocketAddr } };

pub async fn start(
    host_target: SocketAddr,
    host_v4: Ipv4Addr,
    hostname: String
) -> io::Result<()> {
    let multicast_v4 = Ipv4Addr::new(224, 3, 29, 115);
    let multicast_addr = IpAddr::V4(multicast_v4);
    let multicast_target = SocketAddr::new(multicast_addr, 45783);

    let sock = UdpSocket::bind(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 45783)
    ).await?;
    sock.join_multicast_v4(multicast_v4, host_v4)?;

    let message = vec![
        vec![1],
        host_target.port().to_be_bytes().to_vec(),
        hostname.as_bytes().to_owned()
    ].concat();

    let mut buf = [0; 1];
    loop {
        let client = match sock.recv_from(&mut buf).await {
            Ok((1, client)) => client,
            _ => {
                continue;
            }
        };

        if client.ip() != host_target.ip() && buf[0] == 0 {
            sock.send_to(&message, multicast_target).await?;
        }
    }
}
