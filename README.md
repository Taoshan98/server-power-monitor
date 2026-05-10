# 🔌 Server Power Monitor

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Bash](https://img.shields.io/badge/Language-Bash-4EAA25.svg)](https://www.gnu.org/software/bash/)
[![Docker](https://img.shields.io/badge/Docker-Ready-2496ED.svg)](https://www.docker.com/)
[![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey.svg)](https://www.linux.org/)

A professional, lightweight telemetry system for real-time power monitoring on Linux servers. Track CPU, GPU, and Disk energy consumption with beautiful terminal dashboards and automated Telegram reporting.

---

## 📸 Preview

> [!TIP]
> *Add a screenshot or a GIF of your terminal here to show off the real-time dashboard!*

---

## ✨ Key Features

- **🛡️ Comprehensive Monitoring**: 
    - **Intel RAPL**: Precise measurements for Package (SoC), Cores, iGPU, and DRAM.
    - **NVIDIA GPU**: Real-time power draw via `nvidia-smi` integration.
    - **Storage Estimation**: Hybrid model for HDDs (standby/active detection) and SSDs (I/O stats based estimation).
- **📊 Professional Dashboard**: 
    - Color-coded terminal output with intuitive hardware icons.
    - Smart sensor deduplication (handles multiple kernel interfaces cleanly).
    - Power source detection (🔌 AC vs 🔋 Battery) with percentage tracking.
- **📱 Telegram Integration**:
    - **Periodic Updates**: Configurable status reports (e.g., every 1h, 6h, etc.).
    - **Daily Summaries**: Complete daily energy consumption (kWh) and cost estimation.
- **🐳 Container-First Design**: 
    - Optimized **Debian Slim** Docker image (~30MB).
    - Hardware passthrough support for both Intel and NVIDIA.
- **⚡ Performance Focused**: Ultra-low overhead, written in pure Bash and Awk.

---

## 🚀 Quick Start

### Option A: Docker (Recommended)
The fastest way to deploy, especially if you have an NVIDIA GPU.

1. **Clone the repo**:
   ```bash
   git clone https://github.com/yourusername/server-power-monitor.git
   cd server-power-monitor
   ```
2. **Configure**:
   ```bash
   cp server-power-monitor.conf.example server-power-monitor.conf
   nano server-power-monitor.conf # Add your Telegram credentials
   ```
3. **Launch**:
   ```bash
   docker-compose up -d
   ```

### Option B: Native Installation
Ideal for bare-metal servers or lightweight environments.

```bash
bash setup_service.sh
```

---

## ⚙️ Configuration Reference

| Parameter | Description | Default |
|:----------|:------------|:--------|
| `SAMPLE_INTERVAL` | Seconds between power samples | `5` |
| `TARIFF_EUR_KWH` | Electricity cost per kWh | `0.30` |
| `CURRENCY` | Currency symbol for reports | `EUR` |
| `TELEGRAM_REPORT_INTERVAL_HOURS` | Frequency of intermediate Telegram reports | `6` |
| `HDD_ACTIVE_W` | Estimated consumption for an active HDD | `5.0` |
| `SSD_ACTIVE_W` | Estimated consumption for an active SSD | `2.5` |

---

## 🛠️ Requirements

- **Kernel**: Linux 5.0+ with `intel-rapl` enabled.
- **Hardware**: Intel CPU (Sandy Bridge or newer) or NVIDIA GPU.
- **Packages**: `bash`, `gawk`, `curl`, `hdparm`.
- **Permissions**: Root/Sudo access (required for hardware register access).

---

## 📖 How it Works

The system interfaces directly with the **Intel Running Average Power Limit (RAPL)** driver through the Linux `powercap` interface. It reads cumulative energy counters in micro-joules and calculates the instantaneous wattage based on the time delta between samples. For storage devices, it uses a state-machine that monitors rotational status and I/O traffic to apply pre-defined power profiles.

---

## 🗺️ Roadmap & Next Steps

Future features planned for development:

- **🌐 Centralized Dashboard**: Ability to send telemetry from multiple nodes to a central aggregator for unified monitoring.
- **🍎 Apple Silicon Support**: Extend monitoring to M1/M2/M3 chips (macOS support).
- **🔴 AMD Hardware**: Native support for AMD Zen CPUs and Radeon GPUs (via `amdgpu` and `amd_energy`).
- **📈 Advanced Integrations**: 
    - Native **Prometheus Exporter** for Grafana visualization.
    - **Home Assistant (MQTT)** integration for smart home energy tracking.
- **🔔 Threshold Alerts**: Customizable Telegram alerts when power draw exceeds a specific limit.
- **🖥️ Web UI**: A minimal, built-in local web interface for real-time graphs.

---

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.


---

## ☕ Support

If you find this project useful, consider giving it a ⭐ on GitHub!
