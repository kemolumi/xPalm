mod services;
pub mod consts;
pub mod model;

use std::{ net::{ IpAddr, Ipv4Addr, SocketAddr }, str::FromStr, sync::mpsc };
use colored::Colorize;
use services::{ announcer, local_ip };

use crate::services::instance;

#[tokio::main]
async fn main() {
    let hostname = whoami::hostname().unwrap();

    let (ip_sender, ip_receiver) = mpsc::channel::<String>();
    local_ip::fetch(ip_sender);

    let mut announcer_task: Option<
        tokio::task::JoinHandle<Result<(), std::io::Error>>
    > = None;
    let mut instances_task: Option<
        tokio::task::JoinHandle<Result<(), std::io::Error>>
    > = None;

    let mut current_ip = ip_receiver.recv().unwrap();

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
            "45784".bright_cyan()
        );

        if let Some(task) = announcer_task {
            task.abort();
        }
        if let Some(task) = instances_task {
            task.abort();
        }

        let host_v4 = Ipv4Addr::from_str(&current_ip).unwrap();
        let host_addr = IpAddr::V4(host_v4);

        announcer_task = Some(
            tokio::spawn(
                announcer::start(
                    host_addr.clone(),
                    host_v4.clone(),
                    hostname.clone()
                )
            )
        );

        let manager_target = SocketAddr::new(host_addr, 45784);
        instances_task = Some(
            tokio::spawn(async move {
                instance::launch_main(manager_target).await
            })
        );

        current_ip = ip_receiver.recv().unwrap();
    }
}
