# Makefile for Server Power Monitor


PREFIX ?= /usr/local
BINDIR = $(PREFIX)/bin
CONFDIR = /etc
SYSTEMDDIR = /etc/systemd/system

SCRIPT = server-power-monitor.sh
CONFIG = server-power-monitor.conf
SERVICE = server-power-monitor.service

.PHONY: install uninstall run clean

install:
	@echo "Installing..."

	install -D -m 755 $(SCRIPT) $(BINDIR)/$(SCRIPT)
	@if [ ! -f $(CONFDIR)/$(CONFIG) ]; then \
		cp server-power-monitor.conf.example $(CONFDIR)/$(CONFIG); \
		echo "Created $(CONFDIR)/$(CONFIG) from example values."; \
	fi

	# Update the path in the service file before installing it

	sed "s|ExecStart=.*|ExecStart=$(BINDIR)/$(SCRIPT)|" $(SERVICE) > $(SERVICE).tmp
	install -D -m 644 $(SERVICE).tmp $(SYSTEMDDIR)/$(SERVICE)
	rm $(SERVICE).tmp
	systemctl daemon-reload
	@echo "Installazione completata."

	@echo "Configura il file $(CONFDIR)/$(CONFIG) e avvia il servizio con:"
	@echo "systemctl enable --now $(SERVICE)"

uninstall:
	@echo "Uninstalling..."

	systemctl stop $(SERVICE) || true
	systemctl disable $(SERVICE) || true
	rm -f $(BINDIR)/$(SCRIPT)
	rm -f $(SYSTEMDDIR)/$(SERVICE)
	systemctl daemon-reload
	@echo "Removed script and service. Configuration file $(CONFDIR)/$(CONFIG) was kept."


run:
	@echo "Starting in local mode..."

	bash $(SCRIPT)

clean:
	rm -rf state/ server-power-monitor.log
