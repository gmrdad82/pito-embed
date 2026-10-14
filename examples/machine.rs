use std::path::Path;

fn main() {
    let machine = probe::probe_machine();
    println!("{}", machine.title());
    println!("{}", machine.headline());
    println!("{}", machine.spec_line());

    println!();
    println!("embeddings  {}", machine.filing.label());
    println!("speech      {}", machine.listening.label());
    println!("video       {}", machine.video.label());

    println!();
    for adapter in probe::probe_gpu().adapters {
        if adapter.backend == probe::Backend::Gl {
            continue;
        }
        println!(
            "adapter     {}, {:?}, {:?}",
            adapter.name, adapter.backend, adapter.device_type
        );
    }
    if let Some(disk) = probe::probe_disk(Path::new(".")) {
        println!(
            "disk        {} GB free of {} GB",
            disk.free_bytes / 1_000_000_000,
            disk.total_bytes / 1_000_000_000
        );
    }
}
