/*
 * ============================================================================
 * MODULE: sensors.rs — Scoperta e Lettura dei Sensori di Consumo Energetico
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Enum: Istruzioni di scelta tra opzioni disgiunte (Rapl, Nvidia, Disk).
 * 2. Struct: `SensorInfo` definisce la metadata di ciascun sensore scoperto.
 * 3. Command Execution (`std::process::Command`): Esecuzione di `nvidia-smi` e `hdparm`.
 * 4. File I/O & Sysfs: Lettura diretta dei contatori micro-joule da Linux.
 * 5. Diagnostic Checks: Verifica dei permessi di lettura sui file sysfs.
 */

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

/// Enum che rappresenta la tipologia di sensore di potenza/energia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SensorType {
    Rapl,   // Intel / AMD RAPL (Running Average Power Limit) da sysfs
    Nvidia, // GPU NVIDIA via nvidia-smi
    Disk,   // HDD / SSD da /sys/block/
}

/// Metadata di un singolo sensore identificato sul server.
#[derive(Debug, Clone)]
pub struct SensorInfo {
    pub id: String,
    pub raw_name: String,
    pub friendly_name: String,
    pub sensor_type: SensorType,
    pub path: PathBuf,
    pub max_energy_range_uj: u64,
}

impl SensorInfo {
    /// Restituisce un nome formattato visivamente con emoji.
    pub fn compute_friendly_name(id: &str, raw_name: &str) -> String {
        match raw_name {
            n if n.starts_with("package") => "🔳 CPU Package".to_string(),
            n if n.starts_with("core") => "🧠 Cores".to_string(),
            n if n.starts_with("uncore") => "🎨 iGPU".to_string(),
            n if n.starts_with("dram") => "📟 RAM".to_string(),
            n if n.starts_with("psys") => "💻 System".to_string(),
            n if n.contains("SSD") || id.contains("nvme") => format!("📀 SSD {}", id.trim_start_matches("disk_")),
            n if n.contains("HDD") || id.contains("sd") => format!("💿 HDD {}", id.trim_start_matches("disk_")),
            _ => {
                if id.starts_with("nvidia") {
                    "🎨 GPU".to_string()
                } else if !raw_name.is_empty() {
                    raw_name.to_string()
                } else {
                    id.to_string()
                }
            }
        }
    }
}

/// Esegue la scoperta automatica di tutti i sensori energetici disponibili nel sistema.
pub fn discover_sensors() -> Vec<SensorInfo> {
    let mut sensors = Vec::new();

    discover_rapl_sensors(&mut sensors);
    discover_nvidia_sensors(&mut sensors);
    discover_disk_sensors(&mut sensors);

    sensors
}

/// Verifica se i file sysfs di Intel RAPL sono leggibili dall'utente corrente
pub fn check_rapl_permissions(sensors: &[SensorInfo]) -> bool {
    let rapl_sensors: Vec<&SensorInfo> = sensors
        .iter()
        .filter(|s| s.sensor_type == SensorType::Rapl)
        .collect();

    if rapl_sensors.is_empty() {
        return true;
    }

    for sensor in rapl_sensors {
        if fs::File::open(&sensor.path).is_err() {
            return false;
        }
    }

    true
}

/// Cerca i sensori Intel RAPL esplorando la gerarchia /sys/class/powercap/
fn discover_rapl_sensors(sensors: &mut Vec<SensorInfo>) {
    let powercap_dir = Path::new("/sys/class/powercap");
    if !powercap_dir.exists() {
        return;
    }

    let entries = match fs::read_dir(powercap_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("intel-rapl") {
            let energy_file = entry.path().join("energy_uj");
            let name_file = entry.path().join("name");

            if energy_file.exists() && name_file.exists() {
                let raw_name = fs::read_to_string(&name_file)
                    .unwrap_or_else(|_| "unknown".to_string())
                    .trim()
                    .to_string();

                let clean_basename: String = name.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                let id = format!("rapl_{}", clean_basename);

                let max_file = entry.path().join("max_energy_range_uj");
                let max_energy = if max_file.exists() {
                    fs::read_to_string(&max_file)
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .unwrap_or(0)
                } else {
                    0
                };

                let friendly_name = SensorInfo::compute_friendly_name(&id, &raw_name);

                sensors.push(SensorInfo {
                    id,
                    raw_name,
                    friendly_name,
                    sensor_type: SensorType::Rapl,
                    path: energy_file,
                    max_energy_range_uj: max_energy,
                });
            }
        }
    }
}

/// Scoperta delle GPU NVIDIA eseguendo `nvidia-smi`
fn discover_nvidia_sensors(sensors: &mut Vec<SensorInfo>) {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=count", "--format=csv,noheader,nounits"])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let count_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Ok(gpu_count) = count_str.parse::<u32>() {
                for i in 0..gpu_count {
                    let id = format!("nvidia_gpu_{}", i);
                    let name_out = Command::new("nvidia-smi")
                        .args([
                            "--query-gpu=name",
                            "--format=csv,noheader,nounits",
                            &format!("--id={}", i),
                        ])
                        .output();

                    let raw_name = if let Ok(n_out) = name_out {
                        String::from_utf8_lossy(&n_out.stdout).trim().to_string()
                    } else {
                        format!("NVIDIA GPU {}", i)
                    };

                    let friendly_name = SensorInfo::compute_friendly_name(&id, &raw_name);

                    sensors.push(SensorInfo {
                        id,
                        raw_name,
                        friendly_name,
                        sensor_type: SensorType::Nvidia,
                        path: PathBuf::from(i.to_string()),
                        max_energy_range_uj: 0,
                    });
                }
            }
        }
    }
}

/// Scoperta dei dischi HDD e SSD in /sys/block/
fn discover_disk_sensors(sensors: &mut Vec<SensorInfo>) {
    let sys_block = Path::new("/sys/block");
    if !sys_block.exists() {
        return;
    }

    let entries = match fs::read_dir(sys_block) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let disk_name = entry.file_name().to_string_lossy().to_string();

        if (disk_name.starts_with("sd") || disk_name.starts_with("nvme"))
            && !is_partition_name(&disk_name)
        {
            let id = format!("disk_{}", disk_name);
            let rot_file = entry.path().join("queue/rotational");

            let is_rotational = if rot_file.exists() {
                fs::read_to_string(&rot_file)
                    .map(|s| s.trim() == "1")
                    .unwrap_or(false)
            } else {
                false
            };

            let raw_name = if is_rotational {
                format!("HDD {}", disk_name)
            } else {
                format!("SSD {}", disk_name)
            };

            let friendly_name = SensorInfo::compute_friendly_name(&id, &raw_name);

            sensors.push(SensorInfo {
                id,
                raw_name,
                friendly_name,
                sensor_type: SensorType::Disk,
                path: entry.path(),
                max_energy_range_uj: 0,
            });
        }
    }
}

fn is_partition_name(name: &str) -> bool {
    if name.starts_with("nvme") {
        if let Some(pos) = name.rfind('p') {
            return name[pos + 1..].chars().all(|c| c.is_ascii_digit());
        }
    } else if name.starts_with("sd") {
        if let Some(c) = name.chars().last() {
            return c.is_ascii_digit();
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct SensorMeasurement {
    pub cur_uj_or_io: u64,
    pub delta_joules: f64,
    pub watts: f64,
}

pub fn measure_sensor(
    sensor: &SensorInfo,
    last_val: u64,
    delta_sec: u64,
    config: &Config,
) -> SensorMeasurement {
    let delta_s = if delta_sec > 0 { delta_sec as f64 } else { 1.0 };

    match sensor.sensor_type {
        SensorType::Rapl => {
            let cur_uj = fs::read_to_string(&sensor.path)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);

            let delta_uj = if cur_uj >= last_val && last_val > 0 {
                cur_uj - last_val
            } else if sensor.max_energy_range_uj > 0 && last_val > 0 {
                sensor.max_energy_range_uj.saturating_sub(last_val) + cur_uj
            } else {
                0
            };

            let delta_joules = (delta_uj as f64) / 1_000_000.0;
            let watts = delta_joules / delta_s;

            SensorMeasurement {
                cur_uj_or_io: cur_uj,
                delta_joules,
                watts,
            }
        }
        SensorType::Nvidia => {
            let index_str = sensor.path.to_string_lossy().to_string();
            let output = Command::new("nvidia-smi")
                .args([
                    "--query-gpu=power.draw",
                    "--format=csv,noheader,nounits",
                    &format!("--id={}", index_str),
                ])
                .output();

            let watts = if let Ok(out) = output {
                String::from_utf8_lossy(&out.stdout)
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(0.0)
            } else {
                0.0
            };

            let delta_joules = watts * delta_s;

            SensorMeasurement {
                cur_uj_or_io: 0,
                delta_joules,
                watts,
            }
        }
        SensorType::Disk => {
            let disk_name = sensor.id.trim_start_matches("disk_");
            let dev_path = format!("/dev/{}", disk_name);
            let rot_file = sensor.path.join("queue/rotational");

            let is_rotational = rot_file.exists()
                && fs::read_to_string(&rot_file).map(|s| s.trim() == "1").unwrap_or(false);

            let (watts, cur_io) = if is_rotational {
                let status_out = Command::new("hdparm")
                    .args(["-C", &dev_path])
                    .output();

                let is_standby = if let Ok(out) = status_out {
                    String::from_utf8_lossy(&out.stdout).contains("standby")
                } else {
                    false
                };

                let w = if is_standby {
                    config.hdd_standby_w
                } else {
                    config.hdd_active_w
                };
                (w, 0)
            } else {
                let stat_file = sensor.path.join("stat");
                let cur_io = if stat_file.exists() {
                    read_disk_io_stats(&stat_file)
                } else {
                    0
                };

                let delta_io = cur_io.saturating_sub(last_val);
                let w = if delta_io > 0 && last_val > 0 {
                    config.ssd_active_w
                } else {
                    config.ssd_idle_w
                };
                (w, cur_io)
            };

            let delta_joules = watts * delta_s;

            SensorMeasurement {
                cur_uj_or_io: cur_io,
                delta_joules,
                watts,
            }
        }
    }
}

fn read_disk_io_stats(path: &Path) -> u64 {
    if let Ok(content) = fs::read_to_string(path) {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 7 {
            let read_sectors = parts[2].parse::<u64>().unwrap_or(0);
            let write_sectors = parts[6].parse::<u64>().unwrap_or(0);
            return read_sectors + write_sectors;
        }
    }
    0
}

pub fn get_power_status() -> (String, String) {
    let icon = if Path::new("/sys/class/power_supply/ADP1/online").exists() {
        let online = fs::read_to_string("/sys/class/power_supply/ADP1/online")
            .map(|s| s.trim() == "1")
            .unwrap_or(true);
        if online { "🔌" } else { "🔋" }
    } else {
        "🔌"
    };

    let battery_pct = if Path::new("/sys/class/power_supply/BAT1/capacity").exists() {
        fs::read_to_string("/sys/class/power_supply/BAT1/capacity")
            .map(|s| format!("({}%)", s.trim()))
            .unwrap_or_default()
    } else {
        String::new()
    };

    (icon.to_string(), battery_pct)
}
