const F64_MANTISSA_BITS: u32 = 53;

pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed =
            (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed =
            (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }

    pub(crate) fn unit(&mut self) -> f64 {
        let mantissa =
            self.next_u64() >> (u64::BITS - F64_MANTISSA_BITS);
        mantissa as f64 / (1u64 << F64_MANTISSA_BITS) as f64
    }

    pub(crate) fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }
}

pub(crate) fn pick_weighted<'t, T>(
    rng: &mut SplitMix64,
    table: &'t [(T, u32)],
) -> &'t T {
    let total: u32 = table.iter().map(|(_, weight)| *weight).sum();
    let mut roll = rng.below(u64::from(total)) as u32;
    for (item, weight) in table {
        if roll < *weight {
            return item;
        }
        roll -= weight;
    }
    unreachable!("weighted roll exceeded {total}")
}

// pool^u inverts the zipf(1) CDF, so low ranks dominate.
pub(crate) fn zipf_index(rng: &mut SplitMix64, pool: usize) -> usize {
    let rank = (pool as f64).powf(rng.unit()) as usize;
    rank.clamp(1, pool) - 1
}
