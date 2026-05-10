#!/usr/bin/env bash
# Improved installation script for Server Power Monitor

set -e

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_TARGET="/etc/server-power-monitor.conf"
LOCAL_CONFIG="$PROJECT_DIR/server-power-monitor.conf"

echo "--- Server Power Monitor Setup ---"

# Load existing configuration as base if available
# shellcheck disable=SC1090
if [ -f "$LOCAL_CONFIG" ]; then
    source "$LOCAL_CONFIG"
elif [ -f "$CONFIG_TARGET" ]; then
    source "$CONFIG_TARGET"
fi


# Ask for confirmation for system or local installation
read -r -p "Install as a system service? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    read -r -p "Enter installation prefix (default: /usr/local): " PREFIX
    PREFIX=${PREFIX:-/usr/local}
    echo "Installing to system in $PREFIX..."
    
    # Use Makefile for system installation
    sudo make install PREFIX="$PREFIX"

    # Telegram configuration
    if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
        echo ""
        read -r -p "Configure Telegram now? (y/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            read -r -p "Enter Telegram Bot Token: " TELEGRAM_BOT_TOKEN
            read -r -p "Enter Telegram Chat ID: " TELEGRAM_CHAT_ID
            read -r -p "Interval for intermediate reports in hours? (default 6): " TELEGRAM_REPORT_INTERVAL_HOURS
            TELEGRAM_REPORT_INTERVAL_HOURS=${TELEGRAM_REPORT_INTERVAL_HOURS:-6}

            
            sudo sed -i "s/TELEGRAM_ENABLED=0/TELEGRAM_ENABLED=1/" "$CONFIG_TARGET"
            sudo sed -i "s/TELEGRAM_BOT_TOKEN=\"\"/TELEGRAM_BOT_TOKEN=\"$TELEGRAM_BOT_TOKEN\"/" "$CONFIG_TARGET"
            sudo sed -i "s/TELEGRAM_CHAT_ID=\"\"/TELEGRAM_CHAT_ID=\"$TELEGRAM_CHAT_ID\"/" "$CONFIG_TARGET"
            
            # Add or update the interval
            if grep -q "TELEGRAM_REPORT_INTERVAL_HOURS" "$CONFIG_TARGET"; then
                sudo sed -i "s/TELEGRAM_REPORT_INTERVAL_HOURS=.*/TELEGRAM_REPORT_INTERVAL_HOURS=$TELEGRAM_REPORT_INTERVAL_HOURS/" "$CONFIG_TARGET"
            else
                echo "TELEGRAM_REPORT_INTERVAL_HOURS=$TELEGRAM_REPORT_INTERVAL_HOURS" | sudo tee -a "$CONFIG_TARGET" > /dev/null
            fi
        fi
    fi

    sudo systemctl restart server-power-monitor.service
    
    echo "Installation completed! Verify with: sudo systemctl status server-power-monitor.service"

else
    echo "Local mode: you can start the script with 'sudo bash server-power-monitor.sh'"
    echo "Make sure to create a local 'server-power-monitor.conf' if you want to customize parameters."
fi
