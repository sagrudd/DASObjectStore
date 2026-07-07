SHELL := /usr/bin/env bash

.DEFAULT_GOAL := help

.PHONY: help build test fmt check deb rpm package clean distclean

help:
	@printf 'DASObjectStore build targets:\n'
	@printf '  make build      Build release CLI, server, and daemon binaries\n'
	@printf '  make test       Run the full Rust workspace test suite\n'
	@printf '  make fmt        Format Rust sources\n'
	@printf '  make check      Run cargo check for the workspace\n'
	@printf '  make deb        Build a Debian package under target/deb/\n'
	@printf '  make rpm        Build an RPM package under target/rpm/rpmbuild/RPMS/\n'
	@printf '  make package    Build both DEB and RPM packages\n'
	@printf '  make clean      Remove Cargo build artifacts\n'
	@printf '  make distclean  Remove Cargo and package build artifacts\n'

build:
	cargo build --release --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all

check:
	cargo check --workspace

deb:
	bash packaging/debian/build-deb.sh

rpm:
	bash packaging/rpm/build-rpm.sh

package: deb rpm

clean:
	cargo clean

distclean: clean
	rm -rf target/deb target/rpm
