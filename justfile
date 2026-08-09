#!/usr/bin/env just --justfile

os := if os() == "macos" { "macos" } else if os() == "windows" { "windows" } else { "linux" }
target := if os == "macos" { arch() + "-apple-darwin" } else if os == "windows" { arch() + "-pc-windows-msvc" } else { arch() + "-unknown-linux-gnu" }

default: build

set working-directory := 'rust'

build profile="release":
	@echo "Building for {{os}} ({{target}}, {{profile}})..."
	@just _build-{{os}} {{profile}}
	@just _copy-to-godot-{{os}} {{profile}}

copy-to-godot profile="release": (build profile)
	@echo "Copying files to Godot project..."
	@just _copy-to-godot-{{os}} {{profile}}

clean:
	cargo clean

_build-macos profile:
	cargo build --target {{target}} {{ if profile == "release" { "--release" } else { "" } }}

_build-linux profile:
	cargo build --target {{target}} {{ if profile == "release" { "--release" } else { "" } }}

_build-windows profile:
	cargo build --target {{target}} {{ if profile == "release" { "--release" } else { "" } }}

_copy-to-godot-macos profile:
	mkdir -p ../godot/addons/godot_wry/macos
	cp ./target/{{target}}/{{profile}}/libgodot_wry.dylib ../godot/addons/godot_wry/macos/vital.wry.{{profile}}.dylib

_copy-to-godot-linux profile:
	mkdir -p ../godot/addons/godot_wry/linux
	cp ./target/{{target}}/{{profile}}/libgodot_wry.so ../godot/addons/godot_wry/linux/vital.wry.{{profile}}.x86_64.so

_copy-to-godot-windows profile:
	mkdir -p ../godot/addons/godot_wry/windows
	cp ./target/{{target}}/{{profile}}/godot_wry.dll ../godot/addons/godot_wry/windows/vital.wry.{{profile}}.x86_64.dll

build-all: build-macos-universal build-linux build-windows

build-macos-universal profile="release":
	@echo "Building universal macOS binary ({{profile}})..."
	cargo build --target aarch64-apple-darwin {{ if profile == "release" { "--release" } else { "" } }}
	cargo build --target x86_64-apple-darwin {{ if profile == "release" { "--release" } else { "" } }}
	mkdir -p ./target/{{profile}}
	lipo -create -output ./target/{{profile}}/libgodot_wry.dylib ./target/aarch64-apple-darwin/{{profile}}/libgodot_wry.dylib ./target/x86_64-apple-darwin/{{profile}}/libgodot_wry.dylib
	mkdir -p ../godot/addons/godot_wry/macos
	cp ./target/{{profile}}/libgodot_wry.dylib ../godot/addons/godot_wry/macos/vital.wry.{{profile}}.dylib

build-linux profile="release":
	@echo "Building for Linux ({{profile}})..."
	just os="linux" build {{profile}}

build-windows profile="release":
	@echo "Building for Windows ({{profile}})..."
	just os="windows" build {{profile}}
