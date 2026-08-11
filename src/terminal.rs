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

//! The live terminal display, and the coloured status messages around it.
//!
//! The library writes files and shared memory; a terminal is this program's
//! business, so the renderer lives here as an ordinary [`OutputSink`].

use joularcore::{Component, MonitorSample, OutputSink, Schema, Target};
use std::fmt::Write as _;
use std::io::{Write, stdout};

/// Redraws one power line in place, once per sample.
pub struct TerminalWriter {
    component: Option<Component>,
    /// Which suffix the line carries. The sample reports one `target_power`
    /// whatever the target is, so the label has to come from the config.
    target: Target,
    /// In numeric mode the line is a bare number for another program to read.
    numeric: Option<Schema>,
    /// Reused between samples so the loop does not allocate per tick.
    scratch: String,
}

impl TerminalWriter {
    pub fn new(component: Option<Component>, target: Target, numeric_only: bool) -> Self {
        Self {
            component,
            target,
            // The bare-wattage schema is exactly the numeric line: the selected
            // component, or total power, at two decimals.
            numeric: numeric_only.then(|| Schema::watts(component)),
            scratch: String::with_capacity(256),
        }
    }

    fn render(&mut self, sample: &MonitorSample) {
        let buf = &mut self.scratch;
        buf.clear();

        // `\r\x1b[2K` returns to the start of the line and clears it, so each
        // sample overwrites the previous one instead of scrolling.
        match self.component {
            Some(Component::Cpu) => {
                let _ = write!(buf, "\r\x1b[2K\x1b[1;36mCPU {}\x1b[0m", watts(sample.cpu_power));
            }
            Some(Component::Gpu) => {
                let _ = write!(buf, "\r\x1b[2K\x1b[1;35mGPU {}\x1b[0m", watts(sample.gpu_power));
            }
            None => {
                let _ = write!(
                    buf,
                    "\r\x1b[2K\x1b[1;33m⚡ Total {:.2} W\x1b[0m | \
                     \x1b[1;36mCPU {}\x1b[0m | \
                     \x1b[1;35mGPU {}\x1b[0m | \
                     \x1b[1;36mCPU Usage {:.2}%\x1b[0m",
                    sample.total_power(),
                    watts(sample.cpu_power),
                    watts(sample.gpu_power),
                    sample.cpu_usage
                );

                let power = sample.target_power_or_zero();
                match self.target {
                    Target::System => {}
                    Target::Pid(_) => {
                        let _ = write!(buf, " | \x1b[1;32mPID {power:.2} W\x1b[0m");
                    }
                    Target::App(_) => {
                        let pids = sample.app_pid_count.unwrap_or(0);
                        let _ = write!(buf, " | \x1b[1;32mApp {power:.2} W ({pids} PIDs)\x1b[0m");
                    }
                }
            }
        }
    }
}

impl OutputSink for TerminalWriter {
    fn send(&mut self, sample: &MonitorSample) -> joularcore::Result<()> {
        let mut out = stdout();

        // Numeric output is parsed by other programs, so it stays a plain
        // number per line — including a 0.00 for a sensor that could not be
        // read, which is what every previous release wrote.
        if let Some(schema) = self.numeric {
            writeln!(out, "{}", schema.row(sample))?;
            out.flush()?;
            return Ok(());
        }

        self.render(sample);
        out.write_all(self.scratch.as_bytes())?;
        out.flush()?;
        Ok(())
    }
}

/// A power reading for the live line.
///
/// An unreadable sensor reports `None`, not zero. Printing it as `0.00` would
/// claim the machine is idle; `n/a` says the reading is missing, and `-v`
/// prints the library's warning explaining why.
fn watts(power: Option<f64>) -> String {
    match power {
        Some(watts) => format!("{watts:.2} W"),
        None => "n/a".to_string(),
    }
}

pub fn print_error(msg: &str) {
    eprintln!("\x1b[1;31m✗ {msg}\x1b[0m");
}

pub fn print_warning(msg: &str) {
    eprintln!("\x1b[1;33m⚠ {msg}\x1b[0m");
}

pub fn print_success(msg: &str) {
    println!("\x1b[1;32m✓ {msg}\x1b[0m");
}

/// Re-enable the terminal cursor. Errors (e.g. closed stdout during shutdown)
/// are ignored so callers can safely use this on cleanup paths.
pub fn show_cursor() {
    print!("\x1b[?25h");
    let _ = stdout().flush();
}

/// Hide the terminal cursor for live single-line updates.
pub fn hide_cursor() {
    print!("\x1b[?25l");
    let _ = stdout().flush();
}
