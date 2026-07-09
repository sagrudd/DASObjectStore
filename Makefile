SHELL := /usr/bin/env bash

.DEFAULT_GOAL := help

GITHUB_OWNER ?= sagrudd
MNEMOSYNE_WORKSPACE ?= $(abspath ..)
MNEMOSYNE_REPO_MATCH ?= mnemosyne|mneion|monas|synoptikon|mnematikon|gnostikon|grammateus|flounder|prosopikon
GRAMMATEUS_DIR ?= $(MNEMOSYNE_WORKSPACE)/grammateus
FLOUNDER_DIR ?= $(MNEMOSYNE_WORKSPACE)/floundeR
REPORT_PROVIDER_IMAGE ?= grammateus/report:0.8.1
GRAMMATEUS_REPORT_PROVIDER ?= grammateus_report_provider

.PHONY: help pull build web web-screenshots report-provider test fmt check deb rpm remote remote-deb remote-rpm package clean distclean

help:
	@printf 'DASObjectStore build targets:\n'
	@printf '  make pull       Pull this repo and clone/pull sibling Mnemosyne product repos\n'
	@printf '  make build      Build release CLI, server, and daemon binaries\n'
	@printf '  make web        Build or prepare the packaged web interface assets\n'
	@printf '  make web-screenshots Build the Web UI and run Playwright screenshot regressions\n'
	@printf '  make report-provider Initialise the Grammateus/floundeR formal PDF report container\n'
	@printf '  make test       Run the full Rust workspace test suite\n'
	@printf '  make fmt        Format Rust sources\n'
	@printf '  make check      Run cargo check for the workspace\n'
	@printf '  make deb        Build a Debian package under target/deb/\n'
	@printf '  make rpm        Build an RPM package under target/rpm/rpmbuild/RPMS/\n'
	@printf '  make remote     Build the dasobjectstore-remote client only; runtime needs AWS CLI and a browser or --no-browser\n'
	@printf '  make remote-deb Build a remote-only Debian package; package suggests awscli and easyconnect opens a browser when available\n'
	@printf '  make remote-rpm Build a remote-only RPM package; package recommends awscli and easyconnect opens a browser when available\n'
	@printf '  make package    Build both DEB and RPM packages\n'
	@printf '  make clean      Remove Cargo build artifacts\n'
	@printf '  make distclean  Remove Cargo and package build artifacts\n'

pull:
	@set -euo pipefail; \
	workspace="$(MNEMOSYNE_WORKSPACE)"; \
	owner="$(GITHUB_OWNER)"; \
	repo_match="$(MNEMOSYNE_REPO_MATCH)"; \
	mkdir -p "$$workspace"; \
	printf 'Pulling core repository: %s\n' "$$(basename "$$(pwd)")"; \
	git pull --ff-only --autostash; \
	if ! command -v gh >/dev/null 2>&1; then \
		printf 'gh CLI is required for Mnemosyne product discovery. Install gh and authenticate for %s.\n' "$$owner" >&2; \
		exit 1; \
	fi; \
	printf 'Discovering Mnemosyne product repositories for %s under %s\n' "$$owner" "$$workspace"; \
	repos="$$(gh repo list "$$owner" --limit 200 --json name --jq '.[] | .name | select((ascii_downcase | test("'"$$repo_match"'")))' )"; \
	if [ -z "$$repos" ]; then \
		printf 'No matching Mnemosyne product repositories were visible for %s.\n' "$$owner"; \
		exit 0; \
	fi; \
	while IFS= read -r repo; do \
		[ -n "$$repo" ] || continue; \
		target="$$(find "$$workspace" -maxdepth 1 -type d -iname "$$repo" -print -quit)"; \
		if [ -n "$$target" ] && [ -d "$$target/.git" ]; then \
			printf 'Pulling sibling repository: %s\n' "$$target"; \
			git -C "$$target" pull --ff-only --autostash; \
		elif [ -n "$$target" ]; then \
			printf 'Skipping %s because %s exists but is not a git repository.\n' "$$repo" "$$target" >&2; \
		else \
			target="$$workspace/$$repo"; \
			printf 'Cloning sibling repository: %s/%s -> %s\n' "$$owner" "$$repo" "$$target"; \
			gh repo clone "$$owner/$$repo" "$$target"; \
		fi; \
	done <<< "$$repos"

build:
	cargo build --release --workspace

web:
	bash packaging/web/prepare-web-dist.sh

web-screenshots:
	node tools/web-screenshot-regression.mjs

report-provider:
	@set -euo pipefail; \
	if command -v "$(GRAMMATEUS_REPORT_PROVIDER)" >/dev/null 2>&1; then \
		"$(GRAMMATEUS_REPORT_PROVIDER)" install \
			--image "$(REPORT_PROVIDER_IMAGE)" \
			--grammateus-root "$(GRAMMATEUS_DIR)" \
			--flounder-root "$(FLOUNDER_DIR)"; \
	elif [ -f "$(GRAMMATEUS_DIR)/Cargo.toml" ]; then \
		cargo run --manifest-path "$(GRAMMATEUS_DIR)/Cargo.toml" \
			--bin grammateus_report_provider -- install \
			--image "$(REPORT_PROVIDER_IMAGE)" \
			--grammateus-root "$(GRAMMATEUS_DIR)" \
			--flounder-root "$(FLOUNDER_DIR)"; \
	else \
		printf 'grammateus_report_provider is not installed and %s is unavailable. Run make pull or install Grammateus before building report-enabled packages.\n' "$(GRAMMATEUS_DIR)" >&2; \
		exit 1; \
	fi

test:
	cargo test --workspace

fmt:
	cargo fmt --all

check:
	cargo check --workspace

deb: web report-provider
	bash packaging/debian/build-deb.sh

rpm: web report-provider
	bash packaging/rpm/build-rpm.sh

remote:
	cargo build --release -p dasobjectstore-remote

remote-deb:
	bash packaging/debian/build-remote-deb.sh

remote-rpm:
	bash packaging/rpm/build-remote-rpm.sh

package: deb rpm

clean:
	cargo clean

distclean: clean
	rm -rf target/deb target/rpm
