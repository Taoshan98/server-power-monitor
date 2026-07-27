# 🔌 Server Power Monitor (Rust Edition)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust_2021-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Async-Tokio-blue.svg)](https://tokio.rs/)
[![MQTT](https://img.shields.io/badge/Integration-Home--Assistant-41BDF5.svg)](https://www.home-assistant.io/)
[![P2P](https://img.shields.io/badge/Cluster-P2P_Mesh-purple.svg)](https://github.com/taoshan98/server-power-monitor)
[![Docker](https://img.shields.io/badge/Docker-Multi--stage-2496ED.svg)](https://www.docker.com/)

A real-time telemetry and power consumption monitoring tool for Linux servers, written entirely in **Rust** for maximum efficiency, featuring an **Encrypted P2P Cluster Mesh**, native **MQTT Home Assistant Auto-Discovery**, and **Telegram reporting**.

---

## 🌟 Key Features

- **🔒 Zero-Config Encrypted P2P Cluster Mesh (ChaCha20-Poly1305)**: Connect $N$ local and remote Linux servers into an authenticated P2P mesh network. Nodes auto-discover across subnets via **Multi-Interface Broadcast** and propagate endpoints across WAN via **Peer Exchange (PEX)** with zero manual IP setup.
- **🏠 Home Assistant Auto-Discovery**: Automatically publishes telemetry configurations under `homeassistant/sensor/.../config` over MQTT. Instantly creates Home Assistant entities for CPU, GPU, Disk, System Wattage, Daily kWh, and Lifetime kWh.
- **🏛️ Indestructible Lifetime Software Counter**: Preserves absolute cumulative energy history (`lifetime_base.env`) across application restarts and retention prunes.
- **📊 Fullscreen Live TUI Dashboard (Style 3)**: Non-flickering terminal UI with ASCII progress bars, component load distribution, real-time 35-sample ASCII **Sparkline graph**, and dual footer (Today vs Lifetime).
- **🚨 Power Peak Telegram Alerts**: Instant alert notifications sent to Telegram when instant power consumption exceeds a configurable Wattage threshold.
- **🧹 Retention Policy & CSV Export**: Automatic pruning of historical `.env` daily logs while accumulating total energy into the lifetime counter, alongside automated `history.csv` export.
- **🎓 Educational Rust Codebase**: Heavily commented in Italian and structured modularly for developers learning Rust.

---

## 🌐 Network Architecture: P2P Mesh & Home Assistant Bridge

```text
 💻 Remote Server (Cloud/WAN)  ───[ Encrypted P2P Mesh ]───┐
                                                            │
 💻 Laptop (On the move)       ───[ Encrypted P2P Mesh ]───┼──> 🖥️ Local Home Server (P2P + MQTT Bridge) ──> 🏠 Home Assistant
                                                            │
 💻 Local PC (LAN)             ───[ Local Broadcast ]──────┘
```

### Technical P2P Protocol Details

1. **Authenticated Encryption (AEAD)**: Payload serialization uses `bincode` encrypted with **ChaCha20-Poly1305** using a 256-bit key derived from `CLUSTER_SECRET`. Unauthenticated packets are discarded at zero cost.
2. **Multi-Interface Broadcast**: Scans physical and virtual interfaces (Wi-Fi, Ethernet, Docker bridges) to send subnet discovery packets without binding to a single default route.
3. **Peer Exchange (PEX)**: Telemetry packets contain a list of active `known_peers`. Once any node connects to a peer, the entire cluster auto-discovers without static WAN IP configuration.

---

## 🖥️ Live Terminal Dashboard Preview

```text
 🔌 SERVER POWER MONITOR  •  Host: NTM-PC  [🔌 (96%)] [22:08:00]
 ────────────────────────────────────────────────────────────────────────────
 COMPONENT              POWER     VISUAL LOAD               SHARE
  🔳 CPU Package          16.6 W   [████████░░░░░░░░]        31.2%
  💻 System                5.6 W   [██░░░░░░░░░░░░░░]        10.5%
  🎨 GPU (NVIDIA)          7.1 W   [███░░░░░░░░░░░░░]        13.3%
  📀 SSD nvme0n1           0.3 W   [█░░░░░░░░░░░░░░░]         0.5%
  📀 SSD nvme1n1           2.5 W   [█░░░░░░░░░░░░░░░]         4.7%

 🌐 CLUSTER MESH (3 Active Nodes)
  ├ 🖥️ NTM-PC (Local)    :    54.1 W │  0.0012 kWh
  ├ 🖥️ Server-NAS (P2P)   :    28.0 W │  0.1420 kWh (Lifetime: 12.4000 kWh)
  └ 🖥️ Rig-AI (P2P)       :   210.0 W │  1.2400 kWh (Lifetime: 98.2000 kWh)
  ⚡ TOTAL CLUSTER POWER : 292.1 W (Today: 1.3832 kWh)

 📈 LOCAL POWER TREND (Last 35 samples)
  60.0W ┤ ▂▃▅█▇▆▅▄▃▂ ▂▃▄▅▆▇█  (Current: 54.1W)
 ────────────────────────────────────────────────────────────────────────────
  ⚡ POWER:    54.1 W  │  📊 TODAY: 0.0012 kWh (0.0004 EUR)
  🏛️  LIFETIME COUNTER: 16.3890 kWh (4.9167 EUR)
```

---

## ⚙️ Configuration Reference

Configuration parameters can be set in `server-power-monitor.conf` or overridden via environment variables:

| Parameter | Description | Default |
|:----------|:------------|:--------|
| `SAMPLE_INTERVAL` | Sampling interval in seconds | `5` |
| `TARIFF_EUR_KWH` | Electricity tariff per kWh | `0.30` |
| `CURRENCY` | Currency symbol in reports | `EUR` |
| `HOST_LABEL` | Machine identifier (supports `$(hostname)`) | `$(hostname)` |
| `TELEGRAM_ENABLED` | Enable (1) or disable (0) Telegram bot | `0` |
| `TELEGRAM_BOT_TOKEN` | Telegram bot token from BotFather | `""` |
| `TELEGRAM_CHAT_ID` | Telegram chat ID for notifications | `""` |
| `MQTT_ENABLED` | Enable (1) or disable (0) Home Assistant MQTT | `0` |
| `MQTT_HOST` | MQTT broker hostname or IP address | `localhost` |
| `MQTT_PORT` | MQTT broker port | `1883` |
| `P2P_ENABLED` | Enable (1) or disable (0) P2P Cluster Mesh | `0` |
| `CLUSTER_SECRET` | Shared secret key for ChaCha20-Poly1305 P2P encryption | `""` |
| `P2P_PORT` | UDP port for P2P mesh communication | `7432` |
| `P2P_PEERS` | Optional static peer list (`"IP:PORT IP2:PORT"`) | `""` |
| `RETENTION_DAYS` | Daily log retention limit in days (`0` = forever) | `365` |
| `MAX_POWER_ALERT_WATTS` | Instant power threshold for Telegram alerts (`0` = off) | `0` |
| `CSV_EXPORT_ENABLED` | Enable (1) or disable (0) `history.csv` export | `1` |

---

## 🚀 Quick Start Guide

### Native Build & Run

Ensure Rust 1.80+ is installed on your system:

```bash
cargo build --release
sudo ./target/release/server-power-monitor
```

> **Note**: `sudo` privileges are required to access Intel RAPL sysfs files (`/sys/class/powercap/intel-rapl*/energy_uj`).

### Docker Deployment

Use the multi-stage Docker setup:

```bash
# 1. Copy example configuration
cp server-power-monitor.conf.example server-power-monitor.conf

# 2. Build and start the container
docker-compose up -d --build
```

---

## 🧪 Testing

Run the automated test suite covering configuration parsing, RAPL math, P2P encryption/decryption, and TUI chart generation:

```bash
cargo test
```

---

## 📄 License

Distributed under the [MIT License](LICENSE).
