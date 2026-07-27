/*
 * ============================================================================
 * SERVER POWER MONITOR (RUST EDITION) — MAIN APPLICATION ENTRY POINT
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE PRINCIPALE (`main.rs`):
 * 1. Modular Architecture: Separazione in moduli puliti (`config`, `sensors`, `state`, `telegram`, `mqtt`, `p2p`, `tui`).
 * 2. Tokio Async Runtime: Gestione asincrona non bloccante di I/O, segnali e rete.
 * 3. Graceful Shutdown & Signal Handling (`tokio::select!`): Ripristino del cursore alla chiusura con Ctrl+C.
 */

mod config;
mod mqtt;
mod p2p;
mod sensors;
mod state;
mod telegram;
mod tui;

use std::env;
use std::fs;
use std::io::Write;
use std::time::Duration;

use chrono::{Local, Timelike};
use config::Config;
use mqtt::{MqttService, MqttStatePayload};
use p2p::{NodeTelemetryPacket, P2PService};
use state::{DailyState, LastState};
use telegram::TelegramClient;
use tui::PowerHistory;

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
        eprintln!("⚠️ Impossibile creare la directory di stato {:?}: {}", config.state_dir, e);
    }

    // 3. Scoperta automatica dei sensori
    let sensors = sensors::discover_sensors();
    if sensors.is_empty() {
        eprintln!("❌ ERRORE: Nessun sensore di energia o potenza trovato sul sistema (RAPL/NVIDIA/Disk).");
        std::process::exit(1);
    }

    let rapl_ok = sensors::check_rapl_permissions(&sensors);

    // Esegue la retention policy all'avvio
    state::apply_retention_policy(&config.state_dir, config.retention_days, &sensors);

    // 4. Inizializzazione dello stato e dello storico
    let mut last_state = LastState::load_or_init(&config.state_dir, &sensors);
    let mut current_date = Local::now().format("%Y-%m-%d").to_string();
    let mut daily_state = DailyState::load_or_create(&config.state_dir, &current_date, &sensors);
    let mut power_history = PowerHistory::new(35);
    let mut last_alert_ts: u64 = 0;

    // 5. Inizializzazione dei Client Telegram, MQTT e P2P
    let telegram = TelegramClient::new(&config);

    let mqtt_tx = if config.mqtt_enabled {
        match MqttService::start(&config) {
            Ok(tx) => Some(tx),
            Err(e) => {
                eprintln!("⚠️ Avviso: Impossibile avviare il servizio MQTT: {}", e);
                None
            }
        }
    } else {
        None
    };

    let (p2p_tx, cluster_state) = if config.p2p_enabled {
        match P2PService::start(&config).await {
            Ok((tx, state)) => (Some(tx), Some(state)),
            Err(e) => {
                eprintln!("⚠️ Avviso: Impossibile avviare il nodo P2P Cluster Mesh: {}", e);
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    if is_test_report {
        println!("🧪 Modalità Test: Invio report di test su Telegram...");
        telegram
            .send_status_report("MANUAL-TEST", &daily_state, &sensors, &config)
            .await?;
        println!("✅ Report di test inviato con successo.");
        return Ok(());
    }

    let _ = telegram.send_startup(&config.host_label, sensors.len()).await;

    // Pulisce lo schermo e nasconde il cursore per il Live TUI
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

                    state::apply_retention_policy(&config.state_dir, config.retention_days, &sensors);

                    current_date = now_date.clone();
                    daily_state = DailyState::load_or_create(&config.state_dir, &current_date, &sensors);
                }

                // Misurazione sensori
                let mut sensor_watts: Vec<(String, f64, String)> = Vec::new();
                let mut sensor_map_for_mqtt = std::collections::HashMap::new();
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
                    sensor_map_for_mqtt.insert(sensor.id.clone(), meas.watts);
                    total_watts += meas.watts;
                }

                power_history.push(total_watts);

                daily_state.save(&config.state_dir);
                last_state.save(&config.state_dir);

                let (today_j, _) = state::calculate_total_joules(&daily_state, &sensors);
                let today_kwh = today_j / 3_600_000.0;
                let today_cost = today_kwh * config.tariff_eur_kwh;

                let summary = state::compute_alltime_summary(
                    &config.state_dir,
                    &sensors,
                    config.tariff_eur_kwh,
                    Some(&daily_state),
                );

                // Pubblicazione MQTT (Home Assistant)
                if let Some(ref tx) = mqtt_tx {
                    let mqtt_payload = MqttStatePayload {
                        host: config.host_label.clone(),
                        total_watts,
                        today_kwh,
                        today_cost,
                        alltime_kwh: summary.total_kwh,
                        alltime_cost: summary.total_cost,
                        currency: config.currency.clone(),
                        sensors: sensor_map_for_mqtt,
                    };
                    let _ = tx.send(mqtt_payload).await;
                }

                // Pubblicazione P2P Cluster Mesh
                if let Some(ref tx) = p2p_tx {
                    let p2p_packet = NodeTelemetryPacket {
                        host: config.host_label.clone(),
                        total_watts,
                        today_kwh,
                        today_cost,
                        alltime_kwh: summary.total_kwh,
                        alltime_cost: summary.total_cost,
                        timestamp: now_ts,
                    };
                    let _ = tx.send(p2p_packet).await;
                }

                // Controllo Alert Potenza Max
                if config.max_power_alert_watts > 0.0 && total_watts >= config.max_power_alert_watts {
                    if now_ts.saturating_sub(last_alert_ts) > 900 {
                        let _ = telegram.send_power_alert(&config.host_label, total_watts, config.max_power_alert_watts).await;
                        last_alert_ts = now_ts;
                    }
                }

                // Rendering TUI Live Dashboard con Contatore Storico e Cluster Mesh
                tui::render_tui_style_3(
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
                    cluster_state.as_ref(),
                );

                maybe_send_scheduled_report(&now, &daily_state, &sensors, &config, &telegram).await;
            }
        }
    }

    Ok(())
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
