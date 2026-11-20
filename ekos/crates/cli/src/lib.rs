// Terminal output on Windows is plain ASCII (`ascii`). These shadow the standard printing
// macros for the whole crate: a `macro_rules!` defined before the modules is in their textual
// scope, which takes precedence over the prelude. Each one formats as the standard macro does,
// then prints `ascii::console` of the text, so test output capture still works.
macro_rules! println {
    () => { ::std::println!() };
    ($($arg:tt)*) => { ::std::println!("{}", $crate::ascii::console(&::std::format!($($arg)*))) };
}
macro_rules! print {
    ($($arg:tt)*) => { ::std::print!("{}", $crate::ascii::console(&::std::format!($($arg)*))) };
}
macro_rules! eprintln {
    () => { ::std::eprintln!() };
    ($($arg:tt)*) => { ::std::eprintln!("{}", $crate::ascii::console(&::std::format!($($arg)*))) };
}
#[allow(unused_macros)] // kept so a future `eprint!` is ASCII too
macro_rules! eprint {
    ($($arg:tt)*) => { ::std::eprint!("{}", $crate::ascii::console(&::std::format!($($arg)*))) };
}

pub mod app;
pub mod ascii;
pub mod commands;
pub mod coverage;
pub mod detect;
pub mod extension;
pub mod freshness;
pub mod home;
pub mod run_log;
