/*
 * ============================================================================
 * MODULE: config.rs — Gestione della Configurazione del Sistema
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Struct: Struttura dati per raggruppare tutti i parametri del monitor.
 * 2. Ownership & PathBuf: `PathBuf` è il tipo posseduto per i percorsi dei file.
 * 3. Feature Flags & Configs: Gestione di soglie alert, retention e export CSV.
 */

use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Struttura che contiene tutti i parametri di configurazione del monitor.
#[derive(Debug, Clone)]
pub struct Config {
    pub sample_interval: u64,
    pub tariff_eur_kwh: f64,
    pub currency: String,
    pub telegram_enabled: bool,
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    pub report_hour: u32,
    pub report_minute: u32,
    pub telegram_report_interval_hours: u32,
    pub host_label: String,
    pub hdd_active_w: f64,
    pub hdd_standby_w: f64,
    pub ssd_active_w: f64,
    pub ssd_idle_w: f64,

    // Nuovi parametri di retention, alert ed export
    pub retention_days: u32,
    pub max_power_alert_watts: f64,
    pub csv_export_enabled: bool,

    // Percorsi di sistema
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
    pub log_file: PathBuf,
    pub csv_file: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        let hostname = get_hostname().unwrap_or_else(|| "localhost".to_string());

        Self {
            sample_interval: 5,
            tariff_eur_kwh: 0.30,
            currency: "EUR".to_string(),
            telegram_enabled: false,
            telegram_bot_token: String::new(),
            telegram_chat_id: String::new(),
            report_hour: 23,
            report_minute: 55,
            telegram_report_interval_hours: 6,
            host_label: hostname,
            hdd_active_w: 5.0,
            hdd_standby_w: 0.5,
            ssd_active_w: 2.5,
            ssd_idle_w: 0.3,

            retention_days: 365,
            max_power_alert_watts: 0.0,
            csv_export_enabled: true,

            config_file: PathBuf::from("server-power-monitor.conf"),
            state_dir: PathBuf::from("./state"),
            log_file: PathBuf::from("./server-power-monitor.log"),
            csv_file: PathBuf::from("./history.csv"),
        }
    }
}

impl Config {
    /// Carica la configurazione combinando default, file `.conf` ed ambiente.
    pub fn load() -> Self {
        let mut config = Config::default();

        let exe_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        
        let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let local_conf = if current_dir.join("server-power-monitor.conf").exists() {
            current_dir.join("server-power-monitor.conf")
        } else if exe_dir.join("server-power-monitor.conf").exists() {
            exe_dir.join("server-power-monitor.conf")
        } else {
            PathBuf::from("/etc/server-power-monitor.conf")
        };

        config.config_file = env::var("CONFIG_FILE")
            .map(PathBuf::from)
            .unwrap_or(local_conf);

        let is_local = current_dir.join("server-power-monitor.conf").exists() || env::var("INVOCATION_ID").is_err();

        config.state_dir = env::var("STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                if is_local {
                    current_dir.join("state")
                } else {
                    PathBuf::from("/var/lib/server-power-monitor")
                }
            });

        config.log_file = env::var("LOG_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                if is_local {
                    current_dir.join("server-power-monitor.log")
                } else {
                    PathBuf::from("/var/log/server-power-monitor.log")
                }
            });

        config.csv_file = env::var("CSV_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                if is_local {
                    current_dir.join("history.csv")
                } else {
                    PathBuf::from("/var/log/server-power-monitor-history.csv")
                }
            });

        if config.config_file.exists() {
            config.parse_conf_file(&config.config_file.clone());
        }

        config.override_from_env();

        if config.host_label == "$(hostname)" || config.host_label == "$HOSTNAME" || config.host_label.is_empty() {
            config.host_label = get_hostname().unwrap_or_else(|| "localhost".to_string());
        }

        config
    }

    /// Parsing del file .conf
    fn parse_conf_file(&mut self, path: &Path) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let reader = BufReader::new(file);

        for line in reader.lines().flatten() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim();
                let val = val.trim().trim_matches('"').trim_matches('\'');

                match key {
                    "SAMPLE_INTERVAL" => {
                        if let Ok(v) = val.parse::<u64>() { self.sample_interval = v; }
                    }
                    "TARIFF_EUR_KWH" => {
                        if let Ok(v) = val.parse::<f64>() { self.tariff_eur_kwh = v; }
                    }
                    "CURRENCY" => self.currency = val.to_string(),
                    "TELEGRAM_ENABLED" => {
                        self.telegram_enabled = val == "1" || val.eq_ignore_ascii_case("true");
                    }
                    "TELEGRAM_BOT_TOKEN" => self.telegram_bot_token = val.to_string(),
                    "TELEGRAM_CHAT_ID" => self.telegram_chat_id = val.to_string(),
                    "REPORT_HOUR" => {
                        if let Ok(v) = val.parse::<u32>() { self.report_hour = v; }
                    }
                    "REPORT_MINUTE" => {
                        if let Ok(v) = val.parse::<u32>() { self.report_minute = v; }
                    }
                    "TELEGRAM_REPORT_INTERVAL_HOURS" => {
                        if let Ok(v) = val.parse::<u32>() { self.telegram_report_interval_hours = v; }
                    }
                    "HOST_LABEL" => {
                        if !val.is_empty() {
                            if val == "$(hostname)" || val == "$HOSTNAME" || val == "`hostname`" {
                                self.host_label = get_hostname().unwrap_or_else(|| "localhost".to_string());
                            } else {
                                self.host_label = val.to_string();
                            }
                        }
                    }
                    "HDD_ACTIVE_W" => {
                        if let Ok(v) = val.parse::<f64>() { self.hdd_active_w = v; }
                    }
                    "HDD_STANDBY_W" => {
                        if let Ok(v) = val.parse::<f64>() { self.hdd_standby_w = v; }
                    }
                    "SSD_ACTIVE_W" => {
                        if let Ok(v) = val.parse::<f64>() { self.ssd_active_w = v; }
                    }
                    "SSD_IDLE_W" => {
                        if let Ok(v) = val.parse::<f64>() { self.ssd_idle_w = v; }
                    }
                    "RETENTION_DAYS" => {
                        if let Ok(v) = val.parse::<u32>() { self.retention_days = v; }
                    }
                    "MAX_POWER_ALERT_WATTS" => {
                        if let Ok(v) = val.parse::<f64>() { self.max_power_alert_watts = v; }
                    }
                    "CSV_EXPORT_ENABLED" => {
                        self.csv_export_enabled = val == "1" || val.eq_ignore_ascii_case("true");
                    }
                    _ => {}
                }
            }
        }
    }

    fn override_from_env(&mut self) {
        if let Ok(val) = env::var("SAMPLE_INTERVAL") {
            if let Ok(v) = val.parse() { self.sample_interval = v; }
        }
        if let Ok(val) = env::var("TARIFF_EUR_KWH") {
            if let Ok(v) = val.parse() { self.tariff_eur_kwh = v; }
        }
        if let Ok(val) = env::var("CURRENCY") {
            self.currency = val;
        }
        if let Ok(val) = env::var("TELEGRAM_ENABLED") {
            self.telegram_enabled = val == "1" || val.eq_ignore_ascii_case("true");
        }
        if let Ok(val) = env::var("TELEGRAM_BOT_TOKEN") {
            self.telegram_bot_token = val;
        }
        if let Ok(val) = env::var("TELEGRAM_CHAT_ID") {
            self.telegram_chat_id = val;
        }
        if let Ok(val) = env::var("REPORT_HOUR") {
            if let Ok(v) = val.parse() { self.report_hour = v; }
        }
        if let Ok(val) = env::var("REPORT_MINUTE") {
            if let Ok(v) = val.parse() { self.report_minute = v; }
        }
        if let Ok(val) = env::var("TELEGRAM_REPORT_INTERVAL_HOURS") {
            if let Ok(v) = val.parse() { self.telegram_report_interval_hours = v; }
        }
        if let Ok(val) = env::var("HOST_LABEL") {
            if !val.is_empty() {
                if val == "$(hostname)" || val == "$HOSTNAME" || val == "`hostname`" {
                    self.host_label = get_hostname().unwrap_or_else(|| "localhost".to_string());
                } else {
                    self.host_label = val;
                }
            }
        }
        if let Ok(val) = env::var("RETENTION_DAYS") {
            if let Ok(v) = val.parse() { self.retention_days = v; }
        }
        if let Ok(val) = env::var("MAX_POWER_ALERT_WATTS") {
            if let Ok(v) = val.parse() { self.max_power_alert_watts = v; }
        }
        if let Ok(val) = env::var("CSV_EXPORT_ENABLED") {
            self.csv_export_enabled = val == "1" || val.eq_ignore_ascii_case("true");
        }
    }
}

fn get_hostname() -> Option<String> {
    if let Ok(name) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let trimmed = name.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}
