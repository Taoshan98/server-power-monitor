#!/usr/bin/env bash
set -euo pipefail
export LC_NUMERIC=C

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Configuration Priority: 
# 1. Environment Variable CONFIG_FILE
# 2. Local server-power-monitor.conf
# 3. System /etc/server-power-monitor.conf
if [[ -z "${CONFIG_FILE:-}" ]]; then
  if [[ -f "$SCRIPT_DIR/server-power-monitor.conf" ]]; then
    CONFIG_FILE="$SCRIPT_DIR/server-power-monitor.conf"
  else
    CONFIG_FILE="/etc/server-power-monitor.conf"
  fi
fi

# State Directory Priority:
# 1. Environment Variable STATE_DIR
# 2. ./state (if local .conf exists or if not in a systemd service)
# 3. /var/lib/server-power-monitor (system default)

if [[ -z "${STATE_DIR:-}" ]]; then
  if [[ -f "$SCRIPT_DIR/server-power-monitor.conf" ]] || [[ -z "${INVOCATION_ID:-}" ]]; then
    STATE_DIR="$SCRIPT_DIR/state"
  else
    STATE_DIR="/var/lib/server-power-monitor"
  fi
fi

# Log Priority:
# 1. Environment Variable LOG_FILE
# 2. ./server-power-monitor.log
# 3. /var/log/server-power-monitor.log

if [[ -z "${LOG_FILE:-}" ]]; then
  if [[ -f "$SCRIPT_DIR/server-power-monitor.conf" ]] || [[ -z "${INVOCATION_ID:-}" ]]; then
    LOG_FILE="$SCRIPT_DIR/server-power-monitor.log"
  else
    LOG_FILE="/var/log/server-power-monitor.log"
  fi
fi


mkdir -p "$STATE_DIR"

if [[ -f "$CONFIG_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$CONFIG_FILE"
fi

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


# Constants for disk power estimation (Watts)

: "${HDD_ACTIVE_W:=5.0}"
: "${HDD_STANDBY_W:=0.5}"
: "${SSD_ACTIVE_W:=2.5}"
: "${SSD_IDLE_W:=0.3}"


# --- SENSOR DISCOVERY ---

declare -A SENSOR_PATHS
declare -A SENSOR_NAMES
declare -A SENSOR_TYPES # "rapl" or "nvidia"
declare -A SENSOR_MAX_ENERGY
declare -A SENSOR_WATTS


# 1. Discover RAPL sensors
# Direct approach to avoid permission issues or find failures on certain kernels

for p in /sys/class/powercap/intel-rapl*/energy_uj /sys/class/powercap/intel-rapl*/*/energy_uj; do
  if [ -e "$p" ]; then
    name_file="$(dirname "$p")/name"
    if [ -f "$name_file" ]; then
      raw_name=$(cat "$name_file" 2>/dev/null || echo "unknown")
      
      # Generate a unique ID based on the path to avoid collisions

      path_id=$(basename "$(dirname "$p")" | tr -cd '[:alnum:]_')
      id="rapl_${path_id}"
      
      SENSOR_PATHS[$id]="$p"
      SENSOR_NAMES[$id]="$raw_name"
      SENSOR_TYPES[$id]="rapl"
      
      max_file="$(dirname "$p")/max_energy_range_uj"
      if [ -f "$max_file" ]; then
        SENSOR_MAX_ENERGY[$id]=$(cat "$max_file" 2>/dev/null || echo 0)
      fi
    fi
  fi
done




# 2. Discover NVIDIA sensors
if command -v nvidia-smi &>/dev/null; then
  gpu_count=$(nvidia-smi --query-gpu=count --format=csv,noheader,nounits | head -n 1 || echo 0)
  for ((i=0; i<gpu_count; i++)); do
    gpu_name=$(nvidia-smi --query-gpu=name --format=csv,noheader,nounits --id=$i)
    id="nvidia_gpu_$i"
    SENSOR_PATHS[$id]="$i" # Use index as path
    SENSOR_NAMES[$id]="$gpu_name"
    SENSOR_TYPES[$id]="nvidia"
  done
fi

# 3. Discover Disks (Estimation)
for d in /sys/block/sd* /sys/block/nvme*; do
  [[ -e "$d" ]] || continue
  name=$(basename "$d")
  # Skip partitions
  [[ "$name" == *p[0-9]* ]] && [[ "$name" == nvme* ]] && continue
  [[ "$name" =~ [0-9]$ ]] && [[ "$name" == sd* ]] && continue
  
  id="disk_$name"
  SENSOR_PATHS[$id]="$d"
  SENSOR_TYPES[$id]="disk"
  rot=$(cat "$d/queue/rotational" 2>/dev/null || echo "0")
  if [[ "$rot" == "1" ]]; then
    SENSOR_NAMES[$id]="HDD $name"
  else
    SENSOR_NAMES[$id]="SSD $name"
  fi
done


if [[ ${#SENSOR_PATHS[@]} -eq 0 ]]; then
  echo "ERROR: No energy sensors found (Intel RAPL or NVIDIA)." >&2
  exit 1
fi


STATE_FILE="$STATE_DIR/state.env"
TODAY_FILE="$STATE_DIR/today_$(date +%F).env"
LAST_REPORT_FILE="$STATE_DIR/last_report_date"

# --- UTILS ---

get_friendly_name() {
  local id="$1"
  local raw="${SENSOR_NAMES[$id]:-}"
  case "$raw" in
    package*) echo "🔳 CPU" ;;
    core*)    echo "🧠 Cores" ;;
    uncore*)  echo "🎨 iGPU" ;;
    dram*)    echo "📟 RAM" ;;
    psys*)    echo "💻 System" ;;
    *SSD*|*nvme*) echo "📀 $raw" ;;
    *HDD*)        echo "💿 $raw" ;;
    *)       
      if [[ "$id" == nvidia* ]]; then
        echo "🎨 GPU"
      else
        echo "${raw:-$id}"
      fi
      ;;
  esac
}




send_telegram() {

  local text="$1"
  [[ "$TELEGRAM_ENABLED" == "1" ]] || return 0
  [[ -n "$TELEGRAM_BOT_TOKEN" && -n "$TELEGRAM_CHAT_ID" ]] || return 0
  curl -fsS -X POST "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/sendMessage" \
    --data-urlencode "chat_id=${TELEGRAM_CHAT_ID}" \
    --data-urlencode "text=${text}" \
    --data-urlencode "parse_mode=Markdown" >/dev/null || true
}

save_kv() {
  local file="$1"
  shift
  : > "$file"
  for kv in "$@"; do
    echo "$kv" >> "$file"
  done
}

load_state() {
  if [[ -f "$STATE_FILE" ]]; then
    # shellcheck disable=SC1090
    source "$STATE_FILE"
  fi
  
  # Initialize missing sensors in state
  local changed=0
  for id in "${!SENSOR_PATHS[@]}"; do
    last_uj_var="LAST_UJ_$id"
    last_ts_var="LAST_TS_$id"
    if [[ -z "${!last_uj_var:-}" ]]; then
      if [[ "${SENSOR_TYPES[$id]}" == "rapl" ]]; then
        eval "$last_uj_var=$(cat "${SENSOR_PATHS[$id]}")"
      elif [[ "${SENSOR_TYPES[$id]}" == "disk" ]]; then
        # For disks, store last I/O sectors
        eval "$last_uj_var=$(awk '{print $3+$7}' "${SENSOR_PATHS[$id]}/stat" 2>/dev/null || echo 0)"
      else
        eval "$last_uj_var=0"
      fi
      eval "$last_ts_var=$(date +%s)"
      changed=1
    fi

  done
  
  if [[ $changed -eq 1 ]]; then
    local kvs=()
    for id in "${!SENSOR_PATHS[@]}"; do
      last_uj_var="LAST_UJ_$id"
      last_ts_var="LAST_TS_$id"
      kvs+=("$last_uj_var=${!last_uj_var}")
      kvs+=("$last_ts_var=${!last_ts_var}")
    done
    save_kv "$STATE_FILE" "${kvs[@]}"
  fi
}

ensure_today_file() {
  local current_date
  current_date="$(date +%F)"
  TODAY_FILE="$STATE_DIR/today_${current_date}.env"
  if [[ ! -f "$TODAY_FILE" ]]; then
    local kvs=("DATE=$current_date")
    for id in "${!SENSOR_PATHS[@]}"; do
      kvs+=("J_$id=0")
      kvs+=("PEAK_$id=0")
    done
    save_kv "$TODAY_FILE" "${kvs[@]}"
  fi
}

load_today() {
  ensure_today_file
  # shellcheck disable=SC1090
  source "$TODAY_FILE"
}

calc_cost() {
  local kwh="$1"
  awk -v kwh="$kwh" -v tariff="$TARIFF_EUR_KWH" 'BEGIN { printf "%.4f", kwh * tariff }'
}

generate_status_report() {
  local label="$1"
  load_today
  local msg="📊 *Status Update* (${label})%0AHost: ${HOST_LABEL}%0A"
  local total_j=0

  
  for id in "${!SENSOR_PATHS[@]}"; do
    j_var="J_$id"
    peak_var="PEAK_$id"
    local j="${!j_var:-0}"
    local peak="${!peak_var:-0}"
    total_j=$(awk -v a="$total_j" -v b="$j" 'BEGIN { print a+b }')
    
    local friendly=$(get_friendly_name "$id")
    local kwh=$(awk -v j="$j" 'BEGIN { printf "%.4f", j/3600000 }')
    msg+="%0A*${friendly}*:%0A- Consumption: ${kwh} kWh%0A- Peak: ${peak} W"


  done
  
  local total_kwh=$(awk -v j="$total_j" 'BEGIN { printf "%.4f", j/3600000 }')
  local cost=$(calc_cost "$total_kwh")
  msg+="%0A%0A*TOTAL*: ${total_kwh} kWh%0A*Cost*: ${cost} ${CURRENCY}"

  
  send_telegram "$msg"
}

generate_daily_report() {
  local report_date="$1"
  local report_file="$STATE_DIR/today_${report_date}.env"
  [[ -f "$report_file" ]] || return 0
  
  # shellcheck disable=SC1090
  source "$report_file"
  
  local msg="🔌 *Daily Energy Report*%0AHost: ${HOST_LABEL}%0ADate: ${report_date}%0A"
  local total_j=0

  
  for id in "${!SENSOR_PATHS[@]}"; do
    j_var="J_$id"
    peak_var="PEAK_$id"
    local j="${!j_var:-0}"
    local peak="${!peak_var:-0}"
    total_j=$(awk -v a="$total_j" -v b="$j" 'BEGIN { print a+b }')
    
    local friendly=$(get_friendly_name "$id")
    local kwh=$(awk -v j="$j" 'BEGIN { printf "%.4f", j/3600000 }')
    msg+="%0A*${friendly}*:%0A- Consumption: ${kwh} kWh%0A- Peak: ${peak} W"


  done
  
  local total_kwh=$(awk -v j="$total_j" 'BEGIN { printf "%.4f", j/3600000 }')
  local cost=$(calc_cost "$total_kwh")
  msg+="%0A%0A*TOTAL*: ${total_kwh} kWh%0A*Estimated Cost*: ${cost} ${CURRENCY}"

  
  echo "[$(date '+%F %T')] REPORT ${report_date} kWh=${total_kwh} cost=${cost} ${CURRENCY}" >> "$LOG_FILE"
  send_telegram "$msg"
  echo "$report_date" > "$LAST_REPORT_FILE"
}

maybe_send_scheduled_report() {
  local now_h
  local now_m
  local today
  local last_sent
  now_h="$(date +%H)"
  now_m="$(date +%M)"
  today="$(date +%F)"

  
  # 1. Daily Report
  last_sent=""
  [[ -f "$LAST_REPORT_FILE" ]] && last_sent="$(cat "$LAST_REPORT_FILE")"
  if [[ "$now_h" == "$(printf '%02d' "$REPORT_HOUR")" && "$now_m" == "$(printf '%02d' "$REPORT_MINUTE")" && "$last_sent" != "$today" ]]; then
    generate_daily_report "$today"
  fi

  # 2. Intermediate Reports (Configurable)
  if [[ "$TELEGRAM_REPORT_INTERVAL_HOURS" -gt 0 ]]; then
    local last_int_file="$STATE_DIR/last_interval_report"
    local last_int=""
    [[ -f "$last_int_file" ]] && last_int="$(cat "$last_int_file")"
    
    if [[ "$now_m" == "00" ]]; then
      if (( now_h % TELEGRAM_REPORT_INTERVAL_HOURS == 0 )); then
        # Avoid sending partial report if it coincides with daily report hour
        if [[ "$now_h" != "$(printf '%02d' "$REPORT_HOUR")" ]]; then
          if [[ "$last_int" != "${today}_${now_h}" ]]; then
            generate_status_report "${now_h}:00"
            echo "${today}_${now_h}" > "$last_int_file"
          fi
        fi
      fi
    fi
  fi


}

rollover_if_new_day() {
  local current_date
  local today_date
  current_date="$(date +%F)"

  today_date="$(basename "$TODAY_FILE" | sed 's/^today_//; s/\.env$//')"
  if [[ "$current_date" != "$today_date" ]]; then
    generate_daily_report "$today_date"
    ensure_today_file
    load_today
  fi
}

load_state
load_today

send_telegram "🚀 *Server Power Monitor* started on ${HOST_LABEL}. Monitoring ${#SENSOR_PATHS[@]} sensors."


while true; do
  sleep "$SAMPLE_INTERVAL"
  rollover_if_new_day
  
  current_ts=$(date +%s)
  kvs_today=("DATE=$(date +%F)")
  kvs_state=()
  
  total_watts=0
  total_kwh=0
  
  for id in "${!SENSOR_PATHS[@]}"; do
    last_uj_var="LAST_UJ_$id"
    last_ts_var="LAST_TS_$id"
    j_var="J_$id"
    peak_var="PEAK_$id"
    
    last_uj="${!last_uj_var}"
    last_ts="${!last_ts_var}"
    current_j_total="${!j_var:-0}"
    current_peak="${!peak_var:-0}"
    
    delta_s=$(( current_ts - last_ts ))
    [[ $delta_s -le 0 ]] && delta_s=1
    
    delta_j=0
    watts=0
    current_uj=0

    
    if [[ "${SENSOR_TYPES[$id]}" == "rapl" ]]; then
      current_uj=$(cat "${SENSOR_PATHS[$id]}")
      delta_uj=$(( current_uj - last_uj ))
      
      if (( delta_uj < 0 )); then
        max_uj="${SENSOR_MAX_ENERGY[$id]:-0}"
        if (( max_uj > 0 )); then
          delta_uj=$(( (max_uj - last_uj) + current_uj ))
        else
          delta_uj=0
        fi
      fi
      delta_j=$(awk -v uj="$delta_uj" 'BEGIN { printf "%.6f", uj/1000000 }')
      watts=$(awk -v j="$delta_j" -v s="$delta_s" 'BEGIN { printf "%.2f", j/s }')
    elif [[ "${SENSOR_TYPES[$id]}" == "nvidia" ]]; then
      # NVIDIA
      watts=$(nvidia-smi --query-gpu=power.draw --format=csv,noheader,nounits --id="${SENSOR_PATHS[$id]}" || echo 0)
      delta_j=$(awk -v w="$watts" -v s="$delta_s" 'BEGIN { printf "%.6f", w*s }')
      current_uj=0
    elif [[ "${SENSOR_TYPES[$id]}" == "disk" ]]; then
      # DISK Estimation
      is_rot=$(cat "${SENSOR_PATHS[$id]}/queue/rotational" 2>/dev/null || echo 0)
      if [[ "$is_rot" == "1" ]] && command -v hdparm &>/dev/null; then
        # HDD: Check spin status
        status=$(hdparm -C "/dev/${id#disk_}" 2>/dev/null || echo "unknown")
        if [[ "$status" == *"standby"* ]]; then
          watts="$HDD_STANDBY_W"
        else
          watts="$HDD_ACTIVE_W"
        fi
      else
        # SSD: Check activity
        current_io=$(awk '{print $3+$7}' "${SENSOR_PATHS[$id]}/stat" 2>/dev/null || echo 0)
        delta_io=$(( current_io - last_uj ))
        if [[ $delta_io -gt 0 ]]; then
          watts="$SSD_ACTIVE_W"
        else
          watts="$SSD_IDLE_W"
        fi
        current_uj="$current_io" # Re-use current_uj to store IO
      fi
      delta_j=$(awk -v w="$watts" -v s="$delta_s" 'BEGIN { printf "%.6f", w*s }')
    fi

    
    # Update totals
    new_j_total=$(awk -v a="$current_j_total" -v b="$delta_j" 'BEGIN { printf "%.6f", a+b }')
    new_peak=$(awk -v p="$current_peak" -v w="$watts" 'BEGIN { if(w>p) print w; else print p }')
    
    kvs_today+=("J_$id=$new_j_total")
    kvs_today+=("PEAK_$id=$new_peak")
    kvs_state+=("LAST_UJ_$id=$current_uj")
    kvs_state+=("LAST_TS_$id=$current_ts")
    
    # Update local vars for current loop
    eval "$j_var=$new_j_total"
    eval "$peak_var=$new_peak"
    eval "$last_uj_var=$current_uj"
    eval "$last_ts_var=$current_ts"
    
    SENSOR_WATTS[$id]="$watts"
    
    total_watts=$(awk -v a="$total_watts" -v b="$watts" 'BEGIN { print a+b }')
    total_kwh=$(awk -v a="$total_kwh" -v b="$new_j_total" 'BEGIN { printf "%.4f", a + (b/3600000) }')
  done

  
  save_kv "$TODAY_FILE" "${kvs_today[@]}"
  save_kv "$STATE_FILE" "${kvs_state[@]}"
  
  # Colori ANSI
  C_GRAY="\e[90m"
  C_CYAN="\e[36m"
  C_GREEN="\e[32m"
  C_YELLOW="\e[33m"
  C_BOLD="\e[1m"
  C_RESET="\e[0m"

  # Power Status
  power_icon="🔌"
  if [[ -f "/sys/class/power_supply/ADP1/online" ]]; then
    [[ $(cat /sys/class/power_supply/ADP1/online) == "0" ]] && power_icon="🔋"
  fi
  
  bat_level=""
  if [[ -f "/sys/class/power_supply/BAT1/capacity" ]]; then
    bat_level="($(cat /sys/class/power_supply/BAT1/capacity)%%)"
  fi

  # Terminal output preparation
  cost=$(calc_cost "$total_kwh")
  
  # Build status line (main sensors only to avoid clutter)
  status_line=""
  declare -A seen_names=()

  
  # Determine if we have a "package" sensor (Total CPU)
  has_package=0
  for sid in "${!SENSOR_NAMES[@]}"; do
    [[ "${SENSOR_NAMES[$sid]}" == package* ]] && has_package=1 && break
  done

  for id in "${!SENSOR_PATHS[@]}"; do
    raw_n="${SENSOR_NAMES[$id]}"
    
    # Skip "core" if "package" is present for clarity
    [[ "$raw_n" == core* && $has_package -eq 1 ]] && continue
    # Skip if we already added a sensor with this exact name
    [[ -n "${seen_names[$raw_n]:-}" ]] && continue
    # Skip system duplicates if present
    [[ "$raw_n" == psys* && "$status_line" == *"System"* ]] && continue

    seen_names[$raw_n]=1
    friendly=$(get_friendly_name "$id")


    color=$C_CYAN
    [[ "${SENSOR_TYPES[$id]}" == "nvidia" ]] && color=$C_GREEN
    status_line+="${friendly}: ${color}$(printf "%.1f" "${SENSOR_WATTS[$id]}")W${C_RESET} | "
  done

  # Handle output: use \r only for TTY, \n for logs/Docker
  if [[ -t 1 ]]; then
    printf "\r${C_GRAY}[%s]${C_RESET} ${power_icon}${bat_level} | %b${C_BOLD}TOT:${C_RESET}${C_YELLOW}%5.1fW${C_RESET} | %7.4fkWh | ${C_GREEN}%7.4f%s${C_RESET}  " \
      "$(date '+%H:%M')" "$status_line" "$total_watts" "$total_kwh" "$cost" "$CURRENCY"
  else
    printf "${C_GRAY}[%s]${C_RESET} ${power_icon}${bat_level} | %b${C_BOLD}TOT:${C_RESET}${C_YELLOW}%5.1fW${C_RESET} | %7.4fkWh | ${C_GREEN}%7.4f%s${C_RESET}\n" \
      "$(date '+%H:%M')" "$status_line" "$total_watts" "$total_kwh" "$cost" "$CURRENCY"
  fi





  
  maybe_send_scheduled_report
done


