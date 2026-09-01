use std::{ collections::HashMap, net::{ IpAddr, SocketAddr }, sync::Arc };

use tokio::{ io, net::UdpSocket, sync::{ RwLock, mpsc } };

#[derive(Clone)]
pub struct UdpJoystick {
    channels: Arc<
        RwLock<HashMap<IpAddr, mpsc::UnboundedSender<(usize, [u8; 6])>>>
    >,
    socket: Arc<UdpSocket>,
}

impl UdpJoystick {
    pub fn new(socket: UdpSocket) -> Self {
        let socket = Arc::new(socket);
        let channels = Arc::new(RwLock::new(HashMap::new()));

        tokio::task::spawn(
            UdpJoystick::read_handler(socket.clone(), channels.clone())
        );

        UdpJoystick { socket, channels }
    }

    async fn read_handler(
        socket: Arc<UdpSocket>,
        channels: Arc<
            RwLock<HashMap<IpAddr, mpsc::UnboundedSender<(usize, [u8; 6])>>>
        >
    ) {
        let mut joystick = [0u8; 6];
        while let Ok((size, source)) = socket.recv_from(&mut joystick).await {
            match channels.read().await.get(&source.ip()) {
                Some(stream) => {
                    let mut message = [0u8; 6];
                    message.copy_from_slice(&joystick);

                    if stream.send((size, message)).is_err() {
                        break;
                    }
                }
                None => {}
            }
        }
    }

    pub async fn tune(&self, source: SocketAddr) -> JoystickStream {
        let (bridge_tx, bridge_rx) = mpsc::unbounded_channel();
        self.channels.write().await.insert(source.ip(), bridge_tx);

        JoystickStream {
            source,
            bridge_rx,
            socket: self.socket.clone(),
        }
    }
}

pub struct JoystickStream {
    source: SocketAddr,
    bridge_rx: mpsc::UnboundedReceiver<(usize, [u8; 6])>,
    socket: Arc<UdpSocket>,
}

impl JoystickStream {
    pub async fn write(&self, buffer: &[u8]) -> io::Result<usize> {
        self.socket.send_to(buffer, self.source).await
    }

    pub async fn read(&mut self, buffer: &mut [u8; 6]) -> Option<usize> {
        let (size, data) = self.bridge_rx.recv().await?;
        buffer.copy_from_slice(&data);
        Some(size)
    }
}
