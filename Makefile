PREFIX ?= $(HOME)/.local

.PHONY: build test install uninstall

build:
	cargo build --release

test:
	cargo test

# Installs for the current user: binary, launcher entry and icon.
install: build
	install -Dm755 target/release/omavec $(PREFIX)/bin/omavec
	install -Dm644 assets/omavec.desktop $(PREFIX)/share/applications/omavec.desktop
	install -Dm644 assets/omavec.svg $(PREFIX)/share/icons/hicolor/scalable/apps/omavec.svg
	-update-desktop-database $(PREFIX)/share/applications 2>/dev/null

uninstall:
	rm -f $(PREFIX)/bin/omavec $(PREFIX)/share/applications/omavec.desktop \
		$(PREFIX)/share/icons/hicolor/scalable/apps/omavec.svg
