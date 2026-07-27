/*
 * ============================================================================
 * MODULE: tui.rs — Interface TUI Live Dashboard (Stile 3)
 * ============================================================================
 * 
 * 💡 CONCETTI RUST DIDATTICI IN QUESTO FILE:
 * 1. Rendering Non-Flickering: Uso di sequenze ANSI (`\x1b[H`, `\x1b[J`) per sovrascrivere lo schermo.
 * 2. Sparkline Generation: Mappatura di valori numerici float su caratteri Unicode ` ▂▃▄▅▆▇█`.
 * 3. Component Progress Bars: Formattazione visiva delle percentuali di carico dei singoli sensori.
 */

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Local};

use crate::p2p::FleetClusterState;
use crate::sensors;

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

/// Rendering dello STILE 3: Fullscreen TUI Live Dashboard con Grafico Sparkline e Cluster Mesh
pub fn render_tui_style_3(
    now: &DateTime<Local>,
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
    cluster_state: Option<&Arc<Mutex<FleetClusterState>>>,
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

    // Posiziona il cursore in alto a sinistra (1,1)
    buf.push_str("\x1b[H");

    // --- HEADER ---
    buf.push_str(&format!(
        "{}{} 🔌 SERVER POWER MONITOR{}  •  {}Host:{} {}{:<15}{} {}  [{}]\n",
        c_bold, c_cyan, c_reset,
        c_bold, c_reset, c_yellow, host_label, c_reset,
        bat_str, time_str
    ));
    buf.push_str(&format!("{}\n", format!("{}\u{2500}", c_gray).repeat(76)));

    if !rapl_ok {
        buf.push_str(&format!(
            "{}{}⚠️ PERMESSI SYSFS RIFIUTATI: Esegui con 'sudo' per sbloccare i dati CPU!{}\n",
            c_bold, c_red, c_reset
        ));
    }

    // --- TABELLA COMPONENTI LOCAL NODE ---
    buf.push_str(&format!(
        "{}{:<22} {:>10}   {:<24} {:>6}{}\n",
        c_gray, "COMPONENTE", "POTENZA", "CARICO VISIVO", "QUOTA", c_reset
    ));

    let has_package = sensor_watts.iter().any(|(_, _, raw)| raw.starts_with("package"));
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

    // --- SEZIONE CLUSTER MESH (Se ci sono nodi remoti o P2P è attivo) ---
    if let Some(cs) = cluster_state {
        let guard = cs.lock().unwrap();
        let (cluster_w, cluster_kwh, active_count) = guard.compute_cluster_totals(host_label, total_watts, today_kwh);

        buf.push_str(&format!(
            "\n{}{}🌐 CLUSTER MESH ({} Nodi Attivi){}\n",
            c_bold, c_cyan, active_count, c_reset
        ));

        buf.push_str(&format!(
            "  ├ 🖥️ {:<18} : {}{:>7.1} W{} │ {:7.4} kWh\n",
            format!("{} (Locale)", host_label),
            c_yellow, total_watts, c_reset, today_kwh
        ));

        let mut node_list: Vec<_> = guard.nodes.values().collect();
        node_list.sort_by(|a, b| a.host.cmp(&b.host));

        for (idx, node) in node_list.iter().enumerate() {
            let is_last = idx == node_list.len() - 1;
            let prefix = if is_last { "└" } else { "├" };

            buf.push_str(&format!(
                "  {} 🖥️ {:<18} : {}{:>7.1} W{} │ {:7.4} kWh (Lifetime: {:.4} kWh)\n",
                prefix, node.host, c_green, node.total_watts, c_reset, node.today_kwh, node.alltime_kwh
            ));
        }

        buf.push_str(&format!(
            "  {}⚡ POTENZA CLUSTER TOTALE: {}{:.1} W{} (Oggi: {:.4} kWh)\n",
            c_bold, c_yellow, cluster_w, c_reset, cluster_kwh
        ));
    }

    buf.push_str("\n");

    // --- GRAFICO SPARKLINE DELLO STORICO ---
    buf.push_str(&format!(
        "{}{}📈 ANDAMENTO POTENZA LOCALE (Ultimi 35 campionamenti){}\n",
        c_bold, c_yellow, c_reset
    ));

    let sparkline = generate_sparkline(&history.samples);
    let peak_in_history = history.samples.iter().cloned().fold(0.0, f64::max);

    buf.push_str(&format!(
        "  {:>5.1}W ┤ {}{}{}  (Attuale: {}{:.1}W{})\n",
        peak_in_history, c_yellow, sparkline, c_reset, c_bold, total_watts, c_reset
    ));

    buf.push_str(&format!("{}\n", format!("{}\u{2500}", c_gray).repeat(76)));

    // --- FOOTER RIEPILOGO: CONTATORE OGGI + CONTATORE STORICO LIFETIME ---
    buf.push_str(&format!(
        "  {}⚡ POTENZA: {}{:>6.1} W{}  │  {}📊 OGGI: {}{:.4} kWh{} ({:.4} {})\n",
        c_bold, c_yellow, total_watts, c_reset,
        c_bold, c_green, today_kwh, c_reset, today_cost, currency
    ));

    buf.push_str(&format!(
        "  {}🏛️  STORICO LIFETIME: {}{:.4} kWh{} ({:.4} {})\n",
        c_bold, c_cyan, alltime_kwh, c_reset, alltime_cost, currency
    ));

    buf.push_str("\x1b[J");

    print!("{}", buf);
    let _ = std::io::stdout().flush();
}

/// Genera una stringa Sparkline a partire da una sequenza di valori numerici
pub fn generate_sparkline(samples: &VecDeque<f64>) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_history_push_and_limit() {
        let mut history = PowerHistory::new(3);
        history.push(10.0);
        history.push(20.0);
        history.push(30.0);
        assert_eq!(history.samples.len(), 3);

        history.push(40.0);
        assert_eq!(history.samples.len(), 3);
        assert_eq!(history.samples.front().copied(), Some(20.0));
        assert_eq!(history.samples.back().copied(), Some(40.0));
    }

    #[test]
    fn test_generate_sparkline_empty() {
        let samples = VecDeque::new();
        let spark = generate_sparkline(&samples);
        assert_eq!(spark.len(), 35);
    }

    #[test]
    fn test_generate_sparkline_values() {
        let mut samples = VecDeque::new();
        samples.push_back(0.0);
        samples.push_back(5.0);
        samples.push_back(10.0);

        let spark = generate_sparkline(&samples);
        assert!(spark.starts_with(' '));
        assert!(spark.contains('█'));
    }
}
