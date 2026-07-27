/*
 * ============================================================================
 * MODULE: telegram.rs — Integrazione Telegram Bot con Contatori Storici
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Async/Await & Reqwest: Chiamate HTTP asincrone senza bloccare l'event loop.
 * 2. Alert & Schedulazione: Invio di avvisi immediati su picco e report periodici.
 * 3. Contatore Storico di Tutta la Storia del Software: Visualizzazione del consumo
 *    totale cumulativo fin dalla prima installazione.
 */

use std::collections::HashSet;
use anyhow::{Context, Result};
use reqwest::Client;

use crate::config::Config;
use crate::sensors::SensorInfo;
use crate::state::{self, DailyState};

pub struct TelegramClient {
    client: Client,
    bot_token: String,
    chat_id: String,
    enabled: bool,
}

impl TelegramClient {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            bot_token: config.telegram_bot_token.clone(),
            chat_id: config.telegram_chat_id.clone(),
            enabled: config.telegram_enabled
                && !config.telegram_bot_token.is_empty()
                && !config.telegram_chat_id.is_empty(),
        }
    }

    pub async fn send_message(&self, html_text: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let params = [
            ("chat_id", self.chat_id.as_str()),
            ("text", html_text),
            ("parse_mode", "HTML"),
            ("disable_web_page_preview", "true"),
        ];

        let res = self
            .client
            .post(&url)
            .form(&params)
            .send()
            .await
            .context("Errore nella richiesta HTTP a Telegram")?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            eprintln!("⚠️ Impossibile inviare report Telegram (HTTP {}): {}", status, body);
        }

        Ok(())
    }

    pub async fn send_startup(&self, host_label: &str, sensor_count: usize) -> Result<()> {
        let msg = format!(
            "🚀 <b>Server Power Monitor</b> avviato su <code>{}</code>\n\
             📡 Sensori energetici attivi: <b>{}</b>",
            host_label, sensor_count
        );
        self.send_message(&msg).await
    }

    /// Invia alert di picco potenza elevata
    pub async fn send_power_alert(&self, host_label: &str, current_w: f64, threshold_w: f64) -> Result<()> {
        let msg = format!(
            "🚨 <b>ALERT CONSUMO ELEVATO!</b>\n\
             ━━━━━━━━━━━━━━━━━━━━━━\n\
             🖥️ <b>Host:</b> <code>{}</code>\n\
             ⚡ <b>Potenza Attuale:</b> <code>{:.1}</code> W\n\
             ⚠️ <b>Soglia Limite:</b> <code>{:.1}</code> W",
            host_label, current_w, threshold_w
        );
        self.send_message(&msg).await
    }

    pub async fn send_status_report(
        &self,
        tag: &str,
        daily_state: &DailyState,
        sensors: &[SensorInfo],
        config: &Config,
    ) -> Result<()> {
        let (body, total_kwh, total_cost) = build_report_body(daily_state, sensors, config);
        let summary = state::compute_alltime_summary(&config.state_dir, sensors, config.tariff_eur_kwh, Some(daily_state));

        let msg = format!(
            "📊 <b>STATUS UPDATE</b> — <code>{}</code>\n\
             ━━━━━━━━━━━━━━━━━━━━━━\n\
             🖥️ <b>Host:</b> <code>{}</code>\n\
             📅 <b>Data:</b> <code>{}</code>\n\n\
             {}\n\n\
             💰 <b>TOTALE OGGI:</b>\n\
             ├ ⚡ Energia: <code>{:.4}</code> kWh\n\
             └ 💶 Costo: <code>{:.4}</code> {}\n\n\
             🏛️ <b>CONTATORE STORICO ABSOLUTE (LIFETIME):</b>\n\
             └ 📊 <b>Totale Software:</b> <code>{:.4}</code> kWh (<code>{:.4}</code> {})",
            tag,
            config.host_label,
            daily_state.date_str,
            body,
            total_kwh,
            total_cost,
            config.currency,
            summary.total_kwh,
            summary.total_cost,
            config.currency
        );

        self.send_message(&msg).await
    }

    pub async fn send_daily_report(
        &self,
        date_str: &str,
        daily_state: &DailyState,
        sensors: &[SensorInfo],
        config: &Config,
    ) -> Result<()> {
        let (body, day_kwh, day_cost) = build_report_body(daily_state, sensors, config);

        let summary = state::compute_alltime_summary(&config.state_dir, sensors, config.tariff_eur_kwh, Some(daily_state));
        let days = if summary.days_count > 0 { summary.days_count } else { 1 };
        let avg_kwh = summary.total_kwh / (days as f64);
        let avg_cost = summary.total_cost / (days as f64);

        let msg = format!(
            "📅 <b>REPORT ENERGETICO GIORNALIERO</b>\n\
             ━━━━━━━━━━━━━━━━━━━━━━\n\
             🖥️ <b>Host:</b> <code>{}</code>\n\
             📆 <b>Data:</b> <code>{}</code>\n\n\
             📦 <b>Dettaglio Consumi:</b>\n\
             {}\n\n\
             💰 <b>RIEPILOGO OGGI:</b>\n\
             ├ ⚡ <b>Consumo Totale:</b> <code>{:.4}</code> kWh\n\
             └ 💶 <b>Costo Stimato:</b> <code>{:.4}</code> {}\n\n\
             🏛️ <b>CONTATORE STORICO ABSOLUTE (LIFETIME):</b>\n\
             ├ 🗓️ <b>Primo Giorno:</b> <code>{}</code> (<b>{}</b> giorni)\n\
             ├ 📊 <b>Totale Software:</b> <code>{:.4}</code> kWh (<code>{:.4}</code> {})\n\
             └ 📉 <b>Media Giornaliera:</b> <code>{:.4}</code> kWh/g (<code>{:.4}</code> {}/g)",
            config.host_label,
            date_str,
            body,
            day_kwh,
            day_cost,
            config.currency,
            summary.first_date,
            days,
            summary.total_kwh,
            summary.total_cost,
            config.currency,
            avg_kwh,
            avg_cost,
            config.currency
        );

        self.send_message(&msg).await
    }
}

fn build_report_body(
    daily_state: &DailyState,
    sensors: &[SensorInfo],
    config: &Config,
) -> (String, f64, f64) {
    let mut section_cpu = String::new();
    let mut section_gpu = String::new();
    let mut section_ram = String::new();
    let mut section_sys = String::new();
    let mut section_disk = String::new();

    let mut seen_names = HashSet::new();

    let (total_j, _) = state::calculate_total_joules(daily_state, sensors);
    let total_kwh = total_j / 3_600_000.0;
    let total_cost = total_kwh * config.tariff_eur_kwh;

    for sensor in sensors {
        if seen_names.contains(&sensor.raw_name) {
            continue;
        }
        seen_names.insert(sensor.raw_name.clone());

        let joules = daily_state.joules.get(&sensor.id).copied().unwrap_or(0.0);
        let peak = daily_state.peak_watts.get(&sensor.id).copied().unwrap_or(0.0);
        let kwh = joules / 3_600_000.0;

        let line = format!(
            "• {}: <code>{:.4}</code> kWh (Picco: <code>{:.1}</code>W)\n",
            sensor.friendly_name, kwh, peak
        );

        match sensor.friendly_name.as_str() {
            n if n.contains("CPU") || n.contains("Cores") => section_cpu.push_str(&line),
            n if n.contains("GPU") => section_gpu.push_str(&line),
            n if n.contains("RAM") => section_ram.push_str(&line),
            n if n.contains("System") => section_sys.push_str(&line),
            _ => section_disk.push_str(&line),
        }
    }

    let mut full_body = String::new();
    full_body.push_str(&section_cpu);
    full_body.push_str(&section_gpu);
    full_body.push_str(&section_ram);
    full_body.push_str(&section_disk);
    full_body.push_str(&section_sys);

    (full_body.trim_end().to_string(), total_kwh, total_cost)
}
