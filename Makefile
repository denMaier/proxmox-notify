PREFIX ?= /usr/local
SYSCONFDIR ?= /etc
DESTDIR ?=

BINDIR := $(PREFIX)/bin
LIBDIR := $(PREFIX)/lib/proxmox-notify
SYSTEMD_DIR := $(PREFIX)/lib/systemd/system

.PHONY: install package test ci

install:
	install -d "$(DESTDIR)$(BINDIR)" "$(DESTDIR)$(LIBDIR)/helpers" "$(DESTDIR)$(SYSTEMD_DIR)" "$(DESTDIR)$(SYSCONFDIR)/proxmox-notify"
	install -m755 bin/proxmox-notify "$(DESTDIR)$(BINDIR)/proxmox-notify"
	install -m644 lib/proxmox-notify/proxmox-notify.sh "$(DESTDIR)$(LIBDIR)/proxmox-notify.sh"
	install -m755 lib/proxmox-notify/helpers/toml.py "$(DESTDIR)$(LIBDIR)/helpers/toml.py"
	install -m644 systemd/proxmox-notify-announce.service "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-announce.service"
	install -m644 systemd/proxmox-notify-watch@.path "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-watch@.path"
	install -m644 systemd/proxmox-notify-watch@.service "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-watch@.service"
	install -m644 systemd/proxmox-notify-reconcile@.timer "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-reconcile@.timer"
	install -m644 systemd/proxmox-notify-reconcile@.service "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-reconcile@.service"
	install -m644 config/config.toml "$(DESTDIR)$(SYSCONFDIR)/proxmox-notify/config.toml"

package:
	scripts/build-deb

test:
	tests/smoke.sh

ci:
	bash -n bin/proxmox-notify
	bash -n lib/proxmox-notify/proxmox-notify.sh
	bash -n scripts/build-deb
	bash -n tests/smoke.sh
	bash -n tests/installed-e2e.sh
	sh -n packaging/postinst
	sh -n packaging/postrm
	python3 -m py_compile lib/proxmox-notify/helpers/toml.py
	$(MAKE) test
