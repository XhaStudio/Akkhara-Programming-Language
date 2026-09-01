// Akkhara Random library (ကျပန်း)
//
// Usage (from an .akk program):
//     နည်းပညာများ ကျပန်း ကို အသုံးပြုပါ။
//
// This module provides random number generation backed by the `rand` crate.
// It is compiled into the akk binary as a Rust module and registered
// by the interpreter when a program imports the ကျပန်း library.
//
// Akkhara-level statements powered by this module:
//     ကျပန်းကိန်း x သည် 1 နှင့် 10 အကြား ဖြစ်၏။   # x = random_int(1, 10)

use rand::Rng;

/// Generates a random integer between min (inclusive) and max (exclusive).
pub fn random_int(min: i64, max: i64) -> Result<i64, String> {
    if min >= max {
        return Err(format!(
            "E064 ကျပန်းကိန်း အပိုင်းအခြားမှားနေပါသည်။ အစ ({}) သည် အဆုံး ({}) ထက် ငယ်ရပါမည်။",
            min, max
        ));
    }
    let mut rng = rand::thread_rng();
    Ok(rng.gen_range(min..max))
}

/// Generates a random float between min (inclusive) and max (exclusive).
pub fn random_float(min: f64, max: f64) -> Result<f64, String> {
    if min >= max {
        return Err(format!(
            "E065 ကျပန်းကိန်း အပိုင်းအခြားမှားနေပါသည်။ အစ ({}) သည် အဆုံး ({}) ထက် ငယ်ရပါမည်။",
            min, max
        ));
    }
    let mut rng = rand::thread_rng();
    Ok(rng.gen_range(min..max))
}
