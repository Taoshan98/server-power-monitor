# 🔌 Server Power Monitor (Rust Edition)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust_2021-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Async-Tokio-blue.svg)](https://tokio.rs/)
[![MQTT](https://img.shields.io/badge/Integration-Home--Assistant-41BDF5.svg)](https://www.home-assistant.io/)
[![P2P](https://img.shields.io/badge/Cluster-P2P_Mesh-purple.svg)](https://github.com/taoshan98/server-power-monitor)
[![Docker](https://img.shields.io/badge/Docker-Multi--stage-2496ED.svg)](https://www.docker.com/)

Un sistema di telemetria e monitoraggio in tempo reale del consumo energetico per server Linux, riscritto interamente in **Rust** per garantire massima efficienza, rete **P2P Mesh Cifrata**, integrazione nativa **MQTT Home Assistant Auto-Discovery** e reportistica Telegram.

---

## 🌐 Rete P2P Mesh Cifrata & Home Assistant

```text
 💻 PC Ufficio (Fuori casa) ───[ Mesh P2P su Internet ]───┐
                                                          │
 💻 Laptop in viaggio        ───[ Mesh P2P su Internet ]───┼──> 🖥️ Server Casa (Nodo P2P + MQTT Bridge) ──> 🏠 Home Assistant
                                                          │
 💻 PC Casa (Locale)         ───[ Mesh P2P Locale ]───────┘
```

- **🌐 Rete P2P Mesh Cifrata (ChaCha20-Poly1305)**: Collega $N$ server locali e remoti in una rete P2P privata. Ciascun nodo conosce in tempo reale i consumi di tutta la flotta.
- **🏠 Home Assistant Auto-Discovery**: Pubblica automaticamente i sensori di potenza ed energia (Watt, kWh oggi, kWh Lifetime, CPU, GPU, SSD) su Home Assistant via MQTT senza bisogno di configurazioni manuali YAML.

---

## 📱 Dashboard Terminale TUI (Stile 3) con Riepilogo Flotta

L'interfaccia a schermo intero mostra sia i dettagli del nodo locale sia il consumo aggregato di tutti i nodi del cluster P2P:

```text
 🔌 SERVER POWER MONITOR  •  Host: NTM-PC  [🔌 (96%)] [22:08:00]
 ────────────────────────────────────────────────────────────────────────────
 COMPONENTE             POTENZA   CARICO VISIVO             QUOTA
  🔳 CPU Package          16.6 W   [████████░░░░░░░░]        31.2%
  💻 System                5.6 W   [██░░░░░░░░░░░░░░]        10.5%
  🎨 GPU (NVIDIA)          7.1 W   [███░░░░░░░░░░░░░]        13.3%
  📀 SSD nvme0n1           0.3 W   [█░░░░░░░░░░░░░░░]         0.5%
  📀 SSD nvme1n1           2.5 W   [█░░░░░░░░░░░░░░░]         4.7%

 🌐 CLUSTER MESH (3 Nodi Attivi)
  ├ 🖥️ NTM-PC (Locale)   :    54.1 W │  0.0012 kWh
  ├ 🖥️ Server-NAS (P2P)   :    28.0 W │  0.1420 kWh (Lifetime: 12.4000 kWh)
  └ 🖥️ Rig-AI (P2P)       :   210.0 W │  1.2400 kWh (Lifetime: 98.2000 kWh)
  ⚡ POTENZA CLUSTER TOTALE: 292.1 W (Oggi: 1.3832 kWh)

 📈 ANDAMENTO POTENZA LOCALE (Ultimi 35 campionamenti)
  60.0W ┤ ▂▃▅█▇▆▅▄▃▂ ▂▃▄▅▆▇█  (Attuale: 54.1W)
 ────────────────────────────────────────────────────────────────────────────
  ⚡ POTENZA:   54.1 W  │  📊 OGGI: 0.0012 kWh (0.0004 EUR)
  🏛️  STORICO LIFETIME: 16.3890 kWh (4.9167 EUR)
```

---

## ⚙️ Riferimento Configurazione

| Parametro | Descrizione | Default |
|:----------|:------------|:--------|
| `SAMPLE_INTERVAL` | Intervallo in secondi tra i campionamenti | `5` |
| `TARIFF_EUR_KWH` | Costo dell'energia elettrica per kWh | `0.30` |
| `CURRENCY` | Simbolo della valuta nei report | `EUR` |
| `TELEGRAM_ENABLED` | Abilita (1) o disabilita (0) Telegram | `0` |
| `MQTT_ENABLED` | Abilita (1) o disabilita (0) MQTT per Home Assistant | `0` |
| `MQTT_HOST` | Hostname/IP del broker MQTT (es. Home Assistant) | `localhost` |
| `MQTT_PORT` | Porta del broker MQTT | `1883` |
| `P2P_ENABLED` | Abilita (1) o disabilita (0) la Rete P2P Mesh | `0` |
| `CLUSTER_SECRET` | Chiave segreta condivisa per cifrare la rete P2P | `""` |
| `P2P_PORT` | Porta UDP per la comunicazione P2P | `7432` |
| `P2P_PEERS` | Lista IP statici/WAN di nodi remoti (separati da spazio o virgola) | `""` |
| `RETENTION_DAYS` | Giorni di mantenimento log giornalieri (0 = conserva per sempre) | `365` |

---

## 🚀 Guida Rapida

### Esecuzione Nativa

```bash
cargo build --release
sudo ./target/release/server-power-monitor
```

### Docker Compose

```bash
docker-compose up -d --build
```

---

## 📄 Licenza

Rilasciato sotto licenza [MIT License](LICENSE).
