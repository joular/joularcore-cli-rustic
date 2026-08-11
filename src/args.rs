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

//! Command line arguments, and the one place they become library configuration.
//!
//! The library used to own this parser. It no longer depends on `clap`, so the
//! enums it exposes carry no `ValueEnum` derive and are wrapped here.

use clap::{Parser, ValueEnum};
use joularcore::{AppMatch, Component, ElevationPolicy, MonitorConfig, Target};
use std::time::Duration;

#[derive(Parser)]
#[command(author, version, name = "Joular Core")]
pub struct Args {
    /// Monitor a specific process by its PID
    #[arg(short = 'p', long = "pid", conflicts_with = "app")]
    pub pid: Option<u32>,

    /// Monitor a specific application by its name (all PIDs)
    #[arg(short = 'a', long = "app")]
    pub app: Option<String>,

    /// Write output to CSV file
    #[arg(short = 'f', long = "file")]
    pub file: Option<String>,

    /// Monitor only specific component (cpu or gpu)
    #[arg(short = 'c', long = "component", value_enum)]
    pub component: Option<ComponentArg>,

    /// Output only numeric value (no formatting)
    #[arg(short = 'i', long = "numeric")]
    pub numeric_only: bool,

    /// Overwrite file instead of append (only with -f)
    #[arg(short = 'o', long = "overwrite", requires = "file")]
    pub overwrite: bool,

    /// Send power data to ring buffer
    #[arg(short = 'r', long = "ringbuffer")]
    pub ringbuffer: bool,

    /// Disable the live terminal power line while keeping other outputs active
    #[arg(short = 's', long = "silent")]
    pub silent: bool,

    /// Show the library's log records (repeat for more detail)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// How --app matches process names
    #[arg(long = "app-match", value_enum, default_value_t = AppMatchArg::Exact)]
    pub app_match: AppMatchArg,

    /// How far to go to obtain privileged sensor access (macOS powermetrics)
    #[arg(long = "elevation", value_enum, default_value_t = ElevationArg::Never)]
    pub elevation: ElevationArg,

    /// CPU idle baseline in Watts to subtract before attributing PID/app power
    #[arg(
        long = "cpu-idle-baseline",
        value_name = "WATTS",
        default_value_t = 0.0,
        conflicts_with = "calibrate_cpu_idle_baseline"
    )]
    pub cpu_idle_baseline: f64,

    /// Refresh interval in seconds for application PID sweeping (0 to disable caching)
    #[arg(
        long = "app-refresh-interval",
        value_name = "SECONDS",
        default_value_t = 3
    )]
    pub app_refresh_interval: u64,

    /// Calibrate the CPU idle baseline from a short idle measurement window
    #[arg(long = "calibrate-cpu-idle-baseline")]
    pub calibrate_cpu_idle_baseline: bool,
}

impl Args {
    /// What these arguments ask the library to measure.
    pub fn config(&self) -> MonitorConfig {
        MonitorConfig {
            target: match (self.pid, &self.app) {
                (Some(pid), _) => Target::Pid(pid),
                (_, Some(app)) => Target::app(app),
                _ => Target::System,
            },
            component: self.component.map(Into::into),
            app_match: self.app_match.into(),
            app_refresh_interval: Duration::from_secs(self.app_refresh_interval),
            // `--cpu-idle-baseline` defaults to 0.0, which has always meant "do
            // not subtract a baseline". The library spells that `None`.
            cpu_idle_baseline: (self.cpu_idle_baseline > 0.0).then_some(self.cpu_idle_baseline),
            elevation: self.elevation.into(),
        }
    }

    /// The log level `-v` asks for. `RUST_LOG` overrides it.
    pub fn log_level(&self) -> log::LevelFilter {
        match self.verbose {
            0 => log::LevelFilter::Warn,
            1 => log::LevelFilter::Info,
            2 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        }
    }
}

/// `--component`, mirroring [`Component`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ComponentArg {
    Cpu,
    Gpu,
}

impl From<ComponentArg> for Component {
    fn from(arg: ComponentArg) -> Self {
        match arg {
            ComponentArg::Cpu => Component::Cpu,
            ComponentArg::Gpu => Component::Gpu,
        }
    }
}

/// `--app-match`, mirroring [`AppMatch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AppMatchArg {
    /// The executable name must match exactly, ignoring case and a trailing `.exe`.
    Exact,
    /// The executable name must contain the given string, ignoring case.
    Contains,
}

impl From<AppMatchArg> for AppMatch {
    fn from(arg: AppMatchArg) -> Self {
        match arg {
            AppMatchArg::Exact => AppMatch::Exact,
            AppMatchArg::Contains => AppMatch::Contains,
        }
    }
}

/// `--elevation`, mirroring [`ElevationPolicy`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ElevationArg {
    /// Never elevate. A sensor needing root reports itself unavailable.
    Never,
    /// Use `sudo -n`, which works only if a sudo credential is already cached.
    Sudo,
}

impl From<ElevationArg> for ElevationPolicy {
    fn from(arg: ElevationArg) -> Self {
        match arg {
            ElevationArg::Never => ElevationPolicy::Never,
            ElevationArg::Sudo => ElevationPolicy::SudoNonInteractive,
        }
    }
}

/// Whether `process_name` denotes the application `app_name`.
///
/// The library decides this itself while measuring, but keeps its version
/// private, so the startup check that reports "application not found" has to
/// mirror it here. The two must agree: a check that is more permissive than the
/// measurement lets a session start that will only ever attribute 0 W.
pub fn matches_app_name(process_name: &str, app_name: &str, app_match: AppMatchArg) -> bool {
    fn strip_exe(name: &str) -> &str {
        name.strip_suffix(".exe").unwrap_or(name)
    }

    let process_name = process_name.to_lowercase();
    let app_name = app_name.to_lowercase();

    match app_match {
        AppMatchArg::Exact => strip_exe(&process_name) == strip_exe(&app_name),
        AppMatchArg::Contains => process_name.contains(&app_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_follows_the_library_rules() {
        // Pinned against joularcore's own tests, in src/platform/cpu_usage.rs.
        assert!(matches_app_name("Firefox.exe", "firefox", AppMatchArg::Exact));
        assert!(matches_app_name("firefox", "Firefox.exe", AppMatchArg::Exact));
        assert!(!matches_app_name("firefox-bin", "firefox", AppMatchArg::Exact));
        assert!(!matches_app_name("codesign", "code", AppMatchArg::Exact));

        assert!(matches_app_name("firefox-bin", "firefox", AppMatchArg::Contains));
        assert!(matches_app_name("codesign", "code", AppMatchArg::Contains));
        assert!(!matches_app_name("bash", "firefox", AppMatchArg::Contains));
    }

    #[test]
    fn a_zero_baseline_means_no_baseline() {
        let args = Args::parse_from(["joularcore"]);
        assert_eq!(args.config().cpu_idle_baseline, None);

        let args = Args::parse_from(["joularcore", "--cpu-idle-baseline", "3.5"]);
        assert_eq!(args.config().cpu_idle_baseline, Some(3.5));
    }

    #[test]
    fn the_target_comes_from_pid_then_app_then_the_whole_system() {
        assert_eq!(Args::parse_from(["joularcore"]).config().target, Target::System);
        assert_eq!(
            Args::parse_from(["joularcore", "-p", "42"]).config().target,
            Target::Pid(42)
        );
        assert_eq!(
            Args::parse_from(["joularcore", "-a", "firefox"]).config().target,
            Target::app("firefox")
        );
    }
}
