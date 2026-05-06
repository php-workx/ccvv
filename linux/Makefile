PREFIX ?= $(HOME)/.local
BINDIR = $(PREFIX)/bin
AUTOSTART_DIR = $(HOME)/.config/autostart
SYSTEMD_DIR = $(HOME)/.config/systemd/user

.PHONY: install uninstall autostart-enable autostart-disable

install:
	install -Dm755 ../target/release/ccvv-linux $(BINDIR)/ccvv-linux
	install -Dm755 ../target/release/ccvv-tray-sni $(BINDIR)/ccvv-tray-sni
	@if [ -f ../target/release/ccvv ]; then \
		install -Dm755 ../target/release/ccvv $(BINDIR)/ccvv; \
	elif [ -f ./ccvv ]; then \
		install -Dm755 ./ccvv $(BINDIR)/ccvv; \
	fi
	@echo "Installed to $(BINDIR). Run 'make autostart-enable' to start on login."

uninstall:
	rm -f $(BINDIR)/ccvv-linux $(BINDIR)/ccvv-tray-sni $(BINDIR)/ccvv
	rm -f $(AUTOSTART_DIR)/ccvv-linux.desktop
	rm -f $(SYSTEMD_DIR)/ccvv-linux.service
	-systemctl --user disable ccvv-linux.service 2>/dev/null
	@echo "Uninstalled ccvv-linux."

autostart-enable:
	install -Dm644 ccvv-linux.desktop $(AUTOSTART_DIR)/ccvv-linux.desktop
	@echo "Autostart enabled via XDG desktop entry."

autostart-disable:
	rm -f $(AUTOSTART_DIR)/ccvv-linux.desktop
	-systemctl --user disable ccvv-linux.service 2>/dev/null
	@echo "Autostart disabled."
