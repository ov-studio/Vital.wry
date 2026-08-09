#!/usr/bin/env just --justfile

os := if os() == "macos" { "macos" } else if os() == "windows" { "windows" } else { "linux" }
target := if os == "macos" { arch() + "-apple-darwin" } else if os == "windows" { arch() + "-pc-windows-msvc" } else { arch() + "-unknown-linux-gnu" }

default: build

set working-directory := 'rust'

build profile="release":
	@echo "Building for {{os}} ({{target}}, {{profile}})..."
	@just _build-{{os}} {{profile}}
	@just _copy-to-build-{{os}} {{profile}}

copy-to-build profile="release": (build profile)
	@echo "Copying files to .build..."
	@just _copy-to-build-{{os}} {{profile}}

clean:
	cargo clean

_build-macos profile:
	cargo build --target {{target}} {{ if profile == "release" { "--release" } else { "" } }}

_build-linux profile:
	cargo build --target {{target}} {{ if profile == "release" { "--release" } else { "" } }}

_build-windows profile:
	cargo build --target {{target}} {{ if profile == "release" { "--release" } else { "" } }}

_copy-to-build-macos profile:
	mkdir -p ../.build/macos
	cp ../.bin/{{target}}/{{profile}}/libgodot_wry.dylib ../.build/macos/vital.wry.{{profile}}.dylib

_copy-to-build-linux profile:
	mkdir -p ../.build/linux
	cp ../.bin/{{target}}/{{profile}}/libgodot_wry.so ../.build/linux/vital.wry.{{profile}}.x86_64.so

_copy-to-build-windows profile:
	mkdir -p ../.build/windows
	cp ../.bin/{{target}}/{{profile}}/godot_wry.dll ../.build/windows/vital.wry.{{profile}}.x86_64.dll

build-all: build-macos-universal build-linux build-windows

build-macos-universal profile="release":
	@echo "Building universal macOS binary ({{profile}})..."
	cargo build --target aarch64-apple-darwin {{ if profile == "release" { "--release" } else { "" } }}
	cargo build --target x86_64-apple-darwin {{ if profile == "release" { "--release" } else { "" } }}
	mkdir -p ../.bin/{{profile}}
	lipo -create -output ../.bin/{{profile}}/libgodot_wry.dylib ../.bin/aarch64-apple-darwin/{{profile}}/libgodot_wry.dylib ../.bin/x86_64-apple-darwin/{{profile}}/libgodot_wry.dylib
	mkdir -p ../.build/macos
	cp ../.bin/{{profile}}/libgodot_wry.dylib ../.build/macos/vital.wry.{{profile}}.dylib

build-linux profile="release":
	@echo "Building for Linux ({{profile}})..."
	just os="linux" build {{profile}}

build-windows profile="release":
	@echo "Building for Windows ({{profile}})..."
	just os="windows" build {{profile}}
