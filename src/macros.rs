macro_rules! tagged_debug {
    ($tag:literal, $fmt:literal $(, $arg:expr)* $(,)?) => {
        if cfg!(debug_assertions) {
            godot::prelude::godot_print!("[{}] {}", $tag, format_args!($fmt $(, $arg)*))
        }
    };
}

macro_rules! wry_debug {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        tagged_debug!("Vital.wry", $fmt $(, $arg)*)
    };
}

macro_rules! wry_warn {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        godot::prelude::godot_warn!("[Vital.wry] {}", format_args!($fmt $(, $arg)*))
    };
}

macro_rules! wry_error {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        godot::prelude::godot_error!("[Vital.wry] {}", format_args!($fmt $(, $arg)*))
    };
}

macro_rules! wry_protocol_debug {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        tagged_debug!("Vital.wry.protocol", $fmt $(, $arg)*)
    };
}