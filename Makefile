# Build and install Aria Shell.
#
#   make                                 cargo build --release
#   make run                             build in debug and run it from the checkout
#   sudo make install                    in /usr/local, PAM service included
#   make install PREFIX=$HOME/.local     for this user only, no root (no PAM service)
#   make DESTDIR=$pkgdir PREFIX=/usr install    for a package
#
# `install` doesn't build: `make` first, as yourself, not as root.

PREFIX     ?= /usr/local
SYSCONFDIR ?= /etc
BINDIR     ?= $(PREFIX)/bin
DATADIR    ?= $(PREFIX)/share
DOCDIR     ?= $(DATADIR)/doc/aria-shell
UNITDIR    ?= $(DATADIR)/systemd/user
PAMDIR     ?= $(SYSCONFDIR)/pam.d
CARGO      ?= cargo

BIN = target/release/aria-shell
THEMES = $(notdir $(wildcard assets/themes/*.css))

.PHONY: all build run install uninstall

all: build

build:
	$(CARGO) build --release

run:
	$(CARGO) run

install:
	@test -x $(BIN) || { echo "$(BIN) is missing: run make first"; exit 1; }
	install -Dm755 $(BIN) $(DESTDIR)$(BINDIR)/aria-shell
	install -Dm644 -t $(DESTDIR)$(DATADIR)/aria-shell/themes assets/themes/*.css
	install -dm755 $(DESTDIR)$(UNITDIR)
	sed 's|@bindir@|$(BINDIR)|g' assets/systemd/aria-shell.service.in \
		> $(DESTDIR)$(UNITDIR)/aria-shell.service
	chmod 644 $(DESTDIR)$(UNITDIR)/aria-shell.service
	install -Dm644 -t $(DESTDIR)$(DOCDIR) README.md assets/aria.conf
	install -Dm644 -t $(DESTDIR)$(DOCDIR)/compositors assets/compositors/*
	@if [ -n "$(DESTDIR)" ] || [ -w $(PAMDIR) ]; then \
		echo "install -Dm644 assets/pam.d/aria-shell $(DESTDIR)$(PAMDIR)/aria-shell"; \
		install -Dm644 assets/pam.d/aria-shell $(DESTDIR)$(PAMDIR)/aria-shell; \
	else \
		echo "note: $(PAMDIR) not writable, the lock screen will use the 'login' PAM service"; \
	fi

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/aria-shell
	rm -f $(addprefix $(DESTDIR)$(DATADIR)/aria-shell/themes/,$(THEMES))
	-rmdir --ignore-fail-on-non-empty $(DESTDIR)$(DATADIR)/aria-shell/themes $(DESTDIR)$(DATADIR)/aria-shell
	rm -f $(DESTDIR)$(UNITDIR)/aria-shell.service
	rm -rf $(DESTDIR)$(DOCDIR)
	@if [ -n "$(DESTDIR)" ] || [ -w $(PAMDIR) ]; then \
		rm -f $(DESTDIR)$(PAMDIR)/aria-shell; \
	fi
