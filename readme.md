## Overview

A cross-platform native webview extension for Godot 4, built on [WRY](https://github.com/tauri-apps/wry).

Vital.wry lets you render HTML, CSS, and JavaScript natively inside your game using the system's built-in webview — no bundled browser, no extra dependencies — with full two-way communication between JavaScript and GDScript.

It exists to push beyond the limits of upstream WRY, addressing gaps and edge cases that the base library leaves unresolved:

- **Input forwarding**: Proper mouse and keyboard event forwarding into the webview and back into Godot
- **Minimized window handling**: Webview creation is safely deferred when the window is minimized and resumes without interruption once restored
- **Z-index ordering**: Multiple webviews can be layered and reordered independently

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

Linux requires [WebKitGTK](https://webkitgtk.org). Transparency is not currently supported on Linux.
