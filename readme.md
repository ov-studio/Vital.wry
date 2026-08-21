## Overview

A native webview extension for Godot 4, built on WRY to extend and complement the Vital.sandbox ecosystem.

Render HTML, CSS, and JavaScript natively inside Godot 4 — using the system's built-in webview with no extra dependencies.

## Getting Started

- **Releases**: Grab the latest stable build from [Releases](https://github.com/ov-studio/Vital.wry/releases).
- **Documentation**: Learn the APIs, scripting patterns, and engine integration in the [Guides](https://vital-sandbox.com/docs).
- **Community**: Got questions or want to connect? Join us on [Discord](https://discord.vital-sandbox.com).

## Features

- **Native webview**: Exposes the system's built-in webview inside Godot 4 — no bundled browser, no extra dependencies
- **Two-way integration**: Full communication between JavaScript and C++
- **Input forwarding**: Mouse and keyboard events forward correctly into the webview and back into Godot
- **Deferred creation**: Webview construction is safely deferred on minimized windows and resumes without interruption once restored
- **Z-index ordering**: Multiple webviews can be layered and reordered independently
- **Stable profile path**: WebView data stored at `user://webview` instead of defaulting next to the executable

## Building

Requires Python 3 and a Rust toolchain with the appropriate target for your platform.

| Platform | Command |
| --- | --- |
| Windows | `build.bat --[debug/release/all]` |
| macOS / Linux | `./build.sh --[debug/release/all]` |

## Platform Support

| Platform | Support | Web Engine |
| --- | --- | --- |
| Windows 10 / 11 | Supported | WebView2 (Chromium) |
| macOS (Intel, Apple Silicon) | Supported | WebKit |
| Linux (X11) | Supported | WebKitGTK |
| Android | Planned | — |
| iOS | Planned | — |
