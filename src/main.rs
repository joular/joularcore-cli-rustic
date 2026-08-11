/*
 * Copyright (c) 2025-2026, Adel Noureddine.
 * All rights reserved. This program and the accompanying materials
 * are made available under the terms of the
 * GNU General Public License v3.0 only (GPL-3.0-only)
 * which accompanies this distribution, and is available at
 * https://www.gnu.org/licenses/gpl-3.0.en.html
 *
 * Author : Adel Noureddine
 */

mod args;
mod terminal;

use args::{Args, matches_app_name};
use clap::{CommandFactory, FromArgMatches};
use joularcore::monitor::JoularCoreMonitor;
use joularcore::ringbuffer::RingBufferWriter;
use joularcore::{FileWriter, OutputBundle, OutputSink, Schema, Target};
use terminal::{
    TerminalWriter, hide_cursor, print_error, print_success, print_warning, show_cursor,
};

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

const CPU_IDLE_CALIBRATION_SAMPLES: usize = 5;
const CPU_IDLE_CALIBRATION_INTERVAL_SECS: u64 = 1;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Generic runtime/internal failure (I/O, unsupported feature at runtime).
const EXIT_RUNTIME: i32 = 1;
/// A requested resource (PID, app, RAPL, etc.) could not be located.
const EXIT_MISSING: i32 = 3;

fn main() {
    let mut cmd = Args::command();

    let about = format!(
        "\x1b[1;33m⚡ {name} {version}\x1b[0m — is a platform to measure energy across all systems and devices.\n\n\
\x1b[1;36m📝 Description:\x1b[0m Measures CPU, GPU, and total system power in real time.\n\
\x1b[1;36m💻 Supported Systems:\x1b[0m \x1b[32mLinux\x1b[0m, \x1b[32mWindows\x1b[0m, \x1b[32mMacOS\x1b[0m, \x1b[32mRaspberry Pi\x1b[0m, \x1b[32mVirtual Machines\x1b[0m\n\
\x1b[1;36m⚙️ Supported Architectures:\x1b[0m \x1b[34mx86_64 (amd64)\x1b[0m, \x1b[34mx86/i686\x1b[0m, \x1b[34maarch64\x1b[0m, \x1b[34marm\x1b[0m, \x1b[34marmv7\x1b[0m, \x1b[34mGPUs (Nvidia, AMD, Apple)\x1b[0m\n\n\
\x1b[1;36m👤 Author:\x1b[0m \x1b[32mProf. {author}\x1b[0m\n\
\x1b[1;36m📜 License:\x1b[0m \x1b[31mGNU GPL 3 (GPL-3.0-only)\x1b[0m\n\
\x1b[1;36m© Copyright:\x1b[0m \x1b[32m2025-2026 — Prof. {author}\x1b[0m",
        name = cmd.get_name(),
        version = cmd.get_version().unwrap_or("0.0.1"),
        author = cmd.get_author().unwrap_or("Unknown")
    );

    // Parsed once, from the decorated command: parsing a second time from the
    // underived one would discard the banner above.
    cmd = cmd.about(about);
    let args = match Args::from_arg_matches(&cmd.get_matches()) {
        Ok(args) => args,
        Err(e) => e.exit(),
    };

    // The library prints nothing on its own; it emits `log` records. Without a
    // logger installed, "RAPL is not readable" and friends are discarded, and a
    // sensor that reports nothing does so without explanation.
    env_logger::Builder::new()
        .filter_level(args.log_level())
        .parse_default_env()
        .init();

    let config = args.config();

    let builder = JoularCoreMonitor::builder(&config);
    #[cfg(feature = "vm")]
    let builder = apply_vm_sensors(builder, &args);
    let mut monitor = builder.build();

    if args.calibrate_cpu_idle_baseline {
        if !args.numeric_only {
            eprintln!(
                "\x1b[1;33m→ Calibrating CPU idle baseline over {CPU_IDLE_CALIBRATION_SAMPLES} seconds. Keep the machine idle.\x1b[0m"
            );
        }

        let baseline = match monitor.calibrate_cpu_idle_baseline(
            CPU_IDLE_CALIBRATION_SAMPLES,
            Duration::from_secs(CPU_IDLE_CALIBRATION_INTERVAL_SECS),
        ) {
            Ok(baseline) => baseline,
            Err(e) => {
                print_error(&format!("Failed to calibrate CPU idle baseline: {e}"));
                std::process::exit(EXIT_RUNTIME);
            }
        };

        if args.numeric_only {
            println!("{baseline:.2}");
        } else {
            print_success(&format!("Calibrated CPU idle baseline: {baseline:.2} W"));
        }

        // Calibration on its own is a one-shot measurement, not a session.
        if args.pid.is_none()
            && args.app.is_none()
            && args.file.is_none()
            && args.component.is_none()
            && !args.ringbuffer
        {
            std::process::exit(0);
        }
    }

    // Only the in-place line hides the cursor. Numeric output scrolls, and is
    // read by other programs, so it must not carry an escape sequence.
    let live_terminal_output_enabled = !args.silent && args.file.is_none() && !args.numeric_only;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        if live_terminal_output_enabled {
            show_cursor();
        }

        // Change value to stop main loop
        r.store(false, Ordering::SeqCst);
    })
    .expect("Failed to install Ctrl-C handler; another handler may already be installed");

    let exit_with = |code: i32| -> ! {
        if live_terminal_output_enabled {
            show_cursor();
        }
        std::process::exit(code);
    };

    if live_terminal_output_enabled {
        hide_cursor();
    }

    let mut system = System::new();

    // Verify monitoring capabilities and inputs
    if let Some(pid) = args.pid {
        if monitor.platform().process_cpu_usage().is_none() {
            print_error("Process monitoring not supported on this platform");
            exit_with(EXIT_RUNTIME);
        }

        system.refresh_processes(ProcessesToUpdate::All, true);
        if !system.processes().contains_key(&Pid::from_u32(pid)) {
            print_error(&format!(
                "PID {pid} not found (the process may have exited, or you may need elevated privileges to monitor it)"
            ));
            exit_with(EXIT_MISSING);
        }

        if !args.numeric_only {
            print_success(&format!("Monitoring PID {pid}"));
        }
    }

    if let Some(ref app_name) = args.app {
        if monitor
            .platform()
            .app_cpu_usage(config.app_refresh_interval, config.app_match)
            .is_none()
        {
            print_error("Application monitoring not supported on this platform");
            exit_with(EXIT_RUNTIME);
        }

        system.refresh_processes(ProcessesToUpdate::All, true);
        // Matched the way the library will match it while measuring, so a
        // session that starts is one that can actually attribute power.
        let found = system.processes().values().any(|proc| {
            if proc.thread_kind().is_some() {
                return false;
            }
            matches_app_name(&proc.name().to_string_lossy(), app_name, args.app_match)
        });
        if !found {
            print_error(&format!(
                "Application \"{app_name}\" not found (no matching process is currently running, or elevated privileges may be required)"
            ));
            exit_with(EXIT_MISSING);
        }

        if !args.numeric_only {
            print_success(&format!("Monitoring application: {app_name}"));
        }
    }

    // Only print header info if not in numeric-only mode
    if args.file.is_none() && !args.numeric_only {
        println!("\x1b[1;33mJoular Core {}\x1b[0m", env!("CARGO_PKG_VERSION"));
        println!(
            "\x1b[1;36m💻 Platform:\x1b[0m \x1b[32m{}\x1b[0m",
            monitor.platform().name()
        );
    }

    let mut outputs = OutputBundle::new();

    if let Some(path) = &args.file {
        let schema = if args.numeric_only {
            Schema::watts(config.component)
        } else {
            Schema::csv(config.component, &config.target)
        };

        let mut writer = match FileWriter::open(path, schema, args.overwrite) {
            Ok(writer) => writer,
            Err(e) => {
                print_error(&format!("Failed to open output file: {e}"));
                exit_with(EXIT_RUNTIME);
            }
        };

        // A no-op for the bare-wattage schema, and in overwrite mode where the
        // first sample would erase it.
        if let Err(e) = writer.write_header() {
            print_error(&format!("Failed to write CSV header: {e}"));
            exit_with(EXIT_RUNTIME);
        }

        outputs.push(writer);
    } else if !args.silent {
        outputs.push(TerminalWriter::new(
            config.component,
            config.target.clone(),
            args.numeric_only,
        ));
    }

    if args.ringbuffer {
        match RingBufferWriter::new() {
            Ok(writer) => outputs.push(writer),
            Err(e) => print_warning(&format!(
                "Ring buffer unavailable: {e}. Continuing without ring buffer output"
            )),
        }
    }

    // Building the monitor primed the sensors, but a per-target tracker is
    // created with no history: its first reading only establishes a baseline.
    if !matches!(config.target, Target::System) {
        monitor.poll();
    }

    thread::sleep(SAMPLE_INTERVAL);

    while running.load(Ordering::SeqCst) {
        let sample = monitor.poll();

        if let Err(e) = outputs.send(&sample) {
            print_error(&format!("Output Error: {e}"));
            break;
        }

        thread::sleep(SAMPLE_INTERVAL);
    }

    if live_terminal_output_enabled {
        show_cursor();
    }
}

/// Read power from the files a hypervisor writes, when the environment names
/// them. The library supplies the sensor; wiring it in is the program's choice,
/// so an unreadable VM file falls back to the platform's own sensor rather than
/// ending the session.
#[cfg(feature = "vm")]
fn apply_vm_sensors(
    mut builder: joularcore::monitor::MonitorBuilder,
    args: &Args,
) -> joularcore::monitor::MonitorBuilder {
    use joularcore::vm::{VmConfig, VmSensor};

    let vm_config = match VmConfig::from_env() {
        Ok(Some(config)) => config,
        Ok(None) => return builder,
        Err(e) => {
            print_warning(&format!(
                "VM monitoring is misconfigured ({e}); using platform monitoring"
            ));
            return builder;
        }
    };

    match VmSensor::cpu_from_config(&vm_config) {
        Ok(Some(sensor)) => {
            builder = builder.cpu_sensor(Box::new(sensor));
            if !args.numeric_only {
                print_success("VM CPU monitoring");
            }
        }
        Ok(None) => {}
        Err(e) => print_warning(&format!(
            "VM CPU monitoring failed ({e}); falling back to platform CPU monitoring"
        )),
    }

    match VmSensor::gpu_from_config(&vm_config) {
        Ok(Some(sensor)) => {
            builder = builder.gpu_sensor(Box::new(sensor));
            if !args.numeric_only {
                print_success("VM GPU monitoring");
            }
        }
        Ok(None) => {}
        Err(e) => print_warning(&format!(
            "VM GPU monitoring failed ({e}); falling back to platform GPU monitoring"
        )),
    }

    builder
}
