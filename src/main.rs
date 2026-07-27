/*
 * ============================================================================
 * SERVER POWER MONITOR (RUST EDITION) — STILE 3: FULLSCREEN TUI LIVE DASHBOARD
 * ============================================================================
 *
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE PRINCIPALE (`main.rs`):
 * 1. Fullscreen TUI Live Dashboard: Interfaccia a schermo intero ridisegnata sul posto.
 * 2. Lifetime Software Counter: Visualizzazione live del totale cumulativo (kWh & EUR).
 * 3. Power Peak Alerts & Retention Policy: Monitoraggio delle soglie ed elaborazione automatica.
 * 4. Graceful Shutdown & Signal Handling (`tokio::select!`): Ripristino cursore su Ctrl+C.
 */

mod config;
mod sensors;
mod state;
mod telegram;

use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::Write;
use std::time::Duration;

use chrono::{Local, Timelike};
use config::Config;
use state::{DailyState, LastState};
use telegram::TelegramClient;

/// Struttura per conservare lo storico delle letture per il grafico Sparkline
pub struct PowerHistory {
    pub samples: VecDeque<f64>,
    pub max_samples: usize,
}

impl PowerHistory {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    pub fn push(&mut self, val: f64) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(val);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Carica la configurazione del sistema
    let config = Config::load();

    // 2. Gestione parametri CLI
    let args: Vec<String> = env::args().collect();
    let is_test_report = args.iter().any(|arg| arg == "--test-report");

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("🔌 Server Power Monitor v0.2.0 (Rust Edition)");
        println!("Uso: server-power-monitor [OPZIONI]");
        println!("\nOpzioni:");
        println!("  --test-report    Invia immediatamente un report di test su Telegram ed esce.");
        println!("  -h, --help       Mostra questo messaggio di aiuto.");
        return Ok(());
    }

    if let Err(e) = fs::create_dir_all(&config.state_dir) {
        eprintln!(
            "⚠️ Impossibile creare la directory di stato {:?}: {}",
            config.state_dir, e
        );
    }

    // 3. Scoperta automatica dei sensori
    let sensors = sensors::discover_sensors();
    if sensors.is_empty() {
        eprintln!("❌ ERRORE: Nessun sensore di energia o potenza trovato sul sistema (RAPL/NVIDIA/Disk).");
        std::process::exit(1);
    }

    let rapl_ok = sensors::check_rapl_permissions(&sensors);

    // Esegue retention policy all'avvio
    state::apply_retention_policy(&config.state_dir, config.retention_days, &sensors);

    // 4. Inizializzazione dello stato e dello storico
    let mut last_state = LastState::load_or_init(&config.state_dir, &sensors);
    let mut current_date = Local::now().format("%Y-%m-%d").to_string();
    let mut daily_state = DailyState::load_or_create(&config.state_dir, &current_date, &sensors);
    let mut power_history = PowerHistory::new(35);
    let mut last_alert_ts: u64 = 0;

    // 5. Inizializzazione Client Telegram
    let telegram = TelegramClient::new(&config);

    if is_test_report {
        println!("🧪 Modalità Test: Invio report di test su Telegram...");
        telegram
            .send_status_report("MANUAL-TEST", &daily_state, &sensors, &config)
            .await?;
        println!("✅ Report di test inviato con successo.");
        return Ok(());
    }

    let _ = telegram
        .send_startup(&config.host_label, sensors.len())
        .await;

    // Pulisce lo schermo e nasconde il cursore
    print!("\x1b[2J\x1b[H\x1b[?25l");
    let _ = std::io::stdout().flush();

    // 6. MAIN LOOP ASINCRONO
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                print!("\x1b[?25h\x1b[2J\x1b[H");
                let _ = std::io::stdout().flush();
                println!("👋 Server Power Monitor arrestato.");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(config.sample_interval)) => {
                let now = Local::now();
                let now_date = now.format("%Y-%m-%d").to_string();
                let now_ts = now.timestamp() as u64;

                // Rollover a mezzanotte
                if now_date != current_date {
                    let (day_j, _) = state::calculate_total_joules(&daily_state, &sensors);
                    let day_kwh = day_j / 3_600_000.0;
                    let day_cost = day_kwh * config.tariff_eur_kwh;
                    let max_peak = daily_state.peak_watts.values().cloned().fold(0.0, f64::max);

                    // Export CSV
                    if config.csv_export_enabled {
                        state::export_daily_to_csv(
                            &config.csv_file,
                            &config.host_label,
                            &current_date,
                            day_kwh,
                            day_cost,
                            &config.currency,
                            max_peak,
                        );
                    }

                    let _ = telegram
                        .send_daily_report(&current_date, &daily_state, &sensors, &config)
                        .await;

                    // Applica la retention policy
                    state::apply_retention_policy(&config.state_dir, config.retention_days, &sensors);

                    current_date = now_date.clone();
                    daily_state = DailyState::load_or_create(&config.state_dir, &current_date, &sensors);
                }

                // Misurazione sensori
                let mut sensor_watts: Vec<(String, f64, String)> = Vec::new();
                let mut total_watts = 0.0;

                for sensor in &sensors {
                    let last_uj = last_state.last_uj.get(&sensor.id).copied().unwrap_or(0);
                    let last_ts = last_state.last_ts.get(&sensor.id).copied().unwrap_or(now_ts);
                    let delta_sec = now_ts.saturating_sub(last_ts);

                    let meas = sensors::measure_sensor(sensor, last_uj, delta_sec, &config);

                    daily_state.add_sample(&sensor.id, meas.delta_joules, meas.watts);
                    last_state.last_uj.insert(sensor.id.clone(), meas.cur_uj_or_io);
                    last_state.last_ts.insert(sensor.id.clone(), now_ts);

                    sensor_watts.push((sensor.friendly_name.clone(), meas.watts, sensor.raw_name.clone()));
                    total_watts += meas.watts;
                }

                power_history.push(total_watts);

                daily_state.save(&config.state_dir);
                last_state.save(&config.state_dir);

                // Calcola sia il totale di Oggi che il Contatore Storico di Tutta la Storia del Software!
                let (today_j, _) = state::calculate_total_joules(&daily_state, &sensors);
                let today_kwh = today_j / 3_600_000.0;
                let today_cost = today_kwh * config.tariff_eur_kwh;

                let summary = state::compute_alltime_summary(
                    &config.state_dir,
                    &sensors,
                    config.tariff_eur_kwh,
                    Some(&daily_state),
                );

                // Controllo Alert Potenza Max (cooldown 15 min = 900s)
                if config.max_power_alert_watts > 0.0 && total_watts >= config.max_power_alert_watts {
                    if now_ts.saturating_sub(last_alert_ts) > 900 {
                        let _ = telegram.send_power_alert(&config.host_label, total_watts, config.max_power_alert_watts).await;
                        last_alert_ts = now_ts;
                    }
                }

                // Rendering TUI Live Dashboard con Contatore Storico
                render_tui_style_3(
                    &now,
                    &config.host_label,
                    &sensor_watts,
                    total_watts,
                    today_kwh,
                    today_cost,
                    summary.total_kwh,
                    summary.total_cost,
                    &config.currency,
                    &power_history,
                    rapl_ok,
                );

                maybe_send_scheduled_report(&now, &daily_state, &sensors, &config, &telegram).await;
            }
        }
    }

    Ok(())
}

/// Rendering dello STILE 3: Fullscreen TUI Live Dashboard con Contatore Storico Software
fn render_tui_style_3(
    now: &chrono::DateTime<Local>,
    host_label: &str,
    sensor_watts: &[(String, f64, String)],
    total_watts: f64,
    today_kwh: f64,
    today_cost: f64,
    alltime_kwh: f64,
    alltime_cost: f64,
    currency: &str,
    history: &PowerHistory,
    rapl_ok: bool,
) {
    let (power_icon, bat_level) = sensors::get_power_status();

    let c_reset = "\x1b[0m";
    let c_bold = "\x1b[1m";
    let c_gray = "\x1b[90m";
    let c_cyan = "\x1b[36m";
    let c_green = "\x1b[32m";
    let c_yellow = "\x1b[33m";
    let c_red = "\x1b[31m";
    let c_magenta = "\x1b[35m";

    let time_str = now.format("%H:%M:%S").to_string();
    let bat_str = if bat_level.is_empty() {
        power_icon
    } else {
        format!("{} {}", power_icon, bat_level)
    };

    let mut buf = String::new();

    buf.push_str("\x1b[H");

    // HEADER
    buf.push_str(&format!(
        "{}{} 🔌 SERVER POWER MONITOR{}  •  {}Host:{} {}{:<15}{} {}  [{}]\n",
        c_bold, c_cyan, c_reset, c_bold, c_reset, c_yellow, host_label, c_reset, bat_str, time_str
    ));
    buf.push_str(&format!("{}\n", format!("{}\u{2500}", c_gray).repeat(76)));

    if !rapl_ok {
        buf.push_str(&format!(
            "{}{}⚠️ PERMESSI SYSFS RIFIUTATI: Esegui con 'sudo' per sbloccare i dati CPU!{}\n",
            c_bold, c_red, c_reset
        ));
    }

    // TABELLA COMPONENTI CON BARRA VISIVA
    buf.push_str(&format!(
        "{}{:<22} {:>10}   {:<24} {:>6}{}\n",
        c_gray, "COMPONENTE", "POTENZA", "CARICO VISIVO", "QUOTA", c_reset
    ));

    let has_package = sensor_watts
        .iter()
        .any(|(_, _, raw)| raw.starts_with("package"));
    let mut seen_names = std::collections::HashSet::new();

    for (friendly, w, raw) in sensor_watts {
        if raw.starts_with("core") && has_package {
            continue;
        }
        if seen_names.contains(raw) {
            continue;
        }
        seen_names.insert(raw.clone());

        let pct = if total_watts > 0.0 {
            (*w / total_watts) * 100.0
        } else {
            0.0
        };

        let bar_len = 16;
        let filled = ((pct / 100.0) * (bar_len as f64)).round() as usize;
        let filled = filled.min(bar_len);
        let empty = bar_len.saturating_sub(filled);

        let color = if friendly.contains("GPU") {
            c_green
        } else if friendly.contains("CPU") || friendly.contains("System") {
            c_cyan
        } else {
            c_magenta
        };

        let bar_str = format!(
            "{}{}{}{}{}",
            color,
            "█".repeat(filled),
            c_gray,
            "░".repeat(empty),
            c_reset
        );

        buf.push_str(&format!(
            "  {:<20} {}{:>7.1} W{}   [{}] {:>5.1}%\n",
            friendly, color, w, c_reset, bar_str, pct
        ));
    }

    buf.push_str("\n");

    // GRAFICO SPARKLINE
    buf.push_str(&format!(
        "{}{}📈 ANDAMENTO POTENZA (Ultimi 35 campionamenti){}\n",
        c_bold, c_yellow, c_reset
    ));

    let sparkline = generate_sparkline(&history.samples);
    let peak_in_history = history.samples.iter().cloned().fold(0.0, f64::max);

    buf.push_str(&format!(
        "  {:>5.1}W ┤ {}{}{}  (Attuale: {}{:.1}W{})\n",
        peak_in_history, c_yellow, sparkline, c_reset, c_bold, total_watts, c_reset
    ));

    buf.push_str(&format!("{}\n", format!("{}\u{2500}", c_gray).repeat(76)));

    // FOOTER RIEPILOGO: CONTATORE OGGI + CONTATORE STORICO LIFETIME
    buf.push_str(&format!(
        "  {}⚡ POTENZA: {}{:>6.1} W{}  │  {}📊 OGGI: {}{:.4} kWh{} ({:.4} {})\n",
        c_bold,
        c_yellow,
        total_watts,
        c_reset,
        c_bold,
        c_green,
        today_kwh,
        c_reset,
        today_cost,
        currency
    ));

    buf.push_str(&format!(
        "  {}🏛️  STORICO LIFETIME: {}{:.4} kWh{} ({:.4} {})\n",
        c_bold, c_cyan, alltime_kwh, c_reset, alltime_cost, currency
    ));

    buf.push_str("\x1b[J");

    print!("{}", buf);
    let _ = std::io::stdout().flush();
}

fn generate_sparkline(samples: &VecDeque<f64>) -> String {
    if samples.is_empty() {
        return " ".repeat(35);
    }

    let ticks = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let min_val = 0.0;
    let max_val = samples.iter().cloned().fold(10.0, f64::max);
    let range = (max_val - min_val).max(0.001);

    let mut spark = String::new();
    for &val in samples {
        let normalized = (val - min_val) / range;
        let idx = (normalized * (ticks.len() - 1) as f64).round() as usize;
        let idx = idx.min(ticks.len() - 1);
        spark.push(ticks[idx]);
    }

    let padding = 35usize.saturating_sub(spark.chars().count());
    format!("{}{}", spark, " ".repeat(padding))
}

async fn maybe_send_scheduled_report(
    now: &chrono::DateTime<Local>,
    daily_state: &DailyState,
    sensors: &[sensors::SensorInfo],
    config: &Config,
    telegram: &TelegramClient,
) {
    let now_h = now.hour();
    let now_m = now.minute();
    let today = now.format("%Y-%m-%d").to_string();

    let last_report_date = state::get_last_report_date(&config.state_dir);
    if now_h == config.report_hour
        && now_m == config.report_minute
        && last_report_date.as_deref() != Some(&today)
    {
        if let Ok(()) = telegram
            .send_daily_report(&today, daily_state, sensors, config)
            .await
        {
            state::save_last_report_date(&config.state_dir, &today);

            let (total_j, _) = state::calculate_total_joules(daily_state, sensors);
            let day_kwh = total_j / 3_600_000.0;
            let day_cost = day_kwh * config.tariff_eur_kwh;
            let log_msg = format!(
                "[{}] REPORT {} kWh={:.4} cost={:.4} {}\n",
                now.format("%F %T"),
                today,
                day_kwh,
                day_cost,
                config.currency
            );
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&config.log_file)
            {
                let _ = f.write_all(log_msg.as_bytes());
            }
        }
    }

    if config.telegram_report_interval_hours > 0 && now_m == 0 {
        if now_h % config.telegram_report_interval_hours == 0 && now_h != config.report_hour {
            let int_key = format!("{}_{:02}", today, now_h);
            let last_int = state::get_last_interval_report(&config.state_dir);

            if last_int.as_deref() != Some(&int_key) {
                let tag = format!("{:02}:00", now_h);
                if let Ok(()) = telegram
                    .send_status_report(&tag, daily_state, sensors, config)
                    .await
                {
                    state::save_last_interval_report(&config.state_dir, &int_key);
                }
            }
        }
    }
}
