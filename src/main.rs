// Copyright (c) 2026 Valentin
// Licensed under MIT License
// https://github.com/vldn-a/compression-tool


use std::time::Instant;
use std::fs;
use rayon::prelude::*;
use std::convert::TryInto;

mod ans;
mod ans_decoder;
mod range_coder;
use range_coder::{RangeEncoder, TreeModel};

// ── Tokens ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Token {
    Literal(u8),
    Match { distance: usize, length: usize },
}

// ── LZ77 with two-level hash table ───────────────────────────────────────────

fn lz77_encode(data: &[u8], window_size: usize, min_match: usize, max_match: usize) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(data.len() / 4);

    const FAST_BITS: usize = 16;
    const FAST_SIZE: usize = 1 << FAST_BITS;
    const FAST_MASK: usize = FAST_SIZE - 1;
    const GOOD_BITS: usize = 20;
    const GOOD_SIZE: usize = 1 << GOOD_BITS;
    const GOOD_MASK: usize = GOOD_SIZE - 1;

    let mut fast_table = vec![0u32; FAST_SIZE];
    let mut good_table = vec![0u32; GOOD_SIZE];
    let mut chain      = vec![0u32; data.len()];

    let hash4 = |p: usize| -> usize {
        let v = u32::from_le_bytes([data[p], data[p+1], data[p+2], data[p+3]]);
        (v.wrapping_mul(0x9E3779B9) >> 16) as usize & FAST_MASK
    };

    let hash8 = |p: usize| -> usize {
        let v = u64::from_le_bytes([
            data[p], data[p+1], data[p+2], data[p+3],
            data[p+4], data[p+5], data[p+6], data[p+7],
        ]);
        (v.wrapping_mul(0x517CC1B727220A95) >> 44) as usize & GOOD_MASK
    };

    let mut pos = 0;

    while pos < data.len() {
        if pos + 8 >= data.len() {
            tokens.push(Token::Literal(data[pos]));
            pos += 1;
            continue;
        }

        let hf = hash4(pos);
        let hg = hash8(pos);
        let fast_cand = fast_table[hf] as usize;
        let good_cand = good_table[hg] as usize;

        let mut best_distance = 0;
        let mut best_length   = 0;

        // Check 8-byte hash (likely longer match)
        if good_cand > 0 {
            let candidate = good_cand - 1;
            let distance  = pos - candidate;
            if distance <= window_size {
                let mut length = 0;
                // Match 8 bytes at a time using u64
                while length + 8 <= max_match && pos + length + 8 <= data.len() {
                    let a = u64::from_le_bytes(data[pos+length..pos+length+8].try_into().unwrap());
                    let b = u64::from_le_bytes(data[candidate+length..candidate+length+8].try_into().unwrap());
                    if a != b { length += (a^b).trailing_zeros() as usize / 8; break; }
                    length += 8;
                }
                while length < max_match && pos + length < data.len()
                    && data[candidate+length] == data[pos+length] { length += 1; }
                if length > best_length { best_length = length; best_distance = distance; }
            }
        }

        // Check 4-byte hash chain
        if fast_cand > 0 {
            let mut candidate = fast_cand - 1;
            let mut steps = 0;
            while steps < 16 {
                let distance = pos - candidate;
                if distance > window_size { break; }
                let mut length = 0;
                // Match 8 bytes at a time using u64
                while length + 8 <= max_match && pos + length + 8 <= data.len() {
                    let a = u64::from_le_bytes(data[pos+length..pos+length+8].try_into().unwrap());
                    let b = u64::from_le_bytes(data[candidate+length..candidate+length+8].try_into().unwrap());
                    if a != b { length += (a^b).trailing_zeros() as usize / 8; break; }
                    length += 8;
                }
                while length < max_match && pos + length < data.len()
                    && data[candidate+length] == data[pos+length] { length += 1; }
                if length > best_length { best_length = length; best_distance = distance; }
                if best_length >= 64 { break; }
                if chain[candidate] == 0 { break; }
                candidate = chain[candidate] as usize - 1;
                steps += 1;
            }
        }

        // Update tables
        chain[pos]       = fast_table[hf];
        fast_table[hf]   = (pos + 1) as u32;
        good_table[hg]   = (pos + 1) as u32;

        if best_length >= min_match {
            tokens.push(Token::Match { distance: best_distance, length: best_length });
            pos += best_length;
        } else {
            tokens.push(Token::Literal(data[pos]));
            pos += 1;
        }
    }
    tokens
}

fn lz77_decode(tokens: &[Token]) -> Vec<u8> {
    let mut output = Vec::new();
    for token in tokens {
        match token {
            Token::Literal(b) => output.push(*b),
            Token::Match { distance, length } => {
                let start = output.len() - distance;
                for i in 0..*length {
                    let byte = output[start + i];
                    output.push(byte);
                }
            }
        }
    }
    output
}

// ── Compress: LZ77 + Range Coder ─────────────────────────────────────────────

fn compress(data: &[u8]) -> Vec<u8> {
    // Split into chunks and compress in parallel
    let chunk_size = 65_536; // 64KB
    let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();

    let compressed_chunks: Vec<Vec<u8>> = chunks
        .par_iter()
        .map(|chunk| compress_chunk(chunk))
        .collect();

    // Concatenate all chunks
    let mut output = Vec::new();
    for chunk in compressed_chunks {
        output.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        output.extend_from_slice(&chunk);
    }
    output
}

fn compress_chunk(data: &[u8]) -> Vec<u8> {
    use range_coder::{RangeEncoder, TreeModel};
    let tokens = lz77_encode(data, 1_048_576, 4, 258);

    let mut enc        = RangeEncoder::new();
    let mut type_model = TreeModel::new(2);
    let mut lit_model  = TreeModel::new(256);
    let mut len_model  = TreeModel::new(255);
    let mut dist_model = TreeModel::new(32);

    for token in &tokens {
        match token {
            Token::Literal(b) => {
                type_model.encode_symbol(&mut enc, 0);
                type_model.update(0);
                lit_model.encode_symbol(&mut enc, *b as usize);
                lit_model.update(*b as usize);
            }
            Token::Match { distance, length } => {
                type_model.encode_symbol(&mut enc, 1);
                type_model.update(1);
                let l = (*length - 4).min(254);
                len_model.encode_symbol(&mut enc, l);
                len_model.update(l);
                let d = (usize::BITS - distance.leading_zeros()) as usize;
                let d = d.min(31);
                dist_model.encode_symbol(&mut enc, d);
                dist_model.update(d);
            }
        }
    }
    enc.finish();
    enc.output.clone()
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {

    let datasets = vec![
        ("Shakespeare", "../data/shakespeare.txt"),
        ("Wikipedia",   "../data/enwik8"),
    ];

    for (label, path) in &datasets {
        let text = match fs::read(path) {
            Ok(data) => data,
            Err(_)   => { println!("Could not read {}", path); continue; }
        };

      let data = &text[..text.len().min(10_000_000)]; // 10MB
        let original_bits = data.len() * 8;

        println!("\n{}", "=".repeat(58));
        println!("  {}  ({} bytes)", label, data.len());
        println!("{}", "=".repeat(58));
        println!("  Original:        {:>10} bits  —  1.00 : 1", original_bits);

        // LZ77 estimate only (fast)
        for &window in &[32_768usize, 1_048_576] {
            let start  = Instant::now();
            let tokens = lz77_encode(data, window, 4, 258);
            let elapsed = start.elapsed().as_secs_f64();
            let lits: usize = tokens.iter().filter(|t| matches!(t, Token::Literal(_))).count();
            let refs: usize = tokens.iter().filter(|t| matches!(t, Token::Match{..})).count();
            // Rough bit estimate
            let lit_bits: f64 = {
                let mut counts = [0u64; 256];
                let mut total  = 0u64;
                for t in &tokens { if let Token::Literal(b) = t { counts[*b as usize] += 1; total += 1; } }
                counts.iter().filter(|&&c| c > 0)
                    .map(|&c| { let p = c as f64 / total as f64; -p.log2() * c as f64 })
                    .sum()
            };
            let bits  = lit_bits + refs as f64 * 13.0;
            let ratio = original_bits as f64 / bits;
            let mb_s  = (data.len() as f64 / 1_000_000.0) / elapsed;
            println!("  LZ77 w={:<7}:  {:>10.0} bits  —  {:.2} : 1  ({:.3}s, {:.1} MB/s)  [{} lit, {} ref]",
                     window, bits, ratio, elapsed, mb_s, lits, refs);
        }

        // Full compressor: LZ77 + Range Coder
        let start      = Instant::now();
        let compressed = compress(data);
        let elapsed    = start.elapsed().as_secs_f64();
        let bits       = compressed.len() * 8;
        let ratio      = original_bits as f64 / bits as f64;
        let mb_s       = (data.len() as f64 / 1_000_000.0) / elapsed;
        println!("  LZ77 + RangeCod: {:>10} bits  —  {:.2} : 1  ({:.3}s, {:.1} MB/s)",
                 bits, ratio, elapsed, mb_s);
        
        // ANS test
        let start = Instant::now();
        let chunks: Vec<&[u8]> = data.chunks(65_536).collect();
        let ans_chunks: Vec<Vec<u8>> = chunks.par_iter()
            .map(|chunk| {
                let tokens = lz77_encode(chunk, 65_536, 4, 258);
                let literals: Vec<u8> = tokens.iter().filter_map(|t| {
                    if let Token::Literal(b) = t { Some(*b) } else { None }
                }).collect();
                ans::ans_compress(&literals)
            })
            .collect();
        let ans_bytes: usize = ans_chunks.iter().map(|c| c.len()).sum();
        let elapsed = start.elapsed().as_secs_f64();
        let mb_s = (data.len() as f64 / 1_000_000.0) / elapsed;
        println!("  ANS + LZ77:      {:>10} bits  —  {:.2} : 1  ({:.3}s, {:.1} MB/s)",
                 ans_bytes * 8,
                 original_bits as f64 / (ans_bytes * 8) as f64,
                 elapsed, mb_s);

        // Debug: test ANS with tiny input
let tiny = b"aaabbc";
        let compressed_tiny = ans::ans_compress(tiny);
        let decompressed_tiny = ans_decoder::ans_decompress(&compressed_tiny);
        println!("  ANS tiny test:   match={}", decompressed_tiny == tiny);
        println!("  compressed bytes: {}", compressed_tiny.len());
        println!("  header size: {}", 4 + 256 * 4);
        println!("  payload bytes: {}", compressed_tiny.len() - (4 + 256 * 4));

        // Verify ANS round-trip
        let sample = &data[..data.len().min(10000)];
        let compressed_sample = ans::ans_compress(sample);
        let decompressed = ans_decoder::ans_decompress(&compressed_sample);
        println!("  ANS round-trip:  {}", if decompressed == sample { "✅ VERIFIED" } else { "❌ FAILED" });

        // Verify decode
        let tokens  = lz77_encode(data, 1_048_576, 4, 258);
        let decoded = lz77_decode(&tokens);
        println!("  Decode correct:  {}", decoded == data);
    }

    println!("\n{}", "=".repeat(58));
    println!("  Targets:");
    println!("{}", "=".repeat(58));
    println!("  Ratio:  > 3.08 : 1  (beat Brotli)");
    println!("  Ratio:  > 4.30 : 1  (beat xz / LZMA)");
    println!("  Speed:  > 338  MB/s (beat zstd level 1)");
}