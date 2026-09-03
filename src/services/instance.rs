use colored::Colorize;
use tokio::{
    io::{ AsyncReadExt, AsyncWriteExt },
    net::{
        TcpListener,
        TcpStream,
        UdpSocket,
        tcp::{ OwnedReadHalf, OwnedWriteHalf },
    },
    sync::{ Mutex, mpsc },
};
use std::{ collections::HashMap, io, net::{ IpAddr, SocketAddr }, sync::Arc };

use crate::{
    ServerArgs,
    model::x360,
    utils::udp_joystick::{ UdpJoystick, JoystickStream },
};

pub async fn launch(
    host_target: SocketAddr,
    server_args: ServerArgs
) -> io::Result<()> {
    let sock = TcpListener::bind(host_target).await.unwrap();
    let joystick_controller = UdpJoystick::new(
        UdpSocket::bind(host_target).await.unwrap()
    );

    let mut period: HashMap<IpAddr, tokio::time::Instant> = HashMap::new();

    loop {
        let mut client = sock.accept().await.unwrap();
        client.0.set_nodelay(true).unwrap();

        match period.get(&client.1.ip()) {
            Some(last_join) => {
                match last_join.elapsed() > server_args.connect_cooldown {
                    true => {}
                    false => {
                        println!(
                            "{} {} throttled connection. Rejecting...",
                            ">".red(),
                            client.1.ip().to_string().bright_cyan()
                        );
                        period.insert(
                            client.1.ip(),
                            tokio::time::Instant::now()
                        );
                        client.0.write(&[0]).await.unwrap();
                        client.0.shutdown().await.unwrap();
                        continue;
                    }
                }
            }
            None => {}
        }

        period.insert(client.1.ip(), tokio::time::Instant::now());
        tokio::spawn(
            client_bootstrap(client, server_args, joystick_controller.clone())
        );
    }
}

async fn client_bootstrap(
    (mut stream, addr): (TcpStream, SocketAddr),
    server_args: ServerArgs,
    joystick_controller: UdpJoystick
) {
    let code: u16 = rand::random_range(1454..=9999);
    println!(
        "{} {} authorize code: {}",
        "?".yellow(),
        addr.ip().to_string().bright_cyan(),
        code.to_string().red().bold()
    );

    match authorize_client(addr.ip(), &mut stream, code, server_args).await {
        Ok(_) => {
            println!(
                "{} {} was authorized successfully.",
                ">".green(),
                addr.ip().to_string().bright_cyan()
            );
            if stream.write(&[1]).await.is_err() {
                return;
            }
        }
        Err(_) => {
            let _ = stream.write(&[0]).await;
            let _ = stream.shutdown().await;
            return;
        }
    }

    let joystick_stream = joystick_controller.tune(addr).await;

    client_controller(addr.ip(), stream, joystick_stream, server_args).await;
}

async fn authorize_client(
    ip: IpAddr,
    stream: &mut TcpStream,
    code: u16,
    server_args: ServerArgs
) -> Result<(), ()> {
    let mut status = Ok(());
    tokio::select! {
        _ = tokio::time::sleep(server_args.authorization_period) => {
            println!(
                "{} {} took too long to enter the code.",
                ">".red(),
                ip.to_string().bright_cyan()
            );
            status = Err(());
        }
        _ = async {
            let mut handshake = [0; 3];
            match stream.read(&mut handshake).await {
                Ok(3) => {},
                _ => {
                    println!(
                        "{} {} disconnected.",
                        ">".red(),
                        ip.to_string().bright_cyan()
                    );
                    status = Err(());
                    return;
                },
            }

            match handshake[0] {
                1 => {}
                _ => {
                    println!(
                        "{} Conflicted with {}. Maybe the client is too old?",
                        ">".red(),
                        ip.to_string().bright_cyan()
                    );
                    status = Err(());
                    return;
                }
            }
            
            let response = u16::from_be_bytes(handshake[1..3].try_into().unwrap());

            match code == response {
                true => {},
                false => {
                    println!(
                        "{} {} have entered the wrong code.",
                        ">".red(),
                        ip.to_string().bright_cyan()
                    );
                    status = Err(());
                    return;
                },
            }
        } => {}
    }

    status
}

async fn client_controller(
    ip: IpAddr,
    stream: TcpStream,
    joystick_stream: JoystickStream,
    server_args: ServerArgs
) {
    let (reader, writer) = stream.into_split();
    let (vibrating_tx, vibration_tx) = mpsc::channel(1);

    let controller = match
        x360::Controller::new(server_args.polling_rate, vibrating_tx)
    {
        Ok(controller) => controller,
        Err(_) => {
            println!(
                "{} {} have entered the wrong code.",
                ">".red(),
                ip.to_string().bright_cyan()
            );
            return;
        }
    };

    let controller = Arc::new(controller);
    let writer = Arc::new(Mutex::new(writer));
    let (switch_tx, switch_rx) = mpsc::channel(1);

    tokio::select! {
        _ = vibration_handler(vibration_tx, writer.clone()) => {}
        _ = controller_buttons_handler(reader, writer, controller.clone(), switch_tx) => {}
        _ = udp_joystick_handler(joystick_stream, controller) => {}
        _ = a_fucking_deadman_switch_why_not(switch_rx, server_args) => {}
    }

    println!("{} {} disconnected.", ">".red(), ip.to_string().bright_cyan());
}

async fn vibration_handler(
    mut vibrating_rx: mpsc::Receiver<(u8, u16)>,
    writer: Arc<Mutex<OwnedWriteHalf>>
) {
    while let Some((strength, duration)) = vibrating_rx.recv().await {
        let mut command = [0u8; 4];
        command[0..2].copy_from_slice(&[2, strength]);
        command[2..4].copy_from_slice(&duration.to_be_bytes());
        match writer.lock().await.write(&command).await {
            Ok(4) => {}
            _ => {
                break;
            }
        }
    }
}

async fn controller_buttons_handler(
    mut reader: OwnedReadHalf,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    controller: Arc<x360::Controller>,
    switch_tx: mpsc::Sender<()>
) {
    let mut flag = [0u8; 1];
    let mut action = [0u8; 2];
    let mut joystick_instruction = [0u8; 5];

    while let Ok(1) = reader.read(&mut flag).await {
        match flag[0] {
            2 => {
                match reader.read(&mut action).await {
                    Ok(2) => {}
                    _ => {
                        break;
                    }
                }

                match x360::ControllerButton::from_repr(action[0]) {
                    Some(button) => {
                        controller.button(button, action[1]).await;
                    }
                    None => {}
                }
            }
            3 => {
                match reader.read(&mut action).await {
                    Ok(2) => {}
                    _ => {
                        break;
                    }
                }

                match x360::ControllerTrigger::from_repr(action[0]) {
                    Some(trigger) => {
                        controller.trigger(trigger, action[1]).await;
                    }
                    None => {}
                }
            }
            4 => {
                match reader.read(&mut action).await {
                    Ok(2) => {}
                    _ => {
                        break;
                    }
                }

                match x360::ControllerDpad::from_repr(action[0]) {
                    Some(dpad) => {
                        controller.dpad(dpad, action[1]).await;
                    }
                    None => {}
                }
            }
            5 => {
                match reader.read(&mut joystick_instruction).await {
                    Ok(5) => {}
                    _ => {
                        break;
                    }
                }

                match
                    x360::ControllerJoystick::from_repr(joystick_instruction[0])
                {
                    Some(joystick) => {
                        controller.joystick(
                            joystick,
                            i16::from_be_bytes(
                                joystick_instruction[1..3].try_into().unwrap()
                            ),
                            i16::from_be_bytes(
                                joystick_instruction[3..5].try_into().unwrap()
                            )
                        ).await;
                    }
                    None => {}
                }
            }
            6 => {
                if switch_tx.send(()).await.is_err() {
                    break;
                }
                match writer.lock().await.write(&[6]).await {
                    Ok(1) => {}
                    _ => {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

async fn udp_joystick_handler(
    mut joystick_stream: JoystickStream,
    controller: Arc<x360::Controller>
) {
    let mut joystick_instruction = [0u8; 6];
    while let Some(6) = joystick_stream.read(&mut joystick_instruction).await {
        if joystick_instruction[0] != 5 {
            continue;
        }
        match x360::ControllerJoystick::from_repr(joystick_instruction[1]) {
            Some(joystick) => {
                controller.joystick(
                    joystick,
                    i16::from_be_bytes(
                        joystick_instruction[2..4].try_into().unwrap()
                    ),
                    i16::from_be_bytes(
                        joystick_instruction[4..6].try_into().unwrap()
                    )
                ).await;
            }
            None => {}
        }
    }
}

async fn a_fucking_deadman_switch_why_not(
    mut switch_rx: mpsc::Receiver<()>,
    server_args: ServerArgs
) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(server_args.idle_kick) => {
                break;
            }
            _ = switch_rx.recv() => {}
        }
    }
}
