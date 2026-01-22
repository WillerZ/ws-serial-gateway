# ws-serial-gateway

A tiny **WebSocket‑to‑Serial‑Port gateway** written in Rust.

* A client connects to `ws://<host>:9001/<endpoint>`
* The gateway opens the serial device configured for `<endpoint>` and
  forwards all data **both ways**.
* Errors are logged to standard output.
* The program shuts down cleanly on `Ctrl‑C`.

## Features

| Feature | Description |
| ------- | ----------- |
| Cross‑platform | Works on Windows, Linux and macOS |
| Asynchronous I/O | Powered by Tokio for high‑throughput handling |
| Simple configuration | YAML file mapping endpoint names to serial ports and baud rates |
| Graceful shutdown | Handles `SIGINT` (`Ctrl‑C`) and closes all ports cleanly |
| MIT licensed | Free to use, modify and distribute |

## Getting started

### Prerequisites

* **Rust toolchain** (stable) – install via <https://rustup.rs>
* A serial device (e.g. Arduino, USB‑UART, etc.)

### Clone the repository

You know what to do.

### Configuration

Create (or edit) `config.yaml` in the project root:

```yaml
endpoints:
  mydevice:
    port: "/dev/ttyUSB0"   # or "COM4" on Windows
    baud_rate: 115200
```

*The key under `endpoints` (`mydevice` in the example) becomes the WebSocket
path.*

### Build and run

```bash
cargo run --release
```

The server will listen on `0.0.0.0:9001`.  
Open a WebSocket client to `ws://localhost:9001/mydevice` (replace `mydevice`
with the endpoint you defined).

### Example client (Node.js)

```js
const WebSocket = require('ws');
const ws = new WebSocket('ws://localhost:9001/mydevice');

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

Press **Ctrl‑C** in the terminal where `cargo run` is executing. The gateway
will close all open serial ports and exit cleanly.

## License

This project is licensed under the MIT License – see the `LICENSE` file for details.