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
| Windows | `build --[debug/release/all]` |
| macOS / Linux | `bash build.sh --[debug/release/all]` |

## Platform Support

| Platform | Support | Web Engine |
| --- | --- | --- |
| Windows 10 / 11 | Supported | WebView2 (Chromium) |
| macOS (Intel, Apple Silicon) | Supported | WebKit |
| Linux (X11) | Supported | WebKitGTK |
| Android | Planned | — |
| iOS | Planned | — |

### Transparent WebViews on Linux

With `transparent = true`, X11/Xwayland uses XComposite to redirect the native
child's drawing while retaining its input window. The transparent path uses
WRY's existing `build_gtk()` API with an owned GTK child to keep hide/show and
teardown under one owner. WebKitGTK snapshots supply
premultiplied RGBA pixels to a Godot texture, so both OpenGL and Vulkan can
compose transparent HTML over the continually rendered scene. No application
bootstrap, helper binary or additional Godot API is needed. Install XComposite
(e.g. `libxcomposite-dev` on Debian/Ubuntu) along with GTK 3 and WebKitGTK 4.1 to build.

This path uses WebKit's software painter and asynchronous CPU image copies;
video/WebGL and high-resolution animated pages may have different performance
or availability from an accelerated native webview. Visible pages (including static pages) are copied at up to 30 Hz. One snapshot is
pending at a time, bounded to 64 MiB; load, size and visibility changes cancel stale work.
Opaque Linux WebViews and non-Linux platforms keep their existing native path.
The snapshot layer is above the ordinary scene (CanvasLayer `1 + window_z_index`).
Mixed opaque/transparent native windows, overlapping views with different input
stacking, transformed controls and native Window destruction/reparenting need
separate qualification. Native Wayland is not supported by this X11 path.

See [the independent regression project](tests/linux-transparency/README.md).
