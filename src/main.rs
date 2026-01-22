// src/main.rs
use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use log::{error, info};
use serde::Deserialize;
use serialport::SerialPort;
use std::{
    collections::HashMap, io::{Read, Write}, sync::Arc, time::Duration
};
use tokio::{
    net::TcpListener,
    signal,
    sync::{mpsc, Mutex},
};
use tokio_tungstenite::tungstenite::{Message, http};

// Helper functions that provide defaults for the config fields.
fn default_bind_address() -> String {
    "0.0.0.0".to_string()
}
fn default_bind_port() -> u16 {
    9001
}

/// Configuration file format (YAML)
#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_bind_address")]
    bind_address: String,
    #[serde(default = "default_bind_port")]
    bind_port: u16,
    /// Mapping from a WebSocket endpoint name (used in the URL path) to a serial port description.
    endpoints: HashMap<String, SerialConfig>,
}

#[derive(Debug, Deserialize, Clone)]
struct SerialConfig {
    /// OS device name, e.g. `/dev/ttyUSB0` or `COM3`
    port: String,
    /// Baud rate, e.g. 115200
    baud_rate: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise logger (writes to stdout)
    env_logger::init();

    // Load configuration (default file name `config.yaml`)
    let cfg: Config = {
        let cfg_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.yaml".to_string());
        let txt = std::fs::read_to_string(&cfg_path)
            .with_context(|| format!("Failed to read config file {}", cfg_path))?;
        serde_yaml::from_str(&txt).with_context(|| "Failed to parse YAML config")?
    };
    info!("Configuration loaded: {} endpoints", cfg.endpoints.len());

    // Shared map of endpoint -> SerialConfig (Arc for cheap cloning into tasks)
    let cfg = Arc::new(cfg);

    // Bind a TCP listener – we’ll serve all WebSocket endpoints on the same port.
    let addr = format!("{}:{}", cfg.bind_address, cfg.bind_port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind TCP listener on {}", addr))?;
    info!("Listening for WebSocket connections on ws://{}/<endpoint>", addr);

    // Signal handling – when Ctrl‑C is received we break the accept loop.
    let shutdown_signal = async {
        signal::ctrl_c().await.expect("Failed to listen for ctrl_c");
        info!("Ctrl‑C received, shutting down");
    };

    // Accept loop (runs until shutdown signal)
    tokio::select! {
        _ = accept_loop(listener, cfg.clone()) => {},
        _ = shutdown_signal => {},
    }

    Ok(())
}

/// Accept incoming TCP connections, upgrade them to WebSocket and hand them to `handle_connection`.
async fn accept_loop(listener: TcpListener, cfg: Arc<Config>) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                // Clone config reference for each connection
                let cfg = cfg.clone();

                // Spawn a task to handle the connection independently
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, cfg).await {
                        error!("Connection handling error: {:#}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept TCP connection: {:#}", e);
                // Small delay to avoid busy‑looping on repeated accept failures
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Upgrade a raw TCP stream to a WebSocket, parse the URL path to decide which serial
/// port to open, then forward traffic bi‑directionally.
async fn handle_connection(
    raw_stream: tokio::net::TcpStream,
    cfg: Arc<Config>,
) -> Result<()> {
    let mut port_cfg: Option<&SerialConfig> = None;
    let mut ws_endpoint: String = "".to_string();
    // Perform the WebSocket handshake – we need the request URI to know the endpoint name.
    let ws_stream = {
        let cb = |req: &http::Request<()>, resp: http::Response<()>| {
        // Extract the request path (e.g. "/mydevice")
        ws_endpoint = req.uri().path().trim_start_matches('/').to_string();
        port_cfg = cfg
            .endpoints
            .get(ws_endpoint.as_str());
        if port_cfg.is_none() {
            return Ok(http::Response::builder().status(404).body(()).unwrap());// 404 Not Found
        }
        return Ok(resp);
    };
        tokio_tungstenite::accept_hdr_async(raw_stream, cb)
    .await
    .context("WebSocket handshake failed")?};

    // Look up the serial configuration for this endpoint.
    let serial_cfg = port_cfg.unwrap()
        .clone();

    // Open the serial port (blocking call – run in a dedicated thread via spawn_blocking).
    let serial_port = tokio::task::spawn_blocking(move || {
        serialport::new(&serial_cfg.port, serial_cfg.baud_rate)
            .timeout(Duration::from_secs(10))
            .open()
            .with_context(|| format!("Failed to open serial port {}", &serial_cfg.port))
    })
    .await??; // Propagate any errors from the blocking task.

    info!(
        "Serial port `{}` opened at {} baud for endpoint `{}`",
        &port_cfg.unwrap().port, serial_cfg.baud_rate, ws_endpoint
    );

    // Split the WebSocket into a sender and receiver.
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Channel used to forward data read from the serial port to the WebSocket task.
    let (serial_to_ws_tx, mut serial_to_ws_rx) = mpsc::unbounded_channel::<Message>();

    // Wrap the serial port in an Arc<Mutex<>> so both tasks can use it.
    let mutexed_serial_port = Arc::new(Mutex::new(serial_port));

    // ---------- Task: read from serial, send to WebSocket ----------
    let serial_reader = {
        let readable_serial_port = Arc::clone(&mutexed_serial_port);
        let tx = serial_to_ws_tx.clone();
        tokio::spawn(async move {
            // We use a small buffer and block on the read in a blocking thread.
            let mut buf = [0u8; 1024];
            let readable_serial_port = readable_serial_port;
            loop {
                // Read from serial in a blocking fashion.
                let read_result= {
                    let mut ser = readable_serial_port.lock().await;
                    let ser = ser.as_mut();
                    ser.read(&mut buf)
                };
                let n = match read_result
                {
                    Ok(cnt) => cnt,
                    Err(e) => {
                        error!("Serial read error: {:#}", e);
                        break;
                    }
                };
                if n == 0 {
                    // EOF (should not normally happen on serial ports)
                    continue;
                }
                // Forward the bytes as a binary WebSocket message.
                if tx.send(Message::Binary(buf[..n].to_vec())).is_err() {
                    // Receiver has been dropped – connection closed.
                    break;
                }
            }
        })
    };

    // ---------- Task: write to serial from incoming WebSocket messages ----------
    let serial_writer = {
        let writable_serial_port = Arc::clone(&mutexed_serial_port);
        let writer_endpoint = ws_endpoint.clone();
        tokio::spawn(async move {
            while let Some(msg) = ws_rx.next().await {
                match msg {
                    Ok(Message::Binary(bytes)) => {
                        let write_result = {
                            let mut ser = writable_serial_port.lock().await;
                            let ser = ser.as_mut();
                            let data = bytes.clone();
                            ser.write_all(&data)
                        };
                        if write_result.is_err() {
                            // Error writing to serial port.
                            break;
                        }
                    }
                    Ok(Message::Text(text)) => {
                        // Convert text messages to bytes if needed.
                        let bytes = text.into_bytes();
                        // Write to serial (blocking)
                        let write_result = {
                            let mut ser = writable_serial_port.lock().await;
                            let ser = ser.as_mut();
                            let data = bytes.clone();
                            ser.write_all(&data)
                        };
                        if write_result.is_err() {
                            // Error writing to serial port.
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        info!("WebSocket client closed connection for `{}`", writer_endpoint);
                        break;
                    }
                    Ok(_) => {
                        // Ping/Pong/etc. are ignored.
                    }
                    Err(e) => {
                        error!("WebSocket receive error: {:#}", e);
                        break;
                    }
                }
            }
        })
    };

    // ---------- Forward data from serial_to_ws_rx to the WebSocket ----------
    while let Some(msg) = serial_to_ws_rx.recv().await {
        if let Err(e) = ws_tx.send(msg).await {
            error!("Failed to send data to WebSocket: {:#}", e);
            break;
        }
    }

    // When the forwarding loop ends, make sure the background tasks are shut down.
    serial_reader.abort();
    serial_writer.abort();

    info!("Connection for endpoint `{}` terminated", ws_endpoint);
    Ok(())
}