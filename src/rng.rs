use std::ops::{Range, RangeInclusive};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Rng {
    state: [u64; 4],
}

pub fn rng() -> Rng {
    let mut seed = [0u64; 4];
    let result =
        unsafe { libc::getrandom(seed.as_mut_ptr().cast(), std::mem::size_of_val(&seed), 0) };
    if result != std::mem::size_of_val(&seed) as isize {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos() as u64);
        let mut value =
            time ^ u64::from(std::process::id()) ^ COUNTER.fetch_add(1, Ordering::Relaxed);
        for word in &mut seed {
            *word = splitmix64(&mut value);
        }
    }
    if seed == [0; 4] {
        seed[0] = 1;
    }
    Rng { state: seed }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn splitmix64(value: &mut u64) -> u64 {
    *value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut mixed = *value;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^ (mixed >> 31)
}

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let result = self.state[0]
            .wrapping_add(self.state[3])
            .rotate_left(23)
            .wrapping_add(self.state[0]);
        let shifted = self.state[1] << 17;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= shifted;
        self.state[3] = self.state[3].rotate_left(45);
        result
    }

    fn below(&mut self, upper: u64) -> u64 {
        assert!(upper > 0, "empty random range");
        loop {
            let product = u128::from(self.next_u64()) * u128::from(upper);
            let low = product as u64;
            if low >= upper.wrapping_neg() % upper {
                return (product >> 64) as u64;
            }
        }
    }
}

pub trait SampleUniform: Sized {
    fn sample_range(range: Range<Self>, rng: &mut Rng) -> Self;
    fn sample_range_inclusive(range: RangeInclusive<Self>, rng: &mut Rng) -> Self;
}

pub trait SampleRandom: SampleUniform {
    fn sample(rng: &mut Rng) -> Self;
}

impl SampleUniform for bool {
    fn sample_range(_: Range<Self>, _: &mut Rng) -> Self {
        panic!("bool range")
    }
    fn sample_range_inclusive(_: RangeInclusive<Self>, _: &mut Rng) -> Self {
        panic!("bool range")
    }
}

impl SampleRandom for bool {
    fn sample(rng: &mut Rng) -> Self {
        rng.next_u64() & 1 != 0
    }
}

impl SampleUniform for f64 {
    fn sample_range(range: Range<Self>, rng: &mut Rng) -> Self {
        assert!(range.start < range.end, "empty random range");
        range.start + (range.end - range.start) * rng.random::<Self>()
    }
    fn sample_range_inclusive(range: RangeInclusive<Self>, rng: &mut Rng) -> Self {
        let (start, end) = range.into_inner();
        assert!(start <= end, "empty random range");
        start + (end - start) * rng.random::<Self>()
    }
}

impl SampleRandom for f64 {
    fn sample(rng: &mut Rng) -> Self {
        (rng.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

impl SampleUniform for f32 {
    fn sample_range(range: Range<Self>, rng: &mut Rng) -> Self {
        assert!(range.start < range.end, "empty random range");
        range.start + (range.end - range.start) * rng.random::<Self>()
    }
    fn sample_range_inclusive(range: RangeInclusive<Self>, rng: &mut Rng) -> Self {
        let (start, end) = range.into_inner();
        assert!(start <= end, "empty random range");
        start + (end - start) * rng.random::<Self>()
    }
}

impl SampleRandom for f32 {
    fn sample(rng: &mut Rng) -> Self {
        (rng.next_u64() >> 40) as f32 * (1.0 / (1u32 << 24) as f32)
    }
}

macro_rules! sample_integers {
    ($($type:ty),+ $(,)?) => {$(
        impl SampleRandom for $type {
            fn sample(rng: &mut Rng) -> Self { rng.next_u64() as $type }
        }
    )+};
}

sample_integers!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

pub trait SampleRange<T> {
    fn sample(self, rng: &mut Rng) -> T;
}

impl<T: SampleUniform + PartialOrd> SampleRange<T> for Range<T> {
    fn sample(self, rng: &mut Rng) -> T {
        T::sample_range(self, rng)
    }
}

impl<T: SampleUniform + PartialOrd> SampleRange<T> for RangeInclusive<T> {
    fn sample(self, rng: &mut Rng) -> T {
        T::sample_range_inclusive(self, rng)
    }
}

macro_rules! unsigned_ranges {
    ($($type:ty),+ $(,)?) => {$(
        impl SampleUniform for $type {
            fn sample_range(range: Range<Self>, rng: &mut Rng) -> Self {
                assert!(range.start < range.end, "empty random range");
                let width = (range.end as u128) - (range.start as u128);
                let offset = if width == (1u128 << 64) { rng.next_u64() } else { rng.below(width as u64) };
                range.start.wrapping_add(offset as Self)
            }
            fn sample_range_inclusive(range: RangeInclusive<Self>, rng: &mut Rng) -> Self {
                let (start, end) = range.into_inner();
                assert!(start <= end, "empty random range");
                let width = (end as u128) - (start as u128) + 1;
                let offset = if width == (1u128 << 64) { rng.next_u64() } else { rng.below(width as u64) };
                start.wrapping_add(offset as Self)
            }
        }
    )+};
}

macro_rules! signed_ranges {
    ($($type:ty),+ $(,)?) => {$(
        impl SampleUniform for $type {
            fn sample_range(range: Range<Self>, rng: &mut Rng) -> Self {
                assert!(range.start < range.end, "empty random range");
                let width = range.end as i128 - range.start as i128;
                let offset = if width == (1i128 << 64) { rng.next_u64() } else { rng.below(width as u64) };
                (range.start as i128 + offset as i128) as Self
            }
            fn sample_range_inclusive(range: RangeInclusive<Self>, rng: &mut Rng) -> Self {
                let (start, end) = range.into_inner();
                assert!(start <= end, "empty random range");
                let width = end as i128 - start as i128 + 1;
                let offset = if width == (1i128 << 64) { rng.next_u64() } else { rng.below(width as u64) };
                (start as i128 + offset as i128) as Self
            }
        }
    )+};
}

unsigned_ranges!(u8, u16, u32, u64, usize);
signed_ranges!(i8, i16, i32, i64, isize);

pub trait RngExt {
    fn random<T: SampleRandom>(&mut self) -> T;
    fn random_range<T: SampleUniform, R: SampleRange<T>>(&mut self, range: R) -> T;
    fn random_bool(&mut self, probability: f64) -> bool;
    fn random_ratio(&mut self, numerator: u32, denominator: u32) -> bool;
}

impl RngExt for Rng {
    fn random<T: SampleRandom>(&mut self) -> T {
        T::sample(self)
    }

    fn random_range<T: SampleUniform, R: SampleRange<T>>(&mut self, range: R) -> T {
        range.sample(self)
    }

    fn random_bool(&mut self, probability: f64) -> bool {
        assert!((0.0..=1.0).contains(&probability), "invalid probability");
        self.random::<f64>() < probability
    }

    fn random_ratio(&mut self, numerator: u32, denominator: u32) -> bool {
        assert!(
            numerator <= denominator && denominator != 0,
            "invalid ratio"
        );
        self.random_range(0..denominator) < numerator
    }
}

pub trait SliceRandom {
    type Item;

    fn shuffle(&mut self, rng: &mut Rng);
}

impl<T> SliceRandom for [T] {
    type Item = T;

    fn shuffle(&mut self, rng: &mut Rng) {
        for index in (1..self.len()).rev() {
            self.swap(index, rng.random_range(0..=index));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{rng, RngExt};

    #[test]
    fn integer_range_hits_every_bucket() {
        let mut rng = rng();
        let mut buckets = [false; 10];
        for _ in 0..10_000 {
            let value = rng.random_range(0..10);
            assert!((0..10).contains(&value));
            buckets[value as usize] = true;
        }
        assert!(buckets.into_iter().all(|bucket| bucket));
    }

    #[test]
    fn float_range_stays_below_end() {
        let mut rng = rng();
        for _ in 0..10_000 {
            let value = rng.random_range(0.0..1.0);
            assert!((0.0..1.0).contains(&value));
        }
    }
}
