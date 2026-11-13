`src/scripts/linux_snapshot.rs` adapts the CPU snapshot module from the Linux
reference implementation associated with issue #1. The original notice is kept:
Copyright 2026 DreamCity contributors; Apache-2.0 OR MIT. Copies of both licenses
are included here. Adaptation removes transport statistics and the custom WRY
error dependency, handles Cairo native-endian pixels, and replaces draw-only invalidation with
bounded polling to capture WebKit updates without GTK draw signals. The containing project
retains its GPL-3.0 license.

No WRY fork, GPU helper or external native source is bundled by this change.
