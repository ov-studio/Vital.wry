#!/usr/bin/env just --justfile

default: build

build *args:
	python build.py {{args}}

clean:
	cd src && cargo clean
