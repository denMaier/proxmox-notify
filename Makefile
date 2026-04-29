PREFIX ?= /usr/local
SYSCONFDIR ?= /etc
DESTDIR ?=
CARGO ?= cargo
BUILD_PROFILE ?= release

BINDIR := $(PREFIX)/bin
SYSTEMD_DIR := $(PREFIX)/lib/systemd/system
PROFILE_DIR := $(if $(filter release,$(BUILD_PROFILE)),release,debug)
CARGO_PROFILE_FLAG := $(if $(filter release,$(BUILD_PROFILE)),--release,)
TARGET_DIR := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target)
BINARY := $(TARGET_DIR)/$(PROFILE_DIR)/proxmox-notify
DEBUG_BINARY := $(TARGET_DIR)/debug/proxmox-notify
SMOKE_BINARY := $(if $(filter /%,$(DEBUG_BINARY)),$(DEBUG_BINARY),$(CURDIR)/$(DEBUG_BINARY))

.PHONY: build install package test ci

build:
	$(CARGO) build $(CARGO_PROFILE_FLAG)

install: build
	install -d "$(DESTDIR)$(BINDIR)" "$(DESTDIR)$(SYSTEMD_DIR)" "$(DESTDIR)$(SYSCONFDIR)/proxmox-notify"
	install -m755 "$(BINARY)" "$(DESTDIR)$(BINDIR)/proxmox-notify"
	install -m644 systemd/proxmox-notify-announce.service "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-announce.service"
	install -m644 systemd/proxmox-notify-watch@.path "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-watch@.path"
	install -m644 systemd/proxmox-notify-watch@.service "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-watch@.service"
	install -m644 systemd/proxmox-notify-reconcile@.timer "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-reconcile@.timer"
	install -m644 systemd/proxmox-notify-reconcile@.service "$(DESTDIR)$(SYSTEMD_DIR)/proxmox-notify-reconcile@.service"
	install -m644 config/config.toml "$(DESTDIR)$(SYSCONFDIR)/proxmox-notify/config.toml"

package:
	scripts/build-deb

test:
	$(CARGO) test
	$(CARGO) build
	PROXMOX_NOTIFY_BIN="$(SMOKE_BINARY)" tests/smoke.sh

ci:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	bash -n scripts/build-deb
	bash -n tests/smoke.sh
	bash -n tests/installed-e2e.sh
	sh -n packaging/postinst
	sh -n packaging/postrm
	$(MAKE) test
