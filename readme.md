## Overview

A cross-platform native webview extension built on WRY, designed to extend and complement the Vital.sandbox ecosystem.

Vital.wry exposes the system's native webview inside Godot 4, enabling HTML, CSS, and JavaScript UIs with full two-way GDScript integration. It goes beyond upstream WRY to fix gaps the base library leaves unresolved — proper input forwarding, safe deferred creation on minimized windows, and independent z-index ordering across multiple webviews.

## Getting Started

- **Releases**: Grab the latest stable build from [Releases](https://github.com/ov-studio/Vital.wry/releases).
- **Documentation**: Learn the APIs, scripting patterns, and engine integration in the [Guides](https://vital-sandbox.com/docs).
- **Community**: Got questions or want to connect? Join us on [Discord](https://discord.vital-sandbox.com).

## Building from Source

Requires Python 3 and a Rust toolchain with the appropriate target for your platform.

```sh
# Windows
build.bat --[debug/release/all]

# macOS / Linux
./build.sh --[debug/release/all]
```

`--all` builds both debug and release in sequence. Compiled binaries are staged into `.build/` under the platform subdirectory.

## Platform Support

| Platform | Support | Web Engine |
| --- | --- | --- |
| Windows 10 / 11 | Supported | WebView2 (Chromium) |
| macOS (Intel, Apple Silicon) | Supported | WebKit |
| Linux (X11) | Supported | WebKitGTK |
| Android | Planned | — |
| iOS | Planned | — |
