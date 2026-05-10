#!/usr/bin/env bash
# server-power-monitor.sh — Energy usage monitor with Telegram reports
set -euo pipefail
export LC_NUMERIC=C

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- CONFIG / STATE / LOG RESOLUTION ---
# Priority: env var → local → system default

_resolve_path() {
  local env_var="$1" local_name="$2" system_path="$3"
  if [[ -n "${!env_var:-}" ]]; then
    echo "${!env_var}"
  elif [[ -f "$SCRIPT_DIR/$local_name" ]]; then
    echo "$SCRIPT_DIR/$local_name"
  else
    echo "$system_path"
  fi
}

IS_LOCAL=$([[ -f "$SCRIPT_DIR/server-power-monitor.conf" ]] && echo 1 || echo 0)

CONFIG_FILE=$(_resolve_path CONFIG_FILE server-power-monitor.conf /etc/server-power-monitor.conf)
STATE_DIR=$(_resolve_path STATE_DIR "" "$(
  [[ "$IS_LOCAL" == 1 || -z "${INVOCATION_ID:-}" ]] \
    && echo "$SCRIPT_DIR/state" || echo "/var/lib/server-power-monitor"
)")
LOG_FILE=$(_resolve_path LOG_FILE "" "$(
  [[ "$IS_LOCAL" == 1 || -z "${INVOCATION_ID:-}" ]] \
    && echo "$SCRIPT_DIR/server-power-monitor.log" || echo "/var/log/server-power-monitor.log"
)")

mkdir -p "$STATE_DIR"
# shellcheck disable=SC1090
[[ -f "$CONFIG_FILE" ]] && source "$CONFIG_FILE"

# --- DEFAULTS ---
: "${SAMPLE_INTERVAL:=5}"
: "${TARIFF_EUR_KWH:=0.30}"
: "${CURRENCY:=EUR}"
: "${TELEGRAM_ENABLED:=0}"
: "${TELEGRAM_BOT_TOKEN:=}"
: "${TELEGRAM_CHAT_ID:=}"
: "${REPORT_HOUR:=23}"
: "${REPORT_MINUTE:=55}"
: "${TELEGRAM_REPORT_INTERVAL_HOURS:=6}"
: "${HOST_LABEL:=$(hostname)}"
: "${HDD_ACTIVE_W:=5.0}"
: "${HDD_STANDBY_W:=0.5}"
: "${SSD_ACTIVE_W:=2.5}"
: "${SSD_IDLE_W:=0.3}"

# --- SENSOR DISCOVERY ---
declare -A SENSOR_PATHS SENSOR_NAMES SENSOR_TYPES SENSOR_MAX_ENERGY SENSOR_WATTS

for p in /sys/class/powercap/intel-rapl*/energy_uj \
          /sys/class/powercap/intel-rapl*/*/energy_uj; do
  [[ -e "$p" ]] || continue
  name_file="$(dirname "$p")/name"
  [[ -f "$name_file" ]] || continue
  raw_name=$(cat "$name_file" 2>/dev/null || echo "unknown")
  id="rapl_$(basename "$(dirname "$p")" | tr -cd '[:alnum:]_')"
  SENSOR_PATHS[$id]="$p"
  SENSOR_NAMES[$id]="$raw_name"
  SENSOR_TYPES[$id]="rapl"
  max_file="$(dirname "$p")/max_energy_range_uj"
  [[ -f "$max_file" ]] && SENSOR_MAX_ENERGY[$id]=$(cat "$max_file" 2>/dev/null || echo 0)
done

if command -v nvidia-smi &>/dev/null; then
  gpu_count=$(nvidia-smi --query-gpu=count --format=csv,noheader,nounits | head -n1 || echo 0)
  for ((i = 0; i < gpu_count; i++)); do
    id="nvidia_gpu_$i"
    SENSOR_PATHS[$id]="$i"
    SENSOR_NAMES[$id]=$(nvidia-smi --query-gpu=name --format=csv,noheader,nounits --id=$i)
    SENSOR_TYPES[$id]="nvidia"
  done
fi

for d in /sys/block/sd* /sys/block/nvme*; do
  [[ -e "$d" ]] || continue
  name=$(basename "$d")
  [[ "$name" =~ nvme.*p[0-9]+$ ]] && continue
  [[ "$name" =~ sd.*[0-9]$ ]] && continue
  id="disk_$name"
  rot=$(cat "$d/queue/rotational" 2>/dev/null || echo 0)
  SENSOR_PATHS[$id]="$d"
  SENSOR_TYPES[$id]="disk"
  SENSOR_NAMES[$id]="$([[ "$rot" == 1 ]] && echo "HDD $name" || echo "SSD $name")"
done

[[ ${#SENSOR_PATHS[@]} -gt 0 ]] || { echo "ERROR: No energy sensors found." >&2; exit 1; }

# --- STATE FILES ---
STATE_FILE="$STATE_DIR/state.env"
TODAY_FILE="$STATE_DIR/today_$(date +%F).env"
LAST_REPORT_FILE="$STATE_DIR/last_report_date"

# --- UTILS ---

j_to_kwh()  { awk -v j="$1"   'BEGIN { printf "%.4f", j / 3600000 }'; }
calc_cost()  { awk -v k="$1" -v t="$TARIFF_EUR_KWH" 'BEGIN { printf "%.4f", k * t }'; }
awk_sum()    { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.6f", a + b }'; }
awk_max()    { awk -v a="$1" -v b="$2" 'BEGIN { if (b > a) print b; else print a }'; }

save_kv() {
  local file="$1"; shift
  printf '%s\n' "$@" > "$file"
}

get_friendly_name() {
  local id="$1" raw="${SENSOR_NAMES[$1]:-}"
  case "$raw" in
    package*) echo "🔳 CPU"       ;;
    core*)    echo "🧠 Cores"     ;;
    uncore*)  echo "🎨 iGPU"      ;;
    dram*)    echo "📟 RAM"       ;;
    psys*)    echo "💻 System"    ;;
    *SSD*|*nvme*) echo "📀 $raw"  ;;
    *HDD*)        echo "💿 $raw"  ;;
    *)  [[ "$id" == nvidia* ]] && echo "🎨 GPU" || echo "${raw:-$id}" ;;
  esac
}

send_telegram() {
  [[ "$TELEGRAM_ENABLED" == 1 && -n "$TELEGRAM_BOT_TOKEN" && -n "$TELEGRAM_CHAT_ID" ]] || return 0
  curl -fsS -X POST "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/sendMessage" \
    --data-urlencode "chat_id=${TELEGRAM_CHAT_ID}" \
    --data-urlencode "text=$1" \
    --data-urlencode "parse_mode=HTML" >/dev/null || true
}

# --- STATE MANAGEMENT ---

load_state() {
  # shellcheck disable=SC1090
  [[ -f "$STATE_FILE" ]] && source "$STATE_FILE"
  local changed=0
  for id in "${!SENSOR_PATHS[@]}"; do
    local v_uj="LAST_UJ_$id" v_ts="LAST_TS_$id"
    [[ -n "${!v_uj:-}" ]] && continue
    case "${SENSOR_TYPES[$id]}" in
      rapl) printf -v "$v_uj" '%s' "$(cat "${SENSOR_PATHS[$id]}")" ;;
      disk) printf -v "$v_uj" '%s' "$(awk '{print $3+$7}' "${SENSOR_PATHS[$id]}/stat" 2>/dev/null || echo 0)" ;;
      *)    printf -v "$v_uj" '%s' "0" ;;
    esac
    printf -v "$v_ts" '%s' "$(date +%s)"
    changed=1
  done
  if [[ $changed -eq 1 ]]; then
    local kvs=()
    for id in "${!SENSOR_PATHS[@]}"; do
      local v_uj="LAST_UJ_$id" v_ts="LAST_TS_$id"
      kvs+=("$v_uj=${!v_uj}" "$v_ts=${!v_ts}")
    done
    save_kv "$STATE_FILE" "${kvs[@]}"
  fi
}

ensure_today_file() {
  TODAY_FILE="$STATE_DIR/today_$(date +%F).env"
  [[ -f "$TODAY_FILE" ]] && return
  local kvs
  kvs=("DATE=$(date +%F)")
  for id in "${!SENSOR_PATHS[@]}"; do kvs+=("J_$id=0" "PEAK_$id=0"); done
  save_kv "$TODAY_FILE" "${kvs[@]}"
}

load_today() {
  ensure_today_file
  # shellcheck disable=SC1090
  source "$TODAY_FILE"
}

# --- REPORTING ---
# Shared logic: builds per-sensor lines and computes total.
# psys is used as the sole total when present; otherwise package + nvidia + disks.

_build_report_body() {
  local source_file="$1"
  # shellcheck disable=SC1090
  source "$source_file"

  local total_j=0 has_psys=0 section_cpu="" section_gpu="" section_ram="" section_sys="" section_disk=""
  declare -A seen

  # First pass: detect psys
  for id in "${!SENSOR_PATHS[@]}"; do
    [[ "${SENSOR_NAMES[$id]}" == psys* ]] && has_psys=1 && break
  done

  for id in "${!SENSOR_PATHS[@]}"; do
    local raw="${SENSOR_NAMES[$id]:-}"
    [[ -n "${seen[$raw]:-}" ]] && continue
    seen[$raw]=1

    j_val="J_$id"; peak_val="PEAK_$id"
    local j="${!j_val:-0}" peak="${!peak_val:-0}"

    if [[ $has_psys -eq 1 ]]; then
      [[ "$raw" == psys* ]] && total_j="$j"
    else
      [[ "$raw" == package* || "$id" == nvidia* || "$raw" == SSD* || "$raw" == HDD* ]] \
        && total_j=$(awk_sum "$total_j" "$j")
    fi

    local friendly kwh
    friendly=$(get_friendly_name "$id")
    kwh=$(j_to_kwh "$j")
    local line="• ${friendly}: <code>${kwh}</code> kWh (Peak: <code>${peak}</code>W)\n"
    case "$friendly" in
      *CPU*|*Cores*) section_cpu+="$line" ;;
      *GPU*)         section_gpu+="$line" ;;
      *RAM*)         section_ram+="$line" ;;
      *System*)      section_sys+="$line" ;;
      *)             section_disk+="$line" ;;
    esac
  done

  local total_kwh cost
  total_kwh=$(j_to_kwh "$total_j")
  cost=$(calc_cost "$total_kwh")

  printf '%s' "${section_cpu}${section_gpu}${section_ram}${section_disk}${section_sys}"
  printf '\n<b>💰 TOTAL:</b> <code>%s</code> kWh\n<b>💶 Cost:</b> <code>%s</code> %s' \
    "$total_kwh" "$cost" "$CURRENCY"

  # Export for callers
  _REPORT_TOTAL_KWH="$total_kwh"
  _REPORT_COST="$cost"
}

generate_report() {
  local title="$1" label="$2" source_file="$3"
  [[ -f "$source_file" ]] || return 0
  local body
  body=$(_build_report_body "$source_file")
  local msg="<b>${title}</b>\n<b>Host:</b> <code>${HOST_LABEL}</code>\n<b>${label}</b>\n\n${body}"
  send_telegram "$msg"
}

generate_status_report() {
  generate_report "📊 Status Update ($1)" "" "$TODAY_FILE"
}

generate_daily_report() {
  local date="$1"
  local file="$STATE_DIR/today_${date}.env"
  generate_report "📅 Daily Energy Report" "Date: <code>${date}</code>" "$file"
  echo "[$(date '+%F %T')] REPORT ${date} kWh=${_REPORT_TOTAL_KWH} cost=${_REPORT_COST} ${CURRENCY}" >> "$LOG_FILE"
  echo "$date" > "$LAST_REPORT_FILE"
}

maybe_send_scheduled_report() {
  local now_h now_m today last_sent
  now_h="$(date +%H)" now_m="$(date +%M)" today="$(date +%F)"

  last_sent=""; [[ -f "$LAST_REPORT_FILE" ]] && last_sent="$(cat "$LAST_REPORT_FILE")"
  if [[ "$now_h" == "$(printf '%02d' "$REPORT_HOUR")" \
     && "$now_m" == "$(printf '%02d' "$REPORT_MINUTE")" \
     && "$last_sent" != "$today" ]]; then
    generate_daily_report "$today"
  fi

  [[ "$TELEGRAM_REPORT_INTERVAL_HOURS" -gt 0 && "$now_m" == "00" ]] || return 0
  (( now_h % TELEGRAM_REPORT_INTERVAL_HOURS == 0 )) || return 0
  [[ "$now_h" != "$(printf '%02d' "$REPORT_HOUR")" ]] || return 0
  local int_file="$STATE_DIR/last_interval_report"
  local last_int=""; [[ -f "$int_file" ]] && last_int="$(cat "$int_file")"
  if [[ "$last_int" != "${today}_${now_h}" ]]; then
    generate_status_report "${now_h}:00"
    echo "${today}_${now_h}" > "$int_file"
  fi
}

rollover_if_new_day() {
  local today stored
  today="$(date +%F)"
  stored="$(basename "$TODAY_FILE" | sed 's/^today_//;s/\.env$//')"
  if [[ "$today" != "$stored" ]]; then
    generate_daily_report "$stored"
    load_today
  fi
}

# --- STARTUP ---
load_state
load_today

[[ "${1:-}" == "--test-report" ]] && { generate_status_report "MANUAL-TEST"; exit 0; }

send_telegram "🚀 <b>Server Power Monitor</b> started on ${HOST_LABEL}. Monitoring ${#SENSOR_PATHS[@]} sensors."

# --- MAIN LOOP ---
# ANSI colors
C_GRAY=$'\e[90m' C_CYAN=$'\e[36m' C_GREEN=$'\e[32m' C_YELLOW=$'\e[33m' C_BOLD=$'\e[1m' C_RESET=$'\e[0m'

while true; do
  sleep "$SAMPLE_INTERVAL"
  rollover_if_new_day

  local_ts=$(date +%s)
  kvs_today=("DATE=$(date +%F)") kvs_state=()
  total_watts=0 total_kwh=0

  for id in "${!SENSOR_PATHS[@]}"; do
    v_uj="LAST_UJ_$id"; v_ts="LAST_TS_$id"; v_j="J_$id"; v_peak="PEAK_$id"
    last_uj="${!v_uj}" last_ts="${!v_ts}"
    cur_j="${!v_j:-0}" cur_peak="${!v_peak:-0}"

    delta_s=$(( local_ts - last_ts )); (( delta_s > 0 )) || delta_s=1
    delta_j=0 watts=0 cur_uj=0

    case "${SENSOR_TYPES[$id]}" in
      rapl)
        cur_uj=$(cat "${SENSOR_PATHS[$id]}")
        delta_uj=$(( cur_uj - last_uj ))
        if (( delta_uj < 0 )); then
          max_uj="${SENSOR_MAX_ENERGY[$id]:-0}"
          (( max_uj > 0 )) && delta_uj=$(( max_uj - last_uj + cur_uj )) || delta_uj=0
        fi
        delta_j=$(awk -v u="$delta_uj" 'BEGIN { printf "%.6f", u/1e6 }')
        watts=$(awk -v j="$delta_j" -v s="$delta_s" 'BEGIN { printf "%.2f", j/s }')
        ;;
      nvidia)
        watts=$(nvidia-smi --query-gpu=power.draw --format=csv,noheader,nounits \
                  --id="${SENSOR_PATHS[$id]}" || echo 0)
        delta_j=$(awk -v w="$watts" -v s="$delta_s" 'BEGIN { printf "%.6f", w*s }')
        ;;
      disk)
        dev="/dev/${id#disk_}"
        is_rot=$(cat "${SENSOR_PATHS[$id]}/queue/rotational" 2>/dev/null || echo 0)
        if [[ "$is_rot" == 1 ]] && command -v hdparm &>/dev/null; then
          status=$(hdparm -C "$dev" 2>/dev/null || echo "unknown")
          [[ "$status" == *standby* ]] && watts="$HDD_STANDBY_W" || watts="$HDD_ACTIVE_W"
        else
          cur_uj=$(awk '{print $3+$7}' "${SENSOR_PATHS[$id]}/stat" 2>/dev/null || echo 0)
          delta_io=$(( cur_uj - last_uj ))
          (( delta_io > 0 )) && watts="$SSD_ACTIVE_W" || watts="$SSD_IDLE_W"
        fi
        delta_j=$(awk -v w="$watts" -v s="$delta_s" 'BEGIN { printf "%.6f", w*s }')
        ;;
    esac

    new_j=$(awk_sum "$cur_j" "$delta_j")
    new_peak=$(awk_max "$cur_peak" "$watts")

    kvs_today+=("J_$id=$new_j" "PEAK_$id=$new_peak")
    kvs_state+=("$v_uj=$cur_uj" "$v_ts=$local_ts")

    printf -v "$v_j"    '%s' "$new_j"
    printf -v "$v_peak" '%s' "$new_peak"
    printf -v "$v_uj"   '%s' "$cur_uj"
    printf -v "$v_ts"   '%s' "$local_ts"

    SENSOR_WATTS[$id]="$watts"
    total_watts=$(awk_sum "$total_watts" "$watts")
    total_kwh=$(awk -v a="$total_kwh" -v b="$new_j" 'BEGIN { printf "%.4f", a + b/3600000 }')
  done

  save_kv "$TODAY_FILE" "${kvs_today[@]}"
  save_kv "$STATE_FILE" "${kvs_state[@]}"

  # Terminal output
  power_icon="🔌"
  [[ -f /sys/class/power_supply/ADP1/online ]] \
    && [[ $(cat /sys/class/power_supply/ADP1/online) == 0 ]] && power_icon="🔋"

  bat_level=""
  [[ -f /sys/class/power_supply/BAT1/capacity ]] \
    && bat_level="($(cat /sys/class/power_supply/BAT1/capacity)%)"

  cost=$(calc_cost "$total_kwh")
  status_line=""
  declare -A seen_names=()

  has_package=0
  for sid in "${!SENSOR_NAMES[@]}"; do
    [[ "${SENSOR_NAMES[$sid]}" == package* ]] && has_package=1 && break
  done

  for id in "${!SENSOR_PATHS[@]}"; do
    raw="${SENSOR_NAMES[$id]}"
    [[ "$raw" == core* && $has_package -eq 1 ]] && continue
    [[ -n "${seen_names[$raw]:-}" ]] && continue
    seen_names[$raw]=1
    friendly=$(get_friendly_name "$id")
    color=$([[ "${SENSOR_TYPES[$id]}" == nvidia ]] && echo "$C_GREEN" || echo "$C_CYAN")
    status_line+="${friendly}: ${color}$(printf "%.1f" "${SENSOR_WATTS[$id]}")W${C_RESET} | "
  done

  line_args=("$C_GRAY" "$(date '+%H:%M')" "$C_RESET" "$power_icon" "$bat_level" \
             "$status_line" "$C_BOLD" "$C_RESET" "$C_YELLOW" "$total_watts" "$C_RESET" \
             "$total_kwh" "$C_GREEN" "$cost" "$C_RESET" "$CURRENCY")
  if [[ -t 1 ]]; then
    printf "\r%s[%s]%s %s%s | %b%sTOT:%s%s%5.1fW%s | %7.4fkWh | %s%7.4f%s%s  " "${line_args[@]}"
  else
    printf "%s[%s]%s %s%s | %b%sTOT:%s%s%5.1fW%s | %7.4fkWh | %s%7.4f%s%s\n" "${line_args[@]}"
  fi

  maybe_send_scheduled_report
done