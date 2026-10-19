use std::env;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;
use rand::{rng, Rng, RngExt, SeedableRng};
use rand::rngs::StdRng;

mod hash;
use hash::sum_block_hash;

pub struct RandomOrderIterator {
    elements: Vec<u64>,
    rng: Box<dyn Rng>,
}
impl RandomOrderIterator {
    pub fn with_seed(size: u64, seed: u64) -> Self {
        Self { elements: (0..size).collect(), rng: Box::new(StdRng::seed_from_u64(seed)) }
    }
    pub fn new(size: u64) -> Self {
        Self { elements: (0..size).collect(), rng: Box::new(rng()) }
    }
}
impl Iterator for RandomOrderIterator {
    type Item = u64;
    fn next(&mut self) -> Option<Self::Item> {
        if self.elements.is_empty() { return None; }
        let index = self.rng.random_range(0..self.elements.len());
        Some(self.elements.swap_remove(index))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.elements.len(), Some(self.elements.len()))
    }
}

const BLOCK_SIZE: usize = 512;
const RAND_SEED: u64 = 1234;
const THREADS: usize = 4;

fn read_blocks(file: &File, order: &[u64], next: &AtomicUsize,
               block_size: usize, total_size: u64, tx: mpsc::SyncSender<io::Result<Vec<u8>>>) {
    loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        if index >= order.len() { break; }
        let offset = order[index] * block_size as u64;
        let len = (total_size - offset).min(block_size as u64) as usize;
        let mut block = vec![0; len];
        let message = file.read_exact_at(&mut block, offset).map(|_| block);
        let failed = message.is_err();
        if tx.send(message).is_err() || failed { break; }
    }
}

fn compute_hashes(rx: mpsc::Receiver<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    let mut result = vec![0u8; 16];
    let mut first_error = None;
    for message in rx {
        match message {
            Ok(block) if first_error.is_none() => {
                let block_hash = md5::compute(&block);
                sum_block_hash(&mut result, &block_hash);
            }
            Err(error) if first_error.is_none() => first_error = Some(error),
            _ => {}
        }
    }
    match first_error { Some(error) => Err(error), None => Ok(result) }
}

fn calculate(file: &File, block_size: usize, threads: usize) -> io::Result<Vec<u8>> {
    let total_size = file.metadata()?.len();
    let num_blocks = total_size / block_size as u64 + u64::from(total_size % block_size as u64 != 0);
    // Original seeded random iterator is preserved.
    let order: Vec<u64> = RandomOrderIterator::with_seed(num_blocks, RAND_SEED).collect();
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::sync_channel(threads.saturating_mul(2));
    thread::scope(|scope| {
        for _ in 0..threads {
            let sender = tx.clone();
            let order_ref = &order;
            let next_ref = &next;
            scope.spawn(move || read_blocks(file, order_ref, next_ref, block_size, total_size, sender));
        }
        drop(tx);
        compute_hashes(rx) // single consumer on main thread
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.len() > 4 {
        eprintln!("Usage: latency FILE [BLOCK_SIZE=512] [THREADS=4]");
        std::process::exit(2);
    }
    let block_size = args.get(2).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(BLOCK_SIZE);
    let threads = args.get(3).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(THREADS);
    if block_size == 0 || threads == 0 { return Err("BLOCK_SIZE and THREADS must be positive".into()); }
    let file = File::open(&args[1])?;
    let start = Instant::now();
    let result = calculate(&file, block_size, threads)?;
    eprintln!("elapsed_ms={:.3}", start.elapsed().as_secs_f64() * 1000.0);
    print!("{} 0x", args[1]);
    for byte in result { print!("{byte:02x}"); }
    println!();
    Ok(())
}
