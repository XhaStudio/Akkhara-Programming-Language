// Akkhara Time library (အချိန်)
//
// Usage (from an .akk program):
//     နည်းပညာများ အချိန် ကို အသုံးပြုပါ။
//
// This module provides the "wait" (စောင့်) capability backed by the
// standard library thread-sleep. It is compiled into the akk binary as a
// Rust module (see the `#[path]` include in src/main.rs) and registered
// by the interpreter when a program imports the အချိန် library.
//
// Akkhara-level statements powered by this module:
//     10s ကို စောင့်ပါ။   # wait 10 seconds
//     10m ကို စောင့်ပါ။   # wait 10 minutes
//     10h ကို စောင့်ပါ။   # wait 10 hours
//     1000 ကို စောင့်ပါ။  # wait 1000 seconds (no unit defaults to seconds)

use std::time::Duration;

/// The unit suffix attached to a wait amount: `s`, `m`, or `h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitUnit {
    Seconds,
    Minutes,
    Hours,
}

impl WaitUnit {
    /// How many seconds one of this unit equals.
    pub fn to_seconds(self) -> f64 {
        match self {
            WaitUnit::Seconds => 1.0,
            WaitUnit::Minutes => 60.0,
            WaitUnit::Hours => 3600.0,
        }
    }
}

/// Sleep for `amount` of the given `unit`.
pub fn wait(amount: f64, unit: WaitUnit) -> Result<(), String> {
    wait_seconds(amount * unit.to_seconds())
}

/// Sleep for an exact number of seconds.
pub fn wait_seconds(secs: f64) -> Result<(), String> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(format!(
            "E061 စောင့်ရန် အချိန်တန်ဖိုး ({}) သည် မှန်ကန်သော ကိန်းဂဏန်း မဟုတ်ပါ။",
            secs
        ));
    }
    std::thread::sleep(Duration::from_secs_f64(secs));
    Ok(())
}
