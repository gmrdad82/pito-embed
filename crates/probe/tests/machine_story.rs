use probe::{Lane, Machine, Os, Vendor, probe_machine};

fn lanes(machine: &Machine) -> [Lane; 3] {
    [machine.listening, machine.filing, machine.video]
}

#[test]
fn this_host_reports_the_os_it_was_built_for() {
    let machine = probe_machine();
    let want = if cfg!(target_os = "linux") {
        Os::Linux
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Other
    };
    assert_eq!(machine.os, want);
    assert!(machine.cores >= 1);
}

#[test]
fn filing_runs_on_the_processor_exactly_when_there_is_no_card() {
    let machine = probe_machine();
    if machine.os == Os::Other {
        assert_eq!(machine.filing, Lane::Processor);
        return;
    }
    assert_eq!(
        machine.filing == Lane::Processor,
        machine.vendor == Vendor::None,
        "{machine:?}"
    );
}

#[test]
fn a_machine_with_no_card_runs_every_lane_on_the_processor_and_is_not_accelerated() {
    let machine = probe_machine();
    if machine.vendor == Vendor::None {
        assert_eq!(lanes(&machine), [Lane::Processor; 3], "{machine:?}");
        assert!(!machine.accelerated());
        assert_eq!(machine.vram_bytes, None);
    } else {
        assert!(machine.listening.accelerated(), "{machine:?}");
        assert_eq!(
            machine.accelerated(),
            machine.video.accelerated(),
            "{machine:?}"
        );
    }
}

#[test]
fn a_card_that_shares_memory_never_reports_its_own_vram() {
    let machine = probe_machine();
    if matches!(machine.vendor, Vendor::Intel | Vendor::Apple | Vendor::None) {
        assert_eq!(machine.vram_bytes, None, "{machine:?}");
    }
}

#[test]
fn the_headline_spec_line_and_title_read_for_this_host() {
    let machine = probe_machine();
    let headline = machine.headline();
    assert!(headline.starts_with(machine.os.label()), "{headline}");

    let spec = machine.spec_line();
    assert!(spec.contains(&format!("{} cores", machine.cores)), "{spec}");
    assert!(!spec.starts_with(" · ") && !spec.ends_with(" · "), "{spec}");

    let title = machine.title();
    assert!(title.ends_with(" system information"), "{title}");
    assert!(title.len() > " system information".len(), "{title}");
}

#[test]
fn probing_twice_gives_the_same_machine() {
    assert_eq!(probe_machine(), probe_machine());
}
