PREFIX ?= $(HOME)/.local
BINDIR = $(PREFIX)/bin
AUTOSTART_DIR = $(HOME)/.config/autostart
SYSTEMD_DIR = $(HOME)/.config/systemd/user
BIN_SOURCE_DIR ?= $(if $(wildcard ./ccvv-linux),.,$(if $(wildcard ../core/target/release/ccvv-linux),../core/target/release,$(if $(wildcard core/target/release/ccvv-linux),core/target/release,../target/release)))
ICON_SOURCE_DIR ?= $(if $(wildcard ./icons),icons,linux/icons)
ICON_DIR = $(PREFIX)/share/icons/hicolor/scalable/apps
ICONS = ccvv-active.svg ccvv-paused.svg ccvv-limited.svg ccvv-error.svg ccvv-success.svg

.PHONY: install install-icons uninstall autostart-enable autostart-disable

install: install-icons
	mkdir -p "$(BINDIR)"
	install -m755 "$(BIN_SOURCE_DIR)/ccvv-linux" "$(BINDIR)/ccvv-linux"
	install -m755 "$(BIN_SOURCE_DIR)/ccvv-tray-sni" "$(BINDIR)/ccvv-tray-sni"
	@if [ -f "$(BIN_SOURCE_DIR)/ccvv" ]; then \
		install -m755 "$(BIN_SOURCE_DIR)/ccvv" "$(BINDIR)/ccvv"; \
	fi
	@echo "Installed to $(BINDIR). Run 'make autostart-enable' to start on login."

install-icons:
	@mkdir -p "$(ICON_DIR)"
	@for icon in $(ICONS); do \
		install -m644 "$(ICON_SOURCE_DIR)/$$icon" "$(ICON_DIR)/$$icon"; \
	done
	@echo "Icons installed to $(ICON_DIR)"

uninstall:
	rm -f "$(BINDIR)/ccvv-linux" "$(BINDIR)/ccvv-tray-sni" "$(BINDIR)/ccvv"
	@for icon in $(ICONS); do rm -f "$(ICON_DIR)/$$icon"; done
	rm -f "$(AUTOSTART_DIR)/ccvv-linux.desktop"
	rm -f "$(SYSTEMD_DIR)/ccvv-linux.service"
	-systemctl --user disable ccvv-linux.service 2>/dev/null
	@echo "Uninstalled ccvv-linux."

autostart-enable:
	mkdir -p "$(AUTOSTART_DIR)"
	install -m644 ccvv-linux.desktop "$(AUTOSTART_DIR)/ccvv-linux.desktop"
	@echo "Autostart enabled via XDG desktop entry."

autostart-disable:
	rm -f "$(AUTOSTART_DIR)/ccvv-linux.desktop"
	-systemctl --user disable ccvv-linux.service 2>/dev/null
	@echo "Autostart disabled."
