#!/usr/bin/env just --justfile

default: build

build *args="--release":
	python build.py {{args}}

build-all:
	python build.py --all

clean:
	cd src && cargo clean
