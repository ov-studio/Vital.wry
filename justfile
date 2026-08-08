#!/usr/bin/env just --justfile

os := if os() == "macos" { "macos" } else if os() == "windows" { "windows" } else { "linux" }
target := if os == "macos" { arch() + "-apple-darwin" } else if os == "windows" { arch() + "-pc-windows-msvc" } else { arch() + "-unknown-linux-gnu" }

default: build

set working-directory := 'rust'

build: 
	@echo "Building for {{os}} ({{target}})..."
	@just _build-{{os}}
	@just _copy-to-godot-{{os}}

copy-to-godot: build
	@echo "Copying files to Godot project..."
	@just _copy-to-godot-{{os}}

clean:
	cargo clean

_build-macos:
	cargo build --target {{target}}  --release
	mkdir -p ./target/{{target}}/release/libgodot_wry.framework/Resources
	mv ./target/{{target}}/release/libgodot_wry.dylib ./target/{{target}}/release/libgodot_wry.framework/libgodot_wry.dylib
	cp ../assets/Info.plist ./target/{{target}}/release/libgodot_wry.framework/Resources/Info.plist

_build-linux:
	cargo build --target {{target}}  --release

_build-windows:
	cargo build --target {{target}}  --release

_copy-to-godot-macos:
	mkdir -p ../godot/addons/godot_wry/bin/{{target}}
	cp -R ./target/{{target}}/release/libgodot_wry.framework ../godot/addons/godot_wry/bin/{{target}}

_copy-to-godot-linux:
	mkdir -p ../godot/addons/godot_wry/bin/{{target}}
	cp ./target/{{target}}/release/libgodot_wry.so ../godot/addons/godot_wry/bin/{{target}}/

_copy-to-godot-windows:
	mkdir -p ../godot/addons/godot_wry/bin/{{target}}
	cp ./target/{{target}}/release/godot_wry.dll ../godot/addons/godot_wry/bin/{{target}}/

build-all: build-macos-universal build-linux build-windows

build-macos-universal:
	@echo "Building universal macOS binary..."
	cargo build --target aarch64-apple-darwin  --release
	cargo build --target x86_64-apple-darwin  --release
	mkdir -p ./target/release/libgodot_wry.framework/Resources
	lipo -create -output ./target/release/libgodot_wry.dylib ./target/aarch64-apple-darwin/release/libgodot_wry.dylib ./target/x86_64-apple-darwin/release/libgodot_wry.dylib
	mv ./target/release/libgodot_wry.dylib ./target/release/libgodot_wry.framework/libgodot_wry.dylib
	cp ../assets/Info.plist ./target/release/libgodot_wry.framework/Resources/Info.plist
	mkdir -p ../godot/addons/godot_wry/bin/universal-apple-darwin
	cp -R ./target/release/libgodot_wry.framework ../godot/addons/godot_wry/bin/universal-apple-darwin

build-linux:
	@echo "Building for Linux..."
	just os="linux" build

build-windows:
	@echo "Building for Windows..."
	just os="windows" build
