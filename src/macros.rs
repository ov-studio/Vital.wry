macro_rules! debug_print {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            godot::prelude::godot_print!($($arg)*);
        }
    };
}