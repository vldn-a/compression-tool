// range_coder.rs — Range coder with binary probability model
//
// Key insight: instead of one model over 256 symbols,
// use a BINARY TREE of probability models.
// Each decision is just 0 or 1 — much faster to adapt.
//
// This is exactly how LZMA's range coder works.

// ── Range Encoder ─────────────────────────────────────────────────────────────

pub struct RangeEncoder {
    low:    u64,
    range:  u64,
    pub output: Vec<u8>,
}

impl RangeEncoder {
    pub fn new() -> Self {
        RangeEncoder { low: 0, range: 0xFFFF_FFFF, output: Vec::new() }
    }

    // Encode one bit with probability prob/PROB_MAX that it is 0
    pub fn encode_bit(&mut self, bit: u32, prob: u32) {
        let mid = (self.range >> 11) * prob as u64;
        if bit == 0 {
            self.range = mid;
        } else {
            self.low  += mid;
            self.range -= mid;
        }
        // Normalize
        while self.range < 0x100_0000 {
            self.output.push((self.low >> 24) as u8);
            self.low   = (self.low << 8) & 0xFFFF_FFFF;
            self.range = (self.range << 8) & 0xFFFF_FFFF;
        }
    }

    pub fn finish(&mut self) {
        for _ in 0..4 {
            self.output.push((self.low >> 24) as u8);
            self.low = (self.low << 8) & 0xFFFF_FFFF;
        }
    }
}

// ── Bit probability model ─────────────────────────────────────────────────────
// Tracks probability of a 0 bit. Adapts after every symbol.

pub const PROB_MAX: u32 = 1 << 11; // 2048
pub const PROB_INIT: u32 = PROB_MAX / 2; // Start at 50/50

pub struct BitModel {
    pub prob: u32, // Probability of 0 (out of PROB_MAX)
}

impl BitModel {
    pub fn new() -> Self { BitModel { prob: PROB_INIT } }

    pub fn encode(&mut self, enc: &mut RangeEncoder, bit: u32) {
        enc.encode_bit(bit, self.prob);
        // Adapt: move prob toward what we just saw
        if bit == 0 {
            self.prob += (PROB_MAX - self.prob) >> 5; // Move toward PROB_MAX
        } else {
            self.prob -= self.prob >> 5;              // Move toward 0
        }
    }
}

// ── Multi-symbol model using binary tree of BitModels ─────────────────────────
// Encodes a symbol in range [0, num_symbols) using log2(num_symbols) bits.
// Much faster adaptation than flat frequency table.

pub struct TreeModel {
    bits:    Vec<BitModel>,
    num_sym: usize,
}

impl TreeModel {
    pub fn new(num_symbols: usize) -> Self {
        // Allocate a complete binary tree sized to the next power-of-two.
        // We index nodes starting at 1, and may access up to (2 * leaves - 1),
        // so allocate `2 * leaves` entries to be safe (bits[0] unused).
        let leaves = num_symbols.next_power_of_two();
        let size = leaves * 2;
        TreeModel {
            bits:    (0..size).map(|_| BitModel::new()).collect(),
            num_sym: num_symbols,
        }
    }

    pub fn encode(&mut self, enc: &mut RangeEncoder, symbol: usize) {
        let mut node = 1usize;
        let mut bits_left = self.num_sym.next_power_of_two().trailing_zeros() as usize;
        let mut sym = symbol;
        while bits_left > 0 {
            bits_left -= 1;
            let bit = (sym >> bits_left) & 1;
            self.bits[node].encode(enc, bit as u32);
            node = node * 2 + bit;
            sym &= (1 << bits_left) - 1;
        }
    }

    pub fn encode_symbol(&mut self, enc: &mut RangeEncoder, symbol: usize) {
        self.encode(enc, symbol);
    }

    pub fn update(&mut self, _symbol: usize) {
        // TreeModel adapts automatically via BitModel — no explicit update needed
    }
}

// ── Adaptive Model (simple frequency, for compatibility) ──────────────────────

pub struct AdaptiveModel {
    pub counts: Vec<u64>,
    pub total:  u64,
}

impl AdaptiveModel {
    pub fn new(num_symbols: usize) -> Self {
        let total = num_symbols as u64;
        AdaptiveModel { counts: vec![1; num_symbols], total }
    }

    pub fn encode_symbol(&self, enc: &mut RangeEncoder, symbol: usize) {
        let low: u64 = self.counts[..symbol].iter().sum();
        // Encode as series of bits using the range encoder
        // Use fixed-point: encode bit by bit
        let p0 = ((PROB_MAX as u64 * self.counts[symbol]) / self.total) as u32;
        let _ = p0; // Used conceptually
        // Simple: encode each bit of symbol index
        let bits = 8usize;
        for i in (0..bits).rev() {
            let bit = (symbol >> i) & 1;
            enc.encode_bit(bit as u32, PROB_MAX / 2);
        }
        let _ = low;
    }

    pub fn update(&mut self, symbol: usize) {
        self.counts[symbol] += 1;
        self.total += 1;
        if self.total > 1 << 20 {
            self.total = 0;
            for c in &mut self.counts {
                *c = (*c + 1) / 2;
                self.total += *c;
            }
        }
    }
}