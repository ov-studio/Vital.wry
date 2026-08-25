macro_rules! debug_print {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            godot::prelude::godot_print!($($arg)*);
        }
    };
}

macro_rules! wry_debug {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        debug_print!(concat!("[Vital.wry] ", $fmt) $(, $arg)*);
    };
}

macro_rules! wry_warn {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        godot::prelude::godot_warn!(concat!("[Vital.wry] ", $fmt) $(, $arg)*);
    };
}

macro_rules! wry_error {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        godot::prelude::godot_error!(concat!("[Vital.wry] ", $fmt) $(, $arg)*);
    };
}