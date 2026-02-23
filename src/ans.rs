// ans.rs — streaming rANS, no reversal tricks
pub const SCALE_BITS: u32 = 12;
pub const SCALE: u32 = 1 << SCALE_BITS; // 4096

pub struct AnsState {
    pub freqs:  Vec<u32>,
    pub cmf:    Vec<u32>, // cumulative mass function
}

impl AnsState {
    pub fn from_data(data: &[u8]) -> Self {
        let mut counts = vec![0u32; 256];
        for &b in data { counts[b as usize] += 1; }
        let total: u32 = counts.iter().sum();
        
        // Normalize to SCALE
        let mut freqs = vec![0u32; 256];
        let mut sum = 0u32;
        for i in 0..256 {
            if counts[i] > 0 {
                freqs[i] = ((counts[i] as u64 * SCALE as u64) / total as u64).max(1) as u32;
                sum += freqs[i];
            }
        }
        // Fix rounding
        while sum < SCALE {
            let i = (0..256).filter(|&i| counts[i] > 0)
                .max_by_key(|&i| counts[i]).unwrap();
            freqs[i] += 1; sum += 1;
        }
        while sum > SCALE {
            let i = (0..256).filter(|&i| freqs[i] > 1)
                .max_by_key(|&i| freqs[i]).unwrap();
            freqs[i] -= 1; sum -= 1;
        }
        
        let mut cmf = vec![0u32; 257];
        for i in 0..256 { cmf[i+1] = cmf[i] + freqs[i]; }
        
        AnsState { freqs, cmf }
    }
    
    pub fn from_freqs(freqs: Vec<u32>) -> Self {
        let mut cmf = vec![0u32; 257];
        for i in 0..256 { cmf[i+1] = cmf[i] + freqs[i]; }
        AnsState { freqs, cmf }
    }
}

pub fn ans_compress(data: &[u8]) -> Vec<u8> {
    if data.is_empty() { return vec![]; }
    
    let model = AnsState::from_data(data);
    
    // Encode backwards into a stack
    let mut state: u64 = SCALE as u64;
    let mut stack: Vec<u8> = Vec::new();
    
    for &b in data.iter().rev() {
        let sym = b as usize;
        let f = model.freqs[sym] as u64;
        let c = model.cmf[sym] as u64;
        
        // Renormalize
        let max_state = (f << (32 - SCALE_BITS)) as u64;
        while state >= max_state {
            stack.push(state as u8);
            state >>= 8;
        }
        
        // Encode
        state = (state / f) * SCALE as u64 + c + (state % f);
    }
    
    // Write output: header then state then stack reversed
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    for &f in &model.freqs { out.extend_from_slice(&f.to_le_bytes()); }
    // State
    out.extend_from_slice(&(state as u32).to_le_bytes());
    // Stack in reverse = decode order
    for &b in stack.iter().rev() { out.push(b); }
    out
}