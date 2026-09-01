mod services;
pub mod consts;
pub mod model;
pub mod utils;

use std::{
    net::{ IpAddr, Ipv4Addr, SocketAddr },
    str::FromStr,
    time::Duration,
};
use colored::Colorize;
use clap::Parser;
use tokio::sync::mpsc;

use crate::services::{ announcer, instance, local_ip };

/// Server software to allows remotely, or locally controlled virtual XBox 360 controller.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct ConsoleArgs {
    /// Port for xPalm. Default: 45784.
    #[arg(short, long)]
    port: Option<u16>,

    /// Polling rate (hz) for xPalm controllers to fetch and emit information. Default: 125 - min/max: 1/1000
    #[arg(short, long)]
    polling_rate: Option<u64>,

    /// How long to wait (ms) for xPalm client to enter authorization code. Default: 30000
    #[arg(short, long)]
    authorization_period: Option<u64>,

    /// Throttle connection request from a client (ms) after an initial. Default: 10000
    #[arg(short, long)]
    connect_cooldown: Option<u64>,

    /// Idle time (ms) for xPalm to kick client. Default: 3000
    #[arg(short, long)]
    idle_kick: Option<u64>,
}

#[derive(Clone, Copy)]
struct ServerArgs {
    port: u16,
    polling_rate: Duration,
    authorization_period: Duration,
    connect_cooldown: Duration,
    idle_kick: Duration,
}

impl ServerArgs {
    fn from_console(args: ConsoleArgs) -> Self {
        ServerArgs {
            port: args.port.unwrap_or(45784),
            polling_rate: Duration::from_millis(
                1000 / args.polling_rate.unwrap_or(125)
            ),
            authorization_period: Duration::from_millis(
                args.authorization_period.unwrap_or(30000)
            ),
            connect_cooldown: Duration::from_millis(
                args.connect_cooldown.unwrap_or(10000)
            ),
            idle_kick: Duration::from_millis(args.idle_kick.unwrap_or(5000)),
        }
    }
}

#[tokio::main]
async fn main() {
    let console_args = ConsoleArgs::parse();

    match console_args.polling_rate {
        Some(rate) => {
            if rate > 1000 || rate < 1 {
                println!("{} Invalid polling rate.", ">".red());
                return;
            }
        }
        None => {}
    }

    let server_args = ServerArgs::from_console(console_args);

    let hostname = whoami::hostname().unwrap();

    let (ip_sender, mut ip_receiver) = mpsc::channel(1);
    local_ip::fetch(ip_sender);

    let mut announcer_task: Option<
        tokio::task::JoinHandle<Result<(), std::io::Error>>
    > = None;
    let mut instance_task: Option<
        tokio::task::JoinHandle<Result<(), std::io::Error>>
    > = None;

    let mut current_ip = ip_receiver.recv().await.unwrap();

    loop {
        println!(
            "{} Binding instance on IP Address: {}",
            ">".green(),
            current_ip.bright_cyan()
        );
        println!(
            "{} Manual connect information | IP: {} | Port: {}",
            ">".green(),
            current_ip.bright_cyan(),
            server_args.port.to_string().bright_cyan()
        );

        if let Some(task) = announcer_task {
            task.abort();
        }
        if let Some(task) = instance_task {
            task.abort();
        }

        let host_v4 = Ipv4Addr::from_str(&current_ip).unwrap();
        let host_addr = IpAddr::V4(host_v4);

        let host_target = SocketAddr::new(host_addr, server_args.port);

        announcer_task = Some(
            tokio::spawn(
                announcer::start(
                    host_target.clone(),
                    host_v4.clone(),
                    hostname.clone()
                )
            )
        );

        instance_task = Some(
            tokio::spawn(instance::launch(host_target, server_args))
        );

        current_ip = ip_receiver.recv().await.unwrap();
    }
}
