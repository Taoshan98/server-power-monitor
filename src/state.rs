/*
 * ============================================================================
 * MODULE: state.rs — Persistenza dello Stato, Storico e Retention Policy
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Lifetime Software Counter: File `lifetime_base.env` per preservare il contatore
 *    storico totale assoluto anche quando i file giornalieri vecchi vengono rimossi.
 * 2. Retention Policy: Eliminazione sicura dei file vecchi salvaguardando il totale cumulativo.
 * 3. Export CSV: Scrittura append su file `.csv` per la tracciabilità esterna dei dati.
 */

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use chrono::{Duration as ChronoDuration, NaiveDate, Local};

use crate::sensors::SensorInfo;

/// Letture precedenti (Timestamp e valore UJ/IO) per ciascun sensore.
#[derive(Debug, Default)]
pub struct LastState {
    pub last_uj: HashMap<String, u64>,
    pub last_ts: HashMap<String, u64>,
}

impl LastState {
    pub fn load_or_init(state_dir: &Path, sensors: &[SensorInfo]) -> Self {
        let state_file = state_dir.join("state.env");
        let mut last_state = LastState::default();

        if state_file.exists() {
            if let Ok(file) = File::open(&state_file) {
                let reader = BufReader::new(file);
                for line in reader.lines().flatten() {
                    let trimmed = line.trim();
                    if let Some((k, v)) = trimmed.split_once('=') {
                        if let Some(id) = k.strip_prefix("LAST_UJ_") {
                            if let Ok(val) = v.parse::<u64>() {
                                last_state.last_uj.insert(id.to_string(), val);
                            }
                        } else if let Some(id) = k.strip_prefix("LAST_TS_") {
                            if let Ok(val) = v.parse::<u64>() {
                                last_state.last_ts.insert(id.to_string(), val);
                            }
                        }
                    }
                }
            }
        }

        let now_ts = Local::now().timestamp() as u64;
        let mut changed = false;

        for sensor in sensors {
            if !last_state.last_uj.contains_key(&sensor.id) {
                last_state.last_uj.insert(sensor.id.clone(), 0);
                last_state.last_ts.insert(sensor.id.clone(), now_ts);
                changed = true;
            }
        }

        if changed {
            last_state.save(state_dir);
        }

        last_state
    }

    pub fn save(&self, state_dir: &Path) {
        let state_file = state_dir.join("state.env");
        if let Ok(mut file) = File::create(&state_file) {
            for (id, val) in &self.last_uj {
                let ts = self.last_ts.get(id).copied().unwrap_or(0);
                let _ = writeln!(file, "LAST_UJ_{}={}", id, val);
                let _ = writeln!(file, "LAST_TS_{}={}", id, ts);
            }
        }
    }
}

/// Stato dei consumi della giornata corrente.
#[derive(Debug, Default, Clone)]
pub struct DailyState {
    pub date_str: String,
    pub joules: HashMap<String, f64>,
    pub peak_watts: HashMap<String, f64>,
}

impl DailyState {
    pub fn load_or_create(state_dir: &Path, date_str: &str, sensors: &[SensorInfo]) -> Self {
        let filename = format!("today_{}.env", date_str);
        let today_file = state_dir.join(&filename);
        let mut state = DailyState {
            date_str: date_str.to_string(),
            joules: HashMap::new(),
            peak_watts: HashMap::new(),
        };

        if today_file.exists() {
            if let Ok(file) = File::open(&today_file) {
                let reader = BufReader::new(file);
                for line in reader.lines().flatten() {
                    let trimmed = line.trim();
                    if let Some((k, v)) = trimmed.split_once('=') {
                        if let Some(id) = k.strip_prefix("J_") {
                            if let Ok(val) = v.parse::<f64>() {
                                state.joules.insert(id.to_string(), val);
                            }
                        } else if let Some(id) = k.strip_prefix("PEAK_") {
                            if let Ok(val) = v.parse::<f64>() {
                                state.peak_watts.insert(id.to_string(), val);
                            }
                        }
                    }
                }
            }
        }

        for sensor in sensors {
            state.joules.entry(sensor.id.clone()).or_insert(0.0);
            state.peak_watts.entry(sensor.id.clone()).or_insert(0.0);
        }

        state.save(state_dir);
        state
    }

    pub fn save(&self, state_dir: &Path) {
        let filename = format!("today_{}.env", self.date_str);
        let today_file = state_dir.join(&filename);
        if let Ok(mut file) = File::create(&today_file) {
            let _ = writeln!(file, "DATE={}", self.date_str);
            for (id, j) in &self.joules {
                let peak = self.peak_watts.get(id).copied().unwrap_or(0.0);
                let _ = writeln!(file, "J_{}={:.6}", id, j);
                let _ = writeln!(file, "PEAK_{}={:.2}", id, peak);
            }
        }
    }

    pub fn add_sample(&mut self, sensor_id: &str, delta_joules: f64, current_watts: f64) {
        let j = self.joules.entry(sensor_id.to_string()).or_insert(0.0);
        *j += delta_joules;

        let peak = self.peak_watts.entry(sensor_id.to_string()).or_insert(0.0);
        if current_watts > *peak {
            *peak = current_watts;
        }
    }
}

/// Contatore del ciclo di vita del software (`lifetime_base.env`) per preservare il totale storico assoluto.
#[derive(Debug, Clone)]
pub struct LifetimeBase {
    pub archived_joules: f64,
    pub first_date: String,
}

impl LifetimeBase {
    pub fn load_or_create(state_dir: &Path, today_date_str: &str) -> Self {
        let file_path = state_dir.join("lifetime_base.env");
        let mut base = LifetimeBase {
            archived_joules: 0.0,
            first_date: today_date_str.to_string(),
        };

        if file_path.exists() {
            if let Ok(file) = File::open(&file_path) {
                let reader = BufReader::new(file);
                for line in reader.lines().flatten() {
                    let trimmed = line.trim();
                    if let Some((k, v)) = trimmed.split_once('=') {
                        match k {
                            "ARCHIVED_JOULES" => {
                                if let Ok(val) = v.parse::<f64>() {
                                    base.archived_joules = val;
                                }
                            }
                            "FIRST_SOFTWARE_DATE" => {
                                if !v.is_empty() {
                                    base.first_date = v.to_string();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        } else {
            base.save(state_dir);
        }

        base
    }

    pub fn save(&self, state_dir: &Path) {
        let file_path = state_dir.join("lifetime_base.env");
        if let Ok(mut file) = File::create(&file_path) {
            let _ = writeln!(file, "ARCHIVED_JOULES={:.6}", self.archived_joules);
            let _ = writeln!(file, "FIRST_SOFTWARE_DATE={}", self.first_date);
        }
    }
}

/// Riepilogo cumulativo storico assoluto del software.
#[derive(Debug, Clone)]
pub struct AllTimeSummary {
    pub total_kwh: f64,
    pub total_cost: f64,
    pub days_count: usize,
    pub first_date: String,
}

/// Calcola i dati cumulativi storici tenendo conto di:
/// 1. `lifetime_base.env` (totale dei file eventualmente archiviati/eliminati dalla retention)
/// 2. Tutti i file `today_*.env` esistenti
pub fn compute_alltime_summary(
    state_dir: &Path,
    sensors: &[SensorInfo],
    tariff_eur_kwh: f64,
    current_live_state: Option<&DailyState>,
) -> AllTimeSummary {
    let today_str = Local::now().format("%Y-%m-%d").to_string();
    let lifetime = LifetimeBase::load_or_create(state_dir, &today_str);

    let mut total_joules = lifetime.archived_joules;
    let mut days_count = 0;
    let mut first_date = lifetime.first_date.clone();

    if let Ok(entries) = fs::read_dir(state_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with("today_") && file_name.ends_with(".env") {
                let date_part = file_name
                    .trim_start_matches("today_")
                    .trim_end_matches(".env")
                    .to_string();

                let daily_state = if let Some(live) = current_live_state {
                    if live.date_str == date_part {
                        live.clone()
                    } else {
                        DailyState::load_or_create(state_dir, &date_part, sensors)
                    }
                } else {
                    DailyState::load_or_create(state_dir, &date_part, sensors)
                };

                let (day_total_j, _) = calculate_total_joules(&daily_state, sensors);

                total_joules += day_total_j;
                days_count += 1;

                if first_date.is_empty() || date_part < first_date {
                    first_date = date_part;
                }
            }
        }
    }

    if days_count == 0 {
        days_count = 1;
    }

    let total_kwh = total_joules / 3_600_000.0;
    let total_cost = total_kwh * tariff_eur_kwh;

    AllTimeSummary {
        total_kwh,
        total_cost,
        days_count,
        first_date,
    }
}

/// Calcola il totale in Joule della giornata
pub fn calculate_total_joules(daily_state: &DailyState, sensors: &[SensorInfo]) -> (f64, bool) {
    let has_psys = sensors.iter().any(|s| s.raw_name.starts_with("psys"));
    let mut total_j = 0.0;

    for sensor in sensors {
        let j = daily_state.joules.get(&sensor.id).copied().unwrap_or(0.0);
        if has_psys {
            if sensor.raw_name.starts_with("psys") {
                total_j = j;
            }
        } else {
            if sensor.raw_name.starts_with("package")
                || sensor.id.starts_with("nvidia")
                || sensor.raw_name.starts_with("SSD")
                || sensor.raw_name.starts_with("HDD")
            {
                total_j += j;
            }
        }
    }

    (total_j, has_psys)
}

/// Applica la retention policy: elimina i file `today_*.env` vecchi accorpando il loro consumo in `lifetime_base.env`
pub fn apply_retention_policy(state_dir: &Path, retention_days: u32, sensors: &[SensorInfo]) {
    if retention_days == 0 {
        return;
    }

    let today_date = Local::now().naive_local().date();
    let cutoff_date = today_date - ChronoDuration::days(retention_days as i64);
    let today_str = today_date.format("%Y-%m-%d").to_string();

    let mut lifetime = LifetimeBase::load_or_create(state_dir, &today_str);
    let mut modified = false;

    if let Ok(entries) = fs::read_dir(state_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with("today_") && file_name.ends_with(".env") {
                let date_part = file_name
                    .trim_start_matches("today_")
                    .trim_end_matches(".env");

                if let Ok(file_date) = NaiveDate::parse_from_str(date_part, "%Y-%m-%d") {
                    if file_date < cutoff_date {
                        let old_daily = DailyState::load_or_create(state_dir, date_part, sensors);
                        let (j, _) = calculate_total_joules(&old_daily, sensors);
                        lifetime.archived_joules += j;

                        let path = entry.path();
                        let _ = fs::remove_file(path);
                        modified = true;
                    }
                }
            }
        }
    }

    if modified {
        lifetime.save(state_dir);
    }
}

/// Esporta il riepilogo giornaliero su file CSV (`history.csv`)
pub fn export_daily_to_csv(
    csv_file: &Path,
    host_label: &str,
    date_str: &str,
    total_kwh: f64,
    total_cost: f64,
    currency: &str,
    peak_watts: f64,
) {
    let file_exists = csv_file.exists();
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(csv_file)
    {
        if !file_exists {
            let _ = writeln!(file, "Date,Host,kWh,Cost,Currency,PeakWatts");
        }
        let _ = writeln!(
            file,
            "{},{},{:.4},{:.4},{},{:.1}",
            date_str, host_label, total_kwh, total_cost, currency, peak_watts
        );
    }
}

pub fn get_last_report_date(state_dir: &Path) -> Option<String> {
    fs::read_to_string(state_dir.join("last_report_date"))
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn save_last_report_date(state_dir: &Path, date_str: &str) {
    let _ = fs::write(state_dir.join("last_report_date"), date_str);
}

pub fn get_last_interval_report(state_dir: &Path) -> Option<String> {
    fs::read_to_string(state_dir.join("last_interval_report"))
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn save_last_interval_report(state_dir: &Path, key: &str) {
    let _ = fs::write(state_dir.join("last_interval_report"), key);
}
