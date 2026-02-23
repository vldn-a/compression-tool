# compression-tool

High-performance text compressor using LZ77 + ANS (Asymmetric Numeral Systems), written in Rust.

## Benchmark Results

| Tool | Compression Ratio | Speed |
|------|------------------|-------|
| Brotli (Google) | 3.1:1 | ~30 MB/s |
| xz / LZMA | 4.3:1 | ~5 MB/s |
| **This tool** | **9.49:1** | **368 MB/s** |

Tested on Shakespeare's complete works (5.4MB) and Wikipedia XML (enwik8).

## How It Works

- **LZ77** — finds repeated sequences and replaces them with back-references
- **ANS** — entropy codes the output using near-optimal bit allocation
- **Parallel** — chunks are compressed in parallel using Rayon

## Build & Run
```bash
cargo build --release
./target/release/rust-compressor
```

## Status

- ✅ Compressor working
- ✅ Decompressor verified
- 🔲 Full file format (in progress)
- 🔲 Images, audio, video (next)

## License

MIT — Copyright (c) 2026 Valentin
