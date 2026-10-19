use std::env;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::Instant;

mod hash;
use hash::{sum_block_hash, stretch_md5};

const BLOCK_SIZE: usize = 1024 * 1024;
const K: u64 = 1000;
const CAPACITY: usize = 2;

fn read_blocks(file: &File, block_size: usize, tx: SyncSender<io::Result<Vec<u8>>>) -> io::Result<()> {
    let total_size = file.metadata()?.len();
    let num_blocks = total_size / block_size as u64 + u64::from(total_size % block_size as u64 != 0);
    for block_idx in 0..num_blocks {
        let offset = block_idx * block_size as u64;
        let len = (total_size - offset).min(block_size as u64) as usize;
        let mut block = vec![0u8; len];
        if let Err(error) = file.read_exact_at(&mut block, offset) {
            let _ = tx.send(Err(io::Error::new(error.kind(), error.to_string())));
            return Err(error);
        }
        if tx.send(Ok(block)).is_err() { break; }
    }
    Ok(())
}

fn compute_hashes(rx: Receiver<io::Result<Vec<u8>>>, k: u64) -> io::Result<Vec<u8>> {
    let mut result = vec![0u8; 16];
    for message in rx {
        let block = message?;
        let block_hash = stretch_md5(&block, k);
        sum_block_hash(&mut result, &block_hash);
    }
    Ok(result)
}

fn calculate(file: &File, block_size: usize, k: u64, capacity: usize) -> io::Result<Vec<u8>> {
    let (tx, rx) = mpsc::sync_channel(capacity);
    thread::scope(|scope| {
        let reader = scope.spawn(move || read_blocks(file, block_size, tx));
        let computed = compute_hashes(rx, k); // main thread is the consumer
        let read_result = reader.join().expect("reader thread panicked");
        read_result?;
        computed
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.len() > 5 {
        eprintln!("Usage: pipeline FILE [BLOCK_SIZE] [K=1000] [CAPACITY=2]");
        std::process::exit(2);
    }
    let block_size = args.get(2).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(BLOCK_SIZE);
    let k = args.get(3).map(|s| s.parse::<u64>()).transpose()?.unwrap_or(K);
    let capacity = args.get(4).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(CAPACITY);
    if block_size == 0 || k == 0 { return Err("BLOCK_SIZE and K must be positive".into()); }
    let file = File::open(&args[1])?;
    let start = Instant::now();
    let result = calculate(&file, block_size, k, capacity)?;
    eprintln!("elapsed_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    print!("{} 0x", args[1]);
    for byte in result { print!("{byte:02x}"); }
    println!();
    Ok(())
}
