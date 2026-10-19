use std::env;
use std::fs::File;
use std::io;
use std::time::Instant;
use memmap2::MmapOptions;

mod hash;
use hash::sum_block_hash;

const BLOCK_SIZE: usize = 512;

fn compute_hash_mmap(file: &File, block_size: usize) -> io::Result<Vec<u8>> {
    let mut result = vec![0u8; 16];
    if file.metadata()?.len() == 0 { return Ok(result); }
    let mapping = unsafe { MmapOptions::new().map(file)? };
    for block in mapping.chunks(block_size) {
        let block_hash = md5::compute(block);
        sum_block_hash(&mut result, &block_hash);
    }
    Ok(result)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.len() > 3 {
        eprintln!("Usage: throughput FILE [BLOCK_SIZE=512]");
        std::process::exit(2);
    }
    let block_size = args.get(2).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(BLOCK_SIZE);
    if block_size == 0 { return Err("BLOCK_SIZE must be positive".into()); }
    let file = File::open(&args[1])?;
    let start = Instant::now();
    let result = compute_hash_mmap(&file, block_size)?;
    eprintln!("elapsed_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    print!("{} 0x", args[1]);
    for byte in result { print!("{byte:02x}"); }
    println!();
    Ok(())
}
