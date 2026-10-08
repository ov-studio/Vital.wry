//! Logging macros. All messages are prefixed `[Vital.wry]` (or a sub-tag) so the
//! sandbox console can recognise and route them.

// Debug-only print with a custom tag; compiled out of release builds.
macro_rules! tagged_debug {
    ($tag:literal, $fmt:literal $(, $arg:expr)* $(,)?) => {
        if cfg!(debug_assertions) {
            godot::prelude::godot_print!("[{}] {}", $tag, format_args!($fmt $(, $arg)*))
        }
    };
}

// Debug-only log, tagged `Vital.wry`.
macro_rules! wry_debug {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        tagged_debug!("Vital.wry", $fmt $(, $arg)*)
    };
}

// Warning, emitted in all builds.
macro_rules! wry_warn {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        godot::prelude::godot_warn!("[Vital.wry] {}", format_args!($fmt $(, $arg)*))
    };
}

// Error, emitted in all builds.
macro_rules! wry_error {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        godot::prelude::godot_error!("[Vital.wry] {}", format_args!($fmt $(, $arg)*))
    };
}

// Debug-only log for the `res://` protocol handler, tagged `Vital.wry.protocol`.
macro_rules! wry_protocol_debug {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        tagged_debug!("Vital.wry.protocol", $fmt $(, $arg)*)
    };
}
