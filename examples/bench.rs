use std::path::PathBuf;
use std::time::{Duration, Instant};

use embed::{EmbedDevice, Embedder};

const LINES: [&str; 16] = [
    "ninety-nine percent. it died at ninety-nine percent.",
    "calm run, patient, no dying",
    "the soundtrack in this level is incredible",
    "I keep falling off the same ledge",
    "we finally beat the final boss on hard",
    "the menus load slowly on this console",
    "found a secret room behind the waterfall",
    "the patch notes fixed the save bug",
    "two players, one controller, a lot of shouting",
    "the speedrun route skips the whole second act",
    "this puzzle took us the entire evening",
    "the frame rate drops in the big city",
    "my daughter named her horse after the dog",
    "the co-op mode is better than the story",
    "a tiny indie game with a huge heart",
    "the last checkpoint is far too far back",
];
const DOCS: usize = 256;
const BATCH: usize = 32;
const QUERIES: usize = 20;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::var_os("PITO_EMBED_MODELS")
        .map(PathBuf::from)
        .ok_or("set PITO_EMBED_MODELS to the model's folder (the search example fills one)")?;

    let docs: Vec<String> = (0..DOCS)
        .map(|i| format!("{} #{i}", LINES[i % LINES.len()]))
        .collect();
    let docs: Vec<&str> = docs.iter().map(String::as_str).collect();

    println!("EmbeddingGemma-300m q4, {DOCS} lines in batches of {BATCH}, {QUERIES} queries");
    println!();
    println!(
        "{:<14}{:>9}{:>13}{:>9}{:>12}{:>12}",
        "device", "load", "first query", "query", "256 lines", "lines/s"
    );

    let mut devices = vec![EmbedDevice::compiled()];
    if devices[0] != EmbedDevice::Cpu {
        devices.push(EmbedDevice::Cpu);
    }
    for device in devices {
        let started = Instant::now();
        let mut embedder = Embedder::load(&dir, device)?;
        let load = started.elapsed();
        if let Some(reason) = embedder.fall_off_reason() {
            println!("{} refused, skipped: {reason}", device.label());
            continue;
        }

        let started = Instant::now();
        embedder.embed_query("a hidden area")?;
        let first = started.elapsed();

        let mut times = Vec::with_capacity(QUERIES);
        for i in 0..QUERIES {
            let started = Instant::now();
            embedder.embed_query(LINES[i % LINES.len()])?;
            times.push(started.elapsed());
        }
        times.sort();
        let query = times[QUERIES / 2];

        let started = Instant::now();
        for chunk in docs.chunks(BATCH) {
            embedder.embed_documents(chunk)?;
        }
        let all = started.elapsed();

        println!(
            "{:<14}{:>9}{:>13}{:>9}{:>12}{:>12.0}",
            embedder.device_in_use().label(),
            ms(load),
            ms(first),
            ms(query),
            ms(all),
            DOCS as f64 / all.as_secs_f64()
        );
    }
    Ok(())
}

fn ms(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 10.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.0} ms")
    }
}
