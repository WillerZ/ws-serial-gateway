# ws-serial-gateway

A tiny **WebSocket‑to‑Serial‑Port gateway** written in Rust.

* A client connects to `ws://<host>:<port>/<endpoint>`
* The gateway opens the serial device configured for `<endpoint>` and
  forwards all data **both ways**.

## Features

| Feature | Description |
| ------- | ----------- |
| Cross‑platform | Works on Windows, Linux and macOS |
| Simple configuration | YAML file mapping endpoint names to serial ports and baud rates |
| MIT licensed | Free to use, modify and distribute |

## Safety and Security

Do not expose this service to untrusted networks.

I am not interested in adding TLS to this software directly. Serve this behind a
reverse proxy (e.g. nginx) that can handle TLS termination, access control, etc.

## Scalability

Each serial port is still a serial port. That means it's inherently a single-
user device, and you can only have one client connected to it at a time. If
another program on your server is accessing the same serial port, do not expect
to be able to connect via this server at the same time.

## Getting started

### Prerequisites

* **Rust toolchain** (stable) – install via <https://rustup.rs>
* A serial device (e.g. Arduino, USB‑UART, etc.)

### Clone the repository

You know what to do.

### Configuration

Edit `config.yaml` in the project root to configure your server endpoint and
serial devices.

### Build and run

```bash
cargo run --release
```

The server will listen on the configured IP and port.  
Open a WebSocket client to `ws://<host>:<port>/mydevice` (replace `mydevice`
with an endpoint you defined).

### Example client (Node.js)

```js
const WebSocket = require('ws');
const ws = new WebSocket('ws://127.0.0.1:4400/mydevice');

ws.binaryType = 'arraybuffer';

ws.on('open', () => {
  console.log('WebSocket opened');
  // Send a command to the serial device
  ws.send(Buffer.from('AT\r\n'));
});

ws.on('message', data => {
  console.log('From serial:', Buffer.from(data).toString());
});
```

### Stopping the server

Press **Ctrl‑C** in the terminal where `cargo run` is executing or send SIGINT
to the process.

## License

This software is licensed under the MIT License – see the `LICENSE` file for details.