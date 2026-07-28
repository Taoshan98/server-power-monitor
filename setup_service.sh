#!/usr/bin/env bash
# Installation script for Server Power Monitor (Rust Edition)

set -e

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_TARGET="/etc/server-power-monitor.env"
LOCAL_CONFIG="$PROJECT_DIR/.env"

echo "--- 🔌 Server Power Monitor (Rust Edition) Setup ---"

# Check for cargo/rust toolchain
if ! command -v cargo &>/dev/null; then
    echo "❌ Errore: Cargo/Rust non è installato sul sistema."
    echo "Installa Rust eseguendo: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Load existing configuration as base if available
if [ -f "$LOCAL_CONFIG" ]; then
    # shellcheck source=/dev/null
    source "$LOCAL_CONFIG"
elif [ -f "$CONFIG_TARGET" ]; then
    # shellcheck source=/dev/null
    source "$CONFIG_TARGET"
fi

# Ask for confirmation for system or local installation
read -r -p "Installare come servizio di sistema systemd? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    read -r -p "Inserisci il prefisso di installazione (default: /usr/local): " PREFIX
    PREFIX=${PREFIX:-/usr/local}
    echo "Compilazione e installazione in $PREFIX..."
    
    # Use Makefile for system installation
    sudo make install PREFIX="$PREFIX"

    # Configure firewall (UFW) automatically if active
    if command -v ufw &>/dev/null && sudo ufw status 2>/dev/null | grep -q "Status: active"; then
        echo "🔓 Apertura automatica porta 7432/udp nel firewall UFW..."
        sudo ufw allow 7432/udp &>/dev/null || true
    fi

    # Telegram configuration
    if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
        echo ""
        read -r -p "Configurare Telegram adesso? (y/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            read -r -p "Inserisci Telegram Bot Token: " TELEGRAM_BOT_TOKEN
            read -r -p "Inserisci Telegram Chat ID: " TELEGRAM_CHAT_ID
            read -r -p "Intervallo report intermedi in ore (default 6): " TELEGRAM_REPORT_INTERVAL_HOURS
            TELEGRAM_REPORT_INTERVAL_HOURS=${TELEGRAM_REPORT_INTERVAL_HOURS:-6}

            sudo sed -i "s/TELEGRAM_ENABLED=0/TELEGRAM_ENABLED=1/" "$CONFIG_TARGET"
            sudo sed -i "s/TELEGRAM_BOT_TOKEN=\"\"/TELEGRAM_BOT_TOKEN=\"$TELEGRAM_BOT_TOKEN\"/" "$CONFIG_TARGET"
            sudo sed -i "s/TELEGRAM_CHAT_ID=\"\"/TELEGRAM_CHAT_ID=\"$TELEGRAM_CHAT_ID\"/" "$CONFIG_TARGET"
            
            if grep -q "TELEGRAM_REPORT_INTERVAL_HOURS" "$CONFIG_TARGET"; then
                sudo sed -i "s/TELEGRAM_REPORT_INTERVAL_HOURS=.*/TELEGRAM_REPORT_INTERVAL_HOURS=$TELEGRAM_REPORT_INTERVAL_HOURS/" "$CONFIG_TARGET"
            else
                echo "TELEGRAM_REPORT_INTERVAL_HOURS=$TELEGRAM_REPORT_INTERVAL_HOURS" | sudo tee -a "$CONFIG_TARGET" > /dev/null
            fi
        fi
    fi

    sudo systemctl restart server-power-monitor.service
    echo "✅ Installazione completata! Verifica lo stato con: sudo systemctl status server-power-monitor.service"

else
    echo "Modalità Locale: puoi compilare ed avviare l'applicazione con 'cargo run --release'"
    echo "Assicurati di avere un file '.env' locale se desideri personalizzare i parametri."
fi
