# Makefile for Server Power Monitor (Rust Edition)

PREFIX ?= /usr/local
BINDIR = $(PREFIX)/bin
CONFDIR = /etc
SYSTEMDDIR = /etc/systemd/system

BINARY = server-power-monitor
CONFIG = server-power-monitor.env
SERVICE = server-power-monitor.service

.PHONY: all build install uninstall run clean

all: build

build:
	@echo "Building Rust binary in release mode..."
	cargo build --release

install: build
	@echo "Installing system binary and service..."
	install -D -m 755 target/release/$(BINARY) $(BINDIR)/$(BINARY)
	@if [ ! -f $(CONFDIR)/$(CONFIG) ]; then \
		cp .env.example $(CONFDIR)/$(CONFIG); \
		echo "Created $(CONFDIR)/$(CONFIG) from example values."; \
	fi

	sed "s|ExecStart=.*|ExecStart=$(BINDIR)/$(BINARY)|" $(SERVICE) > $(SERVICE).tmp
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
	rm -f $(BINDIR)/$(BINARY)
	rm -f $(SYSTEMDDIR)/$(SERVICE)
	systemctl daemon-reload
	@echo "Removed binary and service. Configuration file $(CONFDIR)/$(CONFIG) was kept."

run: build
	@echo "Starting local execution..."
	./target/release/$(BINARY)

clean:
	cargo clean
	rm -rf state/ server-power-monitor.log
