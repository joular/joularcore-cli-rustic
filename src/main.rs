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

use clap::{CommandFactory, Parser};
use joularcore::output::{OutputBundle, OutputSink, OutputWriter};
use joularcore::{args::Args, common, logging, monitor::JoularCoreMonitor};


use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{thread, time};
use sysinfo::{Pid, ProcessesToUpdate, System};

const CPU_IDLE_CALIBRATION_SAMPLES: usize = 5;
const CPU_IDLE_CALIBRATION_INTERVAL_SECS: u64 = 1;

/// Generic runtime/internal failure (I/O, unsupported feature at runtime).
const EXIT_RUNTIME: i32 = 1;
/// Usage error — bad flag combination or environment. Matches the code
/// clap itself emits on parse failure (also sysexits EX_USAGE).
#[allow(dead_code)] // only referenced under cfg(not(feature = "api"))
const EXIT_USAGE: i32 = 2;
/// A requested resource (PID, app, RAPL, etc.) could not be located.
const EXIT_MISSING: i32 = 3;

fn main() {
    logging::init();

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

    cmd = cmd.about(about);
    let _ = cmd.get_matches();
    let args = Args::parse();

    #[cfg(not(feature = "api"))]
    if args.api_port.is_some() {
        logging::print_error(
            "--api-port is unavailable because this binary was compiled without the API feature",
        );
        std::process::exit(EXIT_USAGE);
    }

    // Setup using common logic
    let ctx = common::setup_joularcore(&args);
    let common::JoularContext {
        cpu_energy,
        gpu_energy,
        platform,
        ringbuffer,
        api_sender,
        api_shutdown_tx,
    } = ctx;
    let _api_shutdown_tx = api_shutdown_tx;

    // Initialize process monitoring trackers
    let process_util = platform.process_cpu_usage();
    let app_util =
        platform.app_cpu_usage(std::time::Duration::from_secs(args.app_refresh_interval));
    let cpu_usage = platform.cpu_usage();

    // Initialize monitor
    let mut monitor = JoularCoreMonitor::new(
        platform,
        cpu_energy,
        gpu_energy,
        cpu_usage,
        process_util,
        app_util,
        args.cpu_idle_baseline,
    );

    if args.gui {
        logging::print_error(
            "GUI mode is available as a separate program: 'joularcoregui'",
        );
        std::process::exit(EXIT_RUNTIME);
    }


    if args.calibrate_cpu_idle_baseline {
        if !args.numeric_only {
            eprintln!(
                "\x1b[1;33m→ Calibrating CPU idle baseline over {} seconds. Keep the machine idle.\x1b[0m",
                CPU_IDLE_CALIBRATION_SAMPLES
            );
        }

        let baseline = monitor.calibrate_cpu_idle_baseline(
            CPU_IDLE_CALIBRATION_SAMPLES,
            time::Duration::from_secs(CPU_IDLE_CALIBRATION_INTERVAL_SECS),
        );

        if !args.numeric_only {
            println!(
                "\x1b[1;32m✓ Calibrated CPU idle baseline: {:.2} W\x1b[0m",
                baseline
            );
        } else {
            println!("{:.2}", baseline);
        }

        if args.pid.is_none()
            && args.app.is_none()
            && args.file.is_none()
            && args.component.is_none()
            && !args.ringbuffer
            && args.api_port.is_none()
        {
            std::process::exit(0);
        }
    }

    let live_terminal_output_enabled = !args.silent && args.file.is_none();

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        if live_terminal_output_enabled {
            logging::show_cursor();
        }

        // Change value to stop main loop
        r.store(false, Ordering::SeqCst);
    })
    .expect("Failed to install Ctrl-C handler; another handler may already be installed");

    let exit_with = |code: i32| -> ! {
        if live_terminal_output_enabled {
            logging::show_cursor();
        }
        std::process::exit(code);
    };

    if live_terminal_output_enabled {
        logging::hide_cursor();
    }

    // Initialize output writer
    let mut writer =
        match OutputWriter::new(args.file.as_deref(), args.numeric_only, args.overwrite) {
            Ok(w) => w,
            Err(e) => {
                logging::print_error(&format!("Failed to open output file: {}", e));
                exit_with(EXIT_RUNTIME);
            }
        };

    let mut system = System::new();

    // Verify monitoring capabilities and inputs
    if let Some(pid) = args.pid {
        if monitor.process_tracker.is_none() {
            logging::print_error("Process monitoring not supported on this platform");
            exit_with(EXIT_RUNTIME);
        }

        system.refresh_processes(ProcessesToUpdate::All, true);
        if !system.processes().contains_key(&Pid::from_u32(pid)) {
            logging::print_error(&format!(
                "PID {} not found (the process may have exited, or you may need elevated privileges to monitor it)",
                pid
            ));
            exit_with(EXIT_MISSING);
        }

        if !args.numeric_only {
            println!("\x1b[1;32m✓ Monitoring PID {}\x1b[0m", pid);
        }
    }

    if let Some(ref app_name) = args.app {
        if monitor.app_tracker.is_none() {
            logging::print_error("Application monitoring not supported on this platform");
            exit_with(EXIT_RUNTIME);
        }

        system.refresh_processes(ProcessesToUpdate::All, true);
        let found = system.processes().values().any(|proc| {
            if proc.thread_kind().is_some() {
                return false;
            }
            let name = proc.name().to_string_lossy();
            name == app_name.as_str() || name.contains(app_name.as_str())
        });
        if !found {
            logging::print_error(&format!(
                "Application \"{}\" not found (no matching process is currently running, or elevated privileges may be required)",
                app_name
            ));
            exit_with(EXIT_MISSING);
        }

        if !args.numeric_only {
            println!("\x1b[1;32m✓ Monitoring application: {}\x1b[0m", app_name);
        }
    }

    // Only print header info if not in numeric-only mode
    if args.file.is_none() && !args.numeric_only {
        println!("\x1b[1;33mJoular Core {}\x1b[0m", env!("CARGO_PKG_VERSION"));
        println!(
            "\x1b[1;36m💻 Platform:\x1b[0m \x1b[32m{}\x1b[0m",
            monitor.platform.name()
        );
    }

    // Trackers initialized above

    // Write CSV header if needed
    if args.file.is_some() && !args.numeric_only && !args.overwrite {
        // Header should match the selected monitoring mode, not platform capabilities.
        let has_process = args.pid.is_some();
        let has_app = args.app.is_some();
        if let Err(e) = writer.write_csv_header(args.component.as_ref(), has_process, has_app) {
            logging::print_error(&format!("Failed to write CSV header: {}", e));
            exit_with(EXIT_RUNTIME);
        }
    }

    let mut outputs = OutputBundle::new(args.component, args.numeric_only, ringbuffer, api_sender);
    if live_terminal_output_enabled || args.file.is_some() {
        outputs.set_writer(writer);
    }

    // Get and discard the first data
    if !args.calibrate_cpu_idle_baseline {
        monitor.loop_init();
    }

    if let Some(pid) = args.pid {
        // Perform one poll to initialize trackers and discard the first reading (delta calculation)
        monitor.poll(Some(pid), None, args.component.as_ref());
    }

    if let Some(ref app_name) = args.app {
        monitor.poll(None, Some(app_name), args.component.as_ref());
    }

    thread::sleep(time::Duration::from_secs(1));

    while running.load(Ordering::SeqCst) {
        let app_name_str = args.app.as_deref();
        let sample = monitor.poll(args.pid, app_name_str, args.component.as_ref());

        if let Err(e) = outputs.send(&sample) {
            logging::print_error(&format!("Output Error: {}", e));
            break;
        }

        thread::sleep(time::Duration::from_secs(1));
    }

    if live_terminal_output_enabled {
        logging::show_cursor();
    }
}
