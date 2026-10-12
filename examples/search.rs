use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use embed::{EmbedDevice, Embedder};
use fetch::{CancelToken, Fetcher, Request};

const MODEL_PREFIX: &str = "models/";
const LINES: [&str; 8] = [
    "ninety-nine percent. it died at ninety-nine percent.",
    "calm run, patient, no dying",
    "the soundtrack in this level is incredible",
    "I keep falling off the same ledge",
    "we finally beat the final boss on hard",
    "the menus load slowly on this console",
    "found a secret room behind the waterfall",
    "the patch notes fixed the save bug",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let query = if query.is_empty() {
        "a hidden area".to_string()
    } else {
        query
    };

    let dir = model_dir().ok_or("no home folder to keep the model in: set PITO_EMBED_MODELS")?;
    fetch_model(&dir)?;

    let machine = probe::probe_machine();
    let device = EmbedDevice::try_from(machine.filing).unwrap_or(EmbedDevice::Cpu);
    let started = Instant::now();
    let mut embedder = Embedder::load(&dir, device)?;
    println!(
        "loaded EmbeddingGemma-300m on {} in {} ms",
        embedder.device_in_use().label(),
        started.elapsed().as_millis()
    );
    if let Some(reason) = embedder.fall_off_reason() {
        println!("the card refused, so it runs on the processor: {reason}");
    }

    let started = Instant::now();
    let docs = embedder.embed_documents(&LINES)?;
    let query_vec = embedder.embed_query(&query)?;
    println!(
        "embedded {} lines and the query in {} ms, {} dimensions each",
        LINES.len(),
        started.elapsed().as_millis(),
        query_vec.len()
    );

    let mut ranked: Vec<(f32, &str)> = docs
        .iter()
        .zip(LINES)
        .map(|(doc, line)| (cosine(&query_vec, doc), line))
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));

    println!();
    println!("query: {query}");
    for (score, line) in ranked.iter().take(3) {
        println!("  {score:.3}  {line}");
    }
    Ok(())
}

fn model_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PITO_EMBED_MODELS") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data = if cfg!(target_os = "macos") {
        home?.join("Library/Application Support")
    } else {
        match std::env::var_os("XDG_DATA_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => home?.join(".local/share"),
        }
    };
    Some(data.join("pito-embed/embeddinggemma-300m-q4"))
}

fn fetch_model(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let fetcher = Fetcher::new(dir);
    let cancel = CancelToken::new();
    for spec in embed::catalog() {
        let rel = spec
            .dest_rel
            .strip_prefix(MODEL_PREFIX)
            .and_then(|rest| rest.split_once('/'))
            .map_or(spec.dest_rel, |(_, file)| file);
        let have = std::fs::metadata(dir.join(rel)).map(|m| m.len()).ok();
        if have.is_some() && have == spec.size_hint {
            continue;
        }
        let request = Request {
            url: spec.url.to_string(),
            dest_rel: rel.to_string(),
            sha256: spec.sha256.to_string(),
            size_hint: spec.size_hint,
        };
        fetcher.fetch_request(&request, &cancel, |p| {
            if let Some(total) = p.total.filter(|t| *t > 0) {
                eprint!("\rdownloading {rel} {}%", p.received * 100 / total);
                let _ = std::io::stderr().flush();
            }
        })?;
        eprintln!("\rdownloaded {rel}, sha256 checked");
    }
    Ok(())
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
