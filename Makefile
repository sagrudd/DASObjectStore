SHELL := /usr/bin/env bash

.DEFAULT_GOAL := help

GITHUB_OWNER ?= sagrudd
MNEMOSYNE_WORKSPACE ?= $(abspath ..)
MNEMOSYNE_REPO_MATCH ?= mnemosyne|mneion|monas|synoptikon|mnematikon|grammateus|flounder

.PHONY: help pull build web test fmt check deb rpm remote-deb remote-rpm package clean distclean

help:
	@printf 'DASObjectStore build targets:\n'
	@printf '  make pull       Pull this repo and clone/pull sibling Mnemosyne product repos\n'
	@printf '  make build      Build release CLI, server, and daemon binaries\n'
	@printf '  make web        Build or prepare the packaged web interface assets\n'
	@printf '  make test       Run the full Rust workspace test suite\n'
	@printf '  make fmt        Format Rust sources\n'
	@printf '  make check      Run cargo check for the workspace\n'
	@printf '  make deb        Build a Debian package under target/deb/\n'
	@printf '  make rpm        Build an RPM package under target/rpm/rpmbuild/RPMS/\n'
	@printf '  make remote-deb Build a Debian package for dasobjectstore-remote only\n'
	@printf '  make remote-rpm Build an RPM package for dasobjectstore-remote only\n'
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

test:
	cargo test --workspace

fmt:
	cargo fmt --all

check:
	cargo check --workspace

deb: web
	bash packaging/debian/build-deb.sh

rpm: web
	bash packaging/rpm/build-rpm.sh

remote-deb:
	bash packaging/debian/build-remote-deb.sh

remote-rpm:
	bash packaging/rpm/build-remote-rpm.sh

package: deb rpm

clean:
	cargo clean

distclean: clean
	rm -rf target/deb target/rpm
