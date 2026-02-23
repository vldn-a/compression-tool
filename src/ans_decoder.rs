// ans_decoder.rs — matches ans.rs exactly
use super::ans::{AnsState, SCALE, SCALE_BITS};

pub fn ans_decompress(compressed: &[u8]) -> Vec<u8> {
    if compressed.is_empty() { return vec![]; }
    
    let original_len = u32::from_le_bytes(compressed[0..4].try_into().unwrap()) as usize;
    let mut freqs = vec![0u32; 256];
    for i in 0..256 {
        freqs[i] = u32::from_le_bytes(compressed[4+i*4..8+i*4].try_into().unwrap());
    }
    let header = 4 + 256*4;
    
    let model = AnsState::from_freqs(freqs);
    
    // Read state
    let mut state = u32::from_le_bytes(compressed[header..header+4].try_into().unwrap()) as u64;
    let mut pos = header + 4;
    
    // Build symbol lookup: slot -> symbol
    let mut sym_of = vec![0u8; SCALE as usize];
    for s in 0..256usize {
        for slot in model.cmf[s]..model.cmf[s+1] {
            sym_of[slot as usize] = s as u8;
        }
    }
    
    let mut out = Vec::with_capacity(original_len);
    
    while out.len() < original_len {
        let slot = (state % SCALE as u64) as usize;
        let sym = sym_of[slot];
        out.push(sym);
        
        let f = model.freqs[sym as usize] as u64;
        let c = model.cmf[sym as usize] as u64;
        
        // Reverse encode step
        state = f * (state >> SCALE_BITS) + (state % SCALE as u64) - c;
        
        // Refill from stream
        while state < (SCALE as u64 * SCALE as u64) && pos < compressed.len() {
            state = (state << 8) | compressed[pos] as u64;
            pos += 1;
        }
    }
    
    out
}