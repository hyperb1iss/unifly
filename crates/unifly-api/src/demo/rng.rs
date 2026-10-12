// ── Deterministic randomness for the demo fleet ──
//
// A tiny splitmix64 generator plus stateless hash noise. Everything the
// demo produces flows from one seed so a recording can be reshot frame for
// frame. Numeric conversions live here too, so the lint allowances for
// float/int casts sit in one documented place instead of across the module.

/// splitmix64: small, fast, and good enough for synthetic telemetry.
#[derive(Debug, Clone)]
pub(crate) struct DemoRng(u64);

const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

impl DemoRng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Derive an independent stream, so adding draws to one part of the
    /// generator does not reshuffle every other part.
    pub(crate) fn fork(&self, salt: u64) -> Self {
        Self(mix(self.0 ^ mix(salt)))
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN_GAMMA);
        mix(self.0)
    }

    /// Uniform in `[0, 1)`.
    pub(crate) fn unit(&mut self) -> f64 {
        unit_from_bits(self.next_u64())
    }

    /// Uniform index in `0..len`; `len` must be non-zero.
    pub(crate) fn index(&mut self, len: usize) -> usize {
        let len64 = u64::try_from(len).unwrap_or(u64::MAX).max(1);
        usize::try_from(self.next_u64() % len64).unwrap_or(0)
    }

    /// Uniform integer in `lo..=hi`.
    pub(crate) fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u64() % (hi - lo + 1)
    }

    /// Uniform float in `lo..hi`.
    pub(crate) fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    pub(crate) fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }

    /// Pick from a non-empty table.
    pub(crate) fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.index(items.len())]
    }

    /// A locally administered, unicast MAC (`x2`, `x6`, `xA`, `xE` first
    /// octet), the same shape phones use for private Wi-Fi addresses.
    pub(crate) fn mac(&mut self) -> String {
        let bits = self.next_u64();
        let bytes = bits.to_be_bytes();
        let first = (bytes[2] & 0xFC) | 0x02;
        format!(
            "{first:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]
        )
    }

    /// A 24-hex-digit identifier shaped like a Session API `_id`.
    pub(crate) fn object_id(&mut self) -> String {
        format!(
            "{:016x}{:08x}",
            self.next_u64(),
            self.next_u64() & 0xFFFF_FFFF
        )
    }
}

/// splitmix64 finalizer: a strong 64-bit mixing function.
pub(crate) fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Stateless noise in `[0, 1)` for a `(seed, key)` pair. Historical series
/// use this so the same bucket always yields the same value across period
/// switches and auto-refreshes.
pub(crate) fn hash_unit(seed: u64, key: u64) -> f64 {
    unit_from_bits(mix(seed ^ mix(key.wrapping_add(GOLDEN_GAMMA))))
}

/// Stable 64-bit hash of a string, for per-entity streams keyed by MAC.
pub(crate) fn hash_str(value: &str) -> u64 {
    value.bytes().fold(0xCBF2_9CE4_8422_2325, |acc, byte| {
        (acc ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

fn unit_from_bits(bits: u64) -> f64 {
    // Top 32 bits are plenty of resolution for telemetry jitter.
    let top = u32::try_from(bits >> 32).unwrap_or(u32::MAX);
    f64::from(top) / 4_294_967_296.0
}

// ── Numeric conversions ──────────────────────────────────────────
//
// Telemetry is modeled in f64 and stored in the domain types as integers.
// These helpers saturate instead of wrapping and are the only place in the
// demo module that needs cast lint allowances.

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn to_u64(value: f64) -> u64 {
    if value.is_nan() || value <= 0.0 {
        0
    } else {
        value.round() as u64
    }
}

#[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
pub(crate) fn to_i32(value: f64) -> i32 {
    if value.is_nan() {
        0
    } else {
        value
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    }
}

#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
pub(crate) fn from_u64(value: u64) -> f64 {
    value as f64
}

#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
pub(crate) fn from_i64(value: i64) -> f64 {
    value as f64
}

#[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
pub(crate) fn to_i64(value: f64) -> i64 {
    if value.is_nan() {
        0
    } else {
        value.round() as i64
    }
}

pub(crate) fn from_usize(value: usize) -> f64 {
    from_u64(u64::try_from(value).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = DemoRng::new(7);
        let mut b = DemoRng::new(7);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn macs_are_locally_administered_unicast() {
        let mut rng = DemoRng::new(99);
        for _ in 0..256 {
            let mac = rng.mac();
            let first = u8::from_str_radix(&mac[..2], 16).expect("hex octet");
            assert_eq!(first & 0x02, 0x02, "{mac} must be locally administered");
            assert_eq!(first & 0x01, 0x00, "{mac} must be unicast");
        }
    }

    #[test]
    fn unit_stays_in_range() {
        let mut rng = DemoRng::new(3);
        for _ in 0..1_000 {
            let value = rng.unit();
            assert!((0.0..1.0).contains(&value));
        }
        assert!((0.0..1.0).contains(&hash_unit(1, 2)));
    }

    #[test]
    fn conversions_saturate() {
        assert_eq!(to_u64(-4.0), 0);
        assert_eq!(to_u64(f64::NAN), 0);
        assert_eq!(to_u64(1e30), u64::MAX);
        assert_eq!(to_i32(-67.4), -67);
    }
}
