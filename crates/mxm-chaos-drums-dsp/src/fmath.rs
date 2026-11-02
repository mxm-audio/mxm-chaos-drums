//! The transcendental the core needs, in basic IEEE operations.
//!
//! The performance must be identical on every platform (the owner, 2026-11-02), so nothing that
//! reaches a cell's state calls the platform's maths library: chaos turns its last-bit differences
//! into a different performance. Addition, multiplication, division, `sqrt`, `floor`, comparisons
//! and bit operations are exactly specified by IEEE 754 and so the same everywhere; what else the
//! core needs is built from them here, or is the kit's `tan_approx` and `tanh_approx`.

/// `ln(2)^k / k!` for `k` = 1…7: the Taylor series of `2^f = e^(f ln 2)`.
const EXP2_TAYLOR: [f32; 7] = [
    core::f32::consts::LN_2,
    0.240_226_5,
    0.055_504_11,
    0.009_618_129,
    0.001_333_356,
    0.000_154_035_3,
    0.000_015_252_73,
];

/// `2^x`: the integer part built straight into the exponent bits, the fractional part from its
/// degree-7 Taylor polynomial, within 3e-6 relative. `x` is clamped to the normal range.
#[must_use]
pub fn exp2(x: f32) -> f32 {
    let x = x.clamp(-126.0, 126.0);
    let whole = x.floor();
    let f = x - whole;
    let c = EXP2_TAYLOR;
    let fraction = 1.0
        + f * (c[0] + f * (c[1] + f * (c[2] + f * (c[3] + f * (c[4] + f * (c[5] + f * c[6]))))));
    // `whole` is an integer in [-126, 126], so the biased exponent is in [1, 253].
    let scale = f32::from_bits(((whole as i32 + 127) as u32) << 23);
    fraction * scale
}

/// `1/k!` for the odd `k` = 3…11, signed: the Taylor series of `sin`.
const SIN_TAYLOR: [f32; 5] = [
    -1.0 / 6.0,
    1.0 / 120.0,
    -1.0 / 5040.0,
    1.0 / 362_880.0,
    -1.0 / 39_916_800.0,
];

/// `sin(2π t)`, for `t` in turns: reduced to a quarter turn by symmetry, then a degree-11 Taylor
/// polynomial, within 1e-7 absolute. For phases, so `t` is best near `[0, 1)`.
#[must_use]
pub fn sin_turns(t: f32) -> f32 {
    let t = t - (t + 0.5).floor();
    let t = if t > 0.25 {
        0.5 - t
    } else if t < -0.25 {
        -0.5 - t
    } else {
        t
    };
    let x = core::f32::consts::TAU * t;
    let x2 = x * x;
    let c = SIN_TAYLOR;
    x * (1.0 + x2 * (c[0] + x2 * (c[1] + x2 * (c[2] + x2 * (c[3] + x2 * c[4])))))
}

/// `cos(2π t)`, for `t` in turns.
#[must_use]
pub fn cos_turns(t: f32) -> f32 {
    sin_turns(t + 0.25)
}

/// The frequency ratio of `semitones`, `2^(semitones / 12)`.
#[must_use]
pub fn semitones(semitones: f32) -> f32 {
    exp2(semitones / 12.0)
}
