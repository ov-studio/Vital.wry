# Linux transparent WebView regression

This is a standalone Godot project using only Vital.wry's existing public API.
It has no SDK, custom host, external assets or web server. The colored background
and moving white marker are rendered by Godot. HTML draws a clear area, opaque
red and 50% white over the scene.

Build with the repository's lockfile (GTK 3, WebKitGTK 4.1 and XComposite development
packages are needed on Linux):

```sh
(cd src && cargo build --locked)
cp .bin/debug/libvital_wry.so tests/linux-transparency/wry.so
mkdir -p tests/linux-transparency/.godot
printf 'res://wry.gdextension\n' > tests/linux-transparency/.godot/extension_list.cfg
GDK_BACKEND=x11 godot --path tests/linux-transparency --display-driver x11 \
  --rendering-method gl_compatibility
```

Repeat with `forward_plus`. For a manual run, select a test display explicitly.
Expected at the initial 960x640 size: clear HTML at (200,320) reveals orange
RGB (229,172,32), opaque red at (450,250) is (255,32,32), and half-white at
(650,250) is approximately (242,214,144). The bottom white marker keeps moving.

The automated checker creates a private headless Weston **GL compositor** with
Xwayland, and captures its real compositor output using `weston-screenshooter`.
It removes inherited display sockets and never opens the fixture on the current
desktop. Requires Weston 15 (`--fake-seat`, kiosk shell and screenshooter),
Xwayland, ImageMagick, Python 3 and Godot on PATH:

```sh
python3 tests/linux-transparency/check.py --library .bin/debug/libvital_wry.so \
  --renderer gl_compatibility --output /tmp/vital-transparency-gl
python3 tests/linux-transparency/check.py --library .bin/debug/libvital_wry.so \
  --renderer forward_plus --output /tmp/vital-transparency-vulkan
```

Output paths must be new. Receipts record library, source and checker hashes;
logs, screenshots and FAIL results remain available for review. The checker
requires three consecutive correct compositor frames per stage, after waiting
up to 15 seconds for actual paint. It exercises resize/move, direct hide/show,
ancestor hide/show, dynamic HTML paint, load_html, ten destroy/recreate cycles, full-window bounds,
removal and normal exit. Motion samples and scene frame counters verify continued
Godot rendering. CI runs unit tests/build only; this compositor test needs a
suitable graphics host and is run separately.

IPC keyboard re-entry from issue #1 is intentionally not exercised or fixed by
this transparency regression. A green result here does not certify that input
bug, native Wayland, every GPU, Windows or macOS.
