# 🔌 Server Power Monitor (Rust Edition)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust_2021-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/Async-Tokio-blue.svg)](https://tokio.rs/)
[![Docker](https://img.shields.io/badge/Docker-Multi--stage-2496ED.svg)](https://www.docker.com/)
[![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey.svg)](https://www.linux.org/)

Un sistema di telemetria e monitoraggio in tempo reale del consumo energetico per server Linux, riscritto interamente in **Rust** per garantire massima efficienza, zero overhead di runtime e reportistica Telegram dal layout moderno ed elegante.

---

## 📱 Anteprima Output Telegram

I report inviati su Telegram sono in formato **HTML** a schede, con icone intuitive ed un **Contatore Storico Absolute (Lifetime)** che non perde mai un solo Watt o Centesimo dalla prima installazione:

```html
📅 REPORT ENERGETICO GIORNALIERO
━━━━━━━━━━━━━━━━━━━━━━
🖥️ Host: server-home
📆 Data: 2026-07-27

📦 Dettaglio Consumi:
• 🔳 CPU Package: 0.1420 kWh (Picco: 65.2W)
• 🧠 Cores: 0.0910 kWh (Picco: 42.1W)
• 🎨 GPU NVIDIA RTX 3080: 0.4500 kWh (Picco: 210.0W)
• 📀 SSD nvme0n1: 0.0150 kWh (Picco: 2.5W)

💰 RIEPILOGO OGGI:
├ ⚡ Consumo Totale: 0.6070 kWh
└ 💶 Costo Stimato: 0.1821 EUR

🏛️ CONTATORE STORICO ABSOLUTE (LIFETIME):
├ 🗓️ Primo Giorno: 2026-05-10 (78 giorni)
├ 📊 Totale Software: 45.3890 kWh (13.6167 EUR)
└ 📉 Media Giornaliera: 0.5819 kWh/g (0.1745 EUR/g)
```

---

## ✨ Caratteristiche Principali

- **🏛️ Contatore Storico Indistruttibile (Lifetime Counter)**: Registra e preserva il consumo cumulativo (kWh ed EUR) di tutta la vita del software tramite il file `lifetime_base.env`. Anche se la retention policy pulisce i vecchi log giornalieri, il totale storico assoluto **non perde mai un singolo Watt**.
- **📊 Terminal Dashboard TUI Live (Stile 3)**: Interfaccia a schermo intero con grafico Sparkline dell'andamento dei Watt, barrette di carico per ciascun componente e doppio contatore in basso (Consumo Oggi vs Contatore Storico Lifetime).
- **🚨 Alert Telegram per Picchi di Potenza**: Notifica immediata su Telegram se il consumo in Watt del server supera la soglia impostata (`MAX_POWER_ALERT_WATTS`).
- **🧹 Retention Policy Automatica**: Pulizia automatica configurabile dei file di log giornalieri più vecchi di N giorni (`RETENTION_DAYS`), con accorpamento automatico nel contatore storico permanente.
- **📄 Export Automatico in CSV**: Salva automaticamente un report giornaliero in `history.csv` per facilitare analisi esterne e grafici su Excel.

---

## 🚀 Guida Rapida

### 1. Compilazione ed Esecuzione Locale (Cargo)

Requisiti: Rust toolchain ([rustup.rs](https://rustup.rs/)).

```bash
# Compilazione ed avvio immediato in modalità release (consigliato con sudo per i registri RAPL)
cargo build --release
sudo ./target/release/server-power-monitor

# Invio di un report di test su Telegram
cargo run --release -- --test-report
```

### 2. Installazione Nativa come Servizio Systemd

```bash
bash setup_service.sh
```

---

## ⚙️ Riferimento Configurazione

| Parametro | Descrizione | Default |
|:----------|:------------|:--------|
| `SAMPLE_INTERVAL` | Intervallo in secondi tra i campionamenti | `5` |
| `TARIFF_EUR_KWH` | Costo dell'energia elettrica per kWh | `0.30` |
| `CURRENCY` | Simbolo della valuta nei report | `EUR` |
| `TELEGRAM_ENABLED` | Abilita (1) o disabilita (0) l'integrazione Telegram | `0` |
| `TELEGRAM_BOT_TOKEN` | Token segreto del Bot Telegram | `""` |
| `TELEGRAM_CHAT_ID` | ID della Chat/Gruppo Telegram | `""` |
| `MAX_POWER_ALERT_WATTS` | Soglia di potenza in Watt per alert Telegram immediati (0 = disattivato) | `0` |
| `RETENTION_DAYS` | Giorni di mantenimento dei log giornalieri (0 = conserva per sempre) | `365` |
| `CSV_EXPORT_ENABLED` | Abilita (1) o disabilita (0) l'export in `history.csv` | `1` |
| `HOST_LABEL` | Nome identificativo del server nei report | `hostname` |

---

## 📄 Licenza

Rilasciato sotto licenza [MIT License](LICENSE).
