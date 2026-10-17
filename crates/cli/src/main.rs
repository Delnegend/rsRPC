#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use clap::Parser;
use rsrpc::{logger::set_logs_enabled, RPCConfig, RPCServer, RpcError};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, conflicts_with = "no_process_scan")]
    detectable_file: Option<PathBuf>,
    #[arg(short, long)]
    no_process_scan: bool,
}

fn build_client(args: &Args) -> Result<RPCServer, RpcError> {
    let config = RPCConfig {
        enable_process_scanner: !args.no_process_scan,
        ..Default::default()
    };

    if args.no_process_scan {
        RPCServer::from_json_str("[]", config)
    } else if let Some(file) = &args.detectable_file {
        RPCServer::from_file(file, config)
    } else {
        let detectable = ureq::get("https://discord.com/api/v9/applications/detectable")
            .call()
            .map_err(|err| RpcError::Io(std::io::Error::other(err.to_string())))?
            .into_body()
            .with_config()
            .limit(32 * 1024 * 1024)
            .read_to_string()
            .map_err(|err| RpcError::Io(std::io::Error::other(err.to_string())))?;
        RPCServer::from_json_str(detectable, config)
    }
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    // When running as a binary, enable logs
    set_logs_enabled(true);

    let args = Args::parse();
    let mut client = build_client(&args)?;

    // Starts the other threads (process detector, client connector, etc)
    client.start();

    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })?;

    println!("Press Ctrl+C to exit");
    let _ = rx.recv();

    println!("Shutting down...");
    drop(client);

    // giving them a bit so they can clean up (e.g. drop BoundListener)
    std::thread::sleep(std::time::Duration::from_millis(100));

    Ok(())
}
