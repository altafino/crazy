# AI App Studio. Run `make` for the list of commands.

SHELL := bash
.SHELLFLAGS := -eu -o pipefail -c
.ONESHELL:
.SILENT:
.DEFAULT_GOAL := help
MAKEFLAGS += --no-print-directory

NAME    := AI App Studio
VERSION := $(shell sed -n 's/^  "version": *"\([^"]*\)".*/\1/p' src-tauri/tauri.conf.json)
BUNDLE  := src-tauri/target/release/bundle/appimage
APPS    ?= $(HOME)/Applications
# Used only without AppImageLauncher: our own menu entry and icon.
DESKTOP := $(HOME)/.local/share/applications/ai-app-studio-appimage.desktop
ICON    := $(HOME)/.local/share/icons/hicolor/128x128/apps/ai-app-studio-appimage.png
# AppImageLauncher's CLI, if installed. AIL= forces the plain install.
AIL     ?= $(shell command -v ail-cli)

# Shell helpers shared by every recipe: colours (off without a terminal or with NO_COLOR) and
# built / installed / running lookups. File names contain spaces, so everything stays quoted.
define UI
if [ -t 1 ] && [ -z "$${NO_COLOR:-}" ]; then B=$$'\e[1m' D=$$'\e[2m' G=$$'\e[32m' Y=$$'\e[33m' R=$$'\e[31m' C=$$'\e[36m' N=$$'\e[0m'; else B= D= G= Y= R= C= N=; fi
step() { printf '\n%s%s▸ %s%s\n' "$$C" "$$B" "$$*" "$$N"; }
ok()   { printf '%s✔%s %s\n' "$$G" "$$N" "$$*"; }
warn() { printf '%s!%s %s\n' "$$Y" "$$N" "$$*" >&2; }
die()  { printf '%s✘ %s%s\n' "$$R" "$$*" "$$N" >&2; exit 1; }
size() { du -h "$$1" | cut -f1; }
built()     { ls -t "$(BUNDLE)/$(NAME)_$(VERSION)_"*.AppImage 2>/dev/null | head -1 || true; }
installed() { ls -t "$(APPS)/$(NAME)_"*.AppImage 2>/dev/null | head -1 || true; }
# Anchored at the start so it never matches this recipe's own shell, whose command line holds the path.
running()   { pgrep -f "^$(APPS)/$(NAME)_[^ ]*\.AppImage" | paste -sd ' ' || true; }
endef

##@ Everyday

.PHONY: help
help: ## Show this list
	$(UI)
	printf '\n%s%s %s%s  %smake <command>%s\n' "$$B" "$(NAME)" "$(VERSION)" "$$N" "$$D" "$$N"
	awk -v b="$$B" -v c="$$C" -v d="$$D" -v n="$$N" '
	  /^##@/ { printf "\n%s%s%s\n", b, substr($$0, 5), n; next }
	  /^[a-z-]+:.*## / { split($$0, a, ":.*## "); printf "  %s%-14s%s %s\n", c, a[1], n, a[2] }
	' $(MAKEFILE_LIST)
	printf '\n%sOptions: APPS=<folder> (default ~/Applications) · AIL= installs without AppImageLauncher · NO_STRIP=true if the AppImage step fails on strip · NO_COLOR=1%s\n\n' "$$D" "$$N"

.PHONY: install
install: appimage ## Build the AppImage and install it (replaces older versions)
	$(MAKE) install-last

.PHONY: run
run: ## Open the installed Studio
	$(UI)
	app=$$(installed); [ -n "$$app" ] || die "Studio isn't installed yet. Run: make install"
	setsid -f "$$app" >/dev/null 2>&1
	ok "Opening $${app##*/}"

.PHONY: status
status: ## Show what is built, installed and running
	$(UI)
	printf '\n%sSource%s     %s\n' "$$B" "$$N" "$(VERSION) · $$(git rev-parse --short HEAD 2>/dev/null)$$(git diff --quiet 2>/dev/null || echo ' + uncommitted changes')"
	b=$$(built)
	if [ -z "$$b" ]; then printf '%sBuilt%s      %snothing for %s yet (make appimage)%s\n' "$$B" "$$N" "$$D" "$(VERSION)" "$$N"
	else
	  printf '%sBuilt%s      %s %s(%s, %s)%s\n' "$$B" "$$N" "$$b" "$$D" "$$(size "$$b")" "$$(date -r "$$b" '+%d %b %H:%M')" "$$N"
	  newer=$$(find src src-tauri/src src-tauri/tauri.conf.json -newer "$$b" -type f -print -quit)
	  [ -z "$$newer" ] || warn "Sources changed since this build (e.g. $$newer). make install rebuilds."
	fi
	i=$$(installed)
	if [ -z "$$i" ]; then printf '%sInstalled%s  %snot installed (make install)%s\n' "$$B" "$$N" "$$D" "$$N"
	else printf '%sInstalled%s  %s %s(%s)%s\n' "$$B" "$$N" "$$i" "$$D" "$$(date -r "$$i" '+%d %b %H:%M')" "$$N"; fi
	p=$$(running)
	printf '%sRunning%s    %s\n\n' "$$B" "$$N" "$$([ -n "$$p" ] && echo "yes, PID $$p" || echo no)"

##@ Build

.PHONY: appimage
appimage: node_modules/.modules.yaml ## Build the AppImage (takes a few minutes)
	$(UI)
	step "Building $(NAME) $(VERSION) AppImage"
	start=$$SECONDS
	# appimagetool downloads the AppImage runtime from GitHub on every build and fails when GitHub
	# does (it returned 504s on 2026-11-08). Keep one copy and hand it over instead.
	rt="$${XDG_CACHE_HOME:-$$HOME/.cache}/tauri/runtime-$$(uname -m)"
	if [ ! -s "$$rt" ]; then
		mkdir -p "$$(dirname "$$rt")"
		curl -fsSL --max-time 60 -o "$$rt.part" "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-$$(uname -m)" && mv "$$rt.part" "$$rt" || rm -f "$$rt.part"
	fi
	if [ -s "$$rt" ]; then export LDAI_RUNTIME_FILE="$$rt"; fi
	pnpm tauri build --bundles appimage || die "Build failed (see above). 'Failed to download runtime': GitHub is down, retry later. Stopped at 'strip': NO_STRIP=true make appimage"
	b=$$(built); [ -n "$$b" ] || die "The build finished but no AppImage is in $(BUNDLE)"
	ok "Built in $$(( (SECONDS - start) / 60 ))m $$(( (SECONDS - start) % 60 ))s: $$b ($$(size "$$b"))"

.PHONY: install-last
install-last: ## Install the last build without rebuilding
	$(UI)
	src=$$(built); [ -n "$$src" ] || die "No AppImage for $(VERSION) in $(BUNDLE). Run: make install"
	step "Installing into $(APPS)"
	mkdir -p "$(APPS)"
	shopt -s nullglob
	for old in "$(APPS)/$(NAME)_"*.AppImage; do
	  if [ -n "$(AIL)" ]; then "$(AIL)" unintegrate "$$old" >/dev/null 2>&1 || true; fi
	  rm -f "$$old"; ok "Removed old $${old##*/}"
	done
	# Off again: with nullglob an unmatched glob makes installed()'s `ls -t` list the cwd ("Installed dist").
	shopt -u nullglob
	if [ -n "$(AIL)" ]; then
	  # AppImageLauncher moves the file into ~/Applications (adding a hash to its name) and
	  # creates the menu entry and icons. Hand it a copy so the build output stays put.
	  tmp=$$(mktemp -d); trap 'rm -rf "$$tmp"' EXIT
	  cp "$$src" "$$tmp/"
	  "$(AIL)" integrate "$$tmp/$${src##*/}" >/dev/null || die "AppImageLauncher couldn't integrate the AppImage"
	else
	  install -m 755 "$$src" "$(APPS)/"
	  mkdir -p "$$(dirname "$(DESKTOP)")" "$$(dirname "$(ICON)")"
	  cp src-tauri/icons/128x128.png "$(ICON)"
	  printf '[Desktop Entry]\nType=Application\nName=$(NAME)\nComment=Create, build and install your own apps with AI coding agents\nExec="%s"\nIcon=ai-app-studio-appimage\nCategories=Development;\nTerminal=false\n' "$(APPS)/$${src##*/}" > "$(DESKTOP)"
	  update-desktop-database -q "$$(dirname "$(DESKTOP)")" 2>/dev/null || true
	fi
	app=$$(installed); [ -n "$$app" ] || die "The AppImage didn't arrive in $(APPS)"
	ok "Installed $${app##*/}"
	ok "Find it in Activities as \"$(NAME)\", or run: make run"
	p=$$(running)
	[ -z "$$p" ] || warn "An older Studio is still running (PID $$p). Quit it, then run: make run"

.PHONY: uninstall
uninstall: ## Remove the installed AppImage and its menu entry (your projects and data stay)
	$(UI)
	shopt -s nullglob
	n=0
	for app in "$(APPS)/$(NAME)_"*.AppImage; do
	  if [ -n "$(AIL)" ]; then "$(AIL)" unintegrate "$$app" >/dev/null 2>&1 || true; fi
	  rm -f "$$app"; ok "Removed $${app##*/}"; n=$$((n + 1))
	done
	rm -f "$(DESKTOP)" "$(ICON)"
	[ "$$n" -gt 0 ] || warn "Nothing to remove in $(APPS)"
	printf '%sYour projects and Studio data (%s) were kept.%s\n' "$$D" "$${XDG_DATA_HOME:-$$HOME/.local/share}/ai-app-studio" "$$N"

##@ Develop

.PHONY: dev
dev: node_modules/.modules.yaml ## Run Studio with hot reload
	pnpm tauri dev

.PHONY: check
check: node_modules/.modules.yaml ## Run every check: Rust format, lint and tests, then frontend types and tests
	$(UI)
	start=$$SECONDS
	step "Rust format";  (cd src-tauri && cargo fmt --check)
	step "Rust lint";    (cd src-tauri && cargo clippy --all-targets -q -- -D warnings)
	step "Rust tests";   (cd src-tauri && cargo test -q)
	step "Type check";   pnpm exec vue-tsc --noEmit
	step "Vue tests";    pnpm exec vitest run
	printf '\n'; ok "All checks passed in $$(( (SECONDS - start) / 60 ))m $$(( (SECONDS - start) % 60 ))s"

.PHONY: fmt
fmt: ## Format the Rust code
	cd src-tauri && cargo fmt

.PHONY: bindings
bindings: ## Regenerate src/bindings.ts after changing a Rust command
	cd src-tauri && UPDATE_BINDINGS=1 cargo test -q bindings_are_fresh

# Install JavaScript dependencies only when the lockfile or package.json changed.
node_modules/.modules.yaml: pnpm-lock.yaml package.json
	$(UI)
	step "Installing JavaScript dependencies"
	pnpm install --frozen-lockfile
	touch "$@"
