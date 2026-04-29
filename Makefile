PREFIX ?= /usr/local
SYSCONFDIR ?= /etc
DESTDIR ?=
CARGO ?= cargo
BUILD_PROFILE ?= release
INSTALL_BINARY ?= 1
INSTALL_CONFIG ?= 1
INSTALL_SYSTEMD_UNIT ?= 1
VERSION ?= 0.1.0
PACKAGE_ARCH ?= $(shell uname -m)
PACKAGE_OS ?= $(shell uname -s | tr '[:upper:]' '[:lower:]')
PACKAGE_PATH ?= build/proxmox-notify_$(VERSION)_$(PACKAGE_ARCH)-$(PACKAGE_OS)

BINDIR := $(PREFIX)/bin
SYSTEMD_DIR := $(PREFIX)/lib/systemd/system
PROFILE_DIR := $(if $(filter release,$(BUILD_PROFILE)),release,debug)
CARGO_PROFILE_FLAG := $(if $(filter release,$(BUILD_PROFILE)),--release,)
TARGET_DIR := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target)
BINARY := $(TARGET_DIR)/$(PROFILE_DIR)/proxmox-notify
RELEASE_BINARY := $(TARGET_DIR)/release/proxmox-notify
DEBUG_BINARY := $(TARGET_DIR)/debug/proxmox-notify
SMOKE_BINARY := $(if $(filter /%,$(DEBUG_BINARY)),$(DEBUG_BINARY),$(CURDIR)/$(DEBUG_BINARY))
INSTALL_ARGS := --prefix "$(PREFIX)" --sysconfdir "$(SYSCONFDIR)"
INSTALL_ARGS += $(if $(DESTDIR),--destdir "$(DESTDIR)",)
INSTALL_ARGS += $(if $(filter 0 false no,$(INSTALL_BINARY)),--no-binary,)
INSTALL_ARGS += $(if $(filter 0 false no,$(INSTALL_CONFIG)),--no-config,)
INSTALL_ARGS += $(if $(filter 0 false no,$(INSTALL_SYSTEMD_UNIT)),--no-systemd-unit,)

.PHONY: build install package deb test ci

build:
	$(CARGO) build $(CARGO_PROFILE_FLAG)

install: build
	"$(BINARY)" install $(INSTALL_ARGS)

package:
	$(CARGO) build --release
	install -d "$(dir $(PACKAGE_PATH))"
	install -m755 "$(RELEASE_BINARY)" "$(PACKAGE_PATH)"
	printf '%s\n' "$(PACKAGE_PATH)"

deb:
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
	sh -n packaging/prerm
	sh -n packaging/postrm
	$(MAKE) test
