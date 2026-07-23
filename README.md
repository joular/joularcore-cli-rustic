# <a href="https://www.noureddine.org/research/joular/"><img src="https://raw.githubusercontent.com/joular/.github/main/profile/joular.png" alt="Joular Project" width="64" /></a> Joular Core :zap:

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue)](https://www.gnu.org/licenses/gpl-3.0) ![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-2b2b2b?logo=rust&logoColor=white)

This is a command line program that uses [Joular Core](https://github.com/joular/joularcore) library to monitor energy on all platforms and operating systems.

---

## Screenshots

#### CLI — Linux, Windows, macOS, Raspberry Pi

<img src="img/joularcore-windows-cli.png" width="500">

<img src="img/joularcore-linux-cli.png" width="500">

<img src="img/joularcore-macos-cli.png" width="500">

<img src="img/joularcore-rpi3.png" width="500">

#### Process and application monitoring

<img src="img/joularcore-pid-monitoring.png" width="500">
    
<img src="img/joularcore-app-monitoring.png" width="500">

#### Numeric and CSV output modes

<img src="img/joularcore-windows-cli-numeric.png" width="500">

<img src="img/joularcore-windows-cli-csv.png" width="500">

---

## CLI Options

Joular Core CLI uses the Joular Core library and supports all of its CLI options.

Run `joularcore --help` for a complete list. The main options are:

| Option | Description |
|--------|-------------|
| `-p`, `--pid <PID>` | Monitor a specific process by PID |
| `-a`, `--app <APP>` | Monitor an application by name (covers all its processes) |
| `-f`, `--file <FILE>` | Write output to a file. CSV is used by default; with `-i`, the file receives numeric-only values. |
| `-o`, `--overwrite` | With `-f`, truncate before each write so only the latest data row/value is kept |
| `-c`, `--component <cpu\|gpu>` | Show only CPU or only GPU power |
| `-i`, `--numeric` | Output only the numeric value, no formatting or labels |
| `-s`, `--silent` | Suppress terminal output (file, ring buffer, and API still work) |
| `-g`, `--gui` | Start the graphical user interface |
| `-r`, `--ringbuffer` | Write power data to the shared-memory ring buffer |
| `--api-port <PORT>` | Start the HTTP and WebSocket API server on this port |
| `--api-allowed-origin <ORIGIN>` | Allow an extra CORS origin for the API (repeatable). Localhost is always allowed. |
| `--app-refresh-interval <SECONDS>` | How often to rescan for new PIDs belonging to an application (default: 3s; set to 0 to rescan every second) |
| `--cpu-idle-baseline <WATTS>` | Subtract a fixed idle CPU baseline before attributing power to a PID or application |
| `--calibrate-cpu-idle-baseline` | Measure idle CPU power automatically (5 samples, 1 second each) and use that as the baseline |

**Notes:**
- `--pid` and `--app` are mutually exclusive.
- `--cpu-idle-baseline` and `--calibrate-cpu-idle-baseline` are mutually exclusive.
- `-o` only has effect when used with `-f`.
- In the CLI, `-g` / `--gui` conflicts with `--pid`, `--app`, `--file`, `--overwrite`, `--silent`, and `--numeric`.
- When `-f` is used, the live terminal display is replaced by file output.

### Examples

```bash
# Monitor system power
joularcore

# Monitor a specific process
joularcore -p 1234

# Monitor an application by name, write to CSV
joularcore -a firefox -f power.csv

# Run silently, write to CSV, expose via API
joularcore -s -f power.csv --api-port 8080

# Only show CPU power, numeric output
joularcore -c cpu -i

# Launch the GUI (configure everything from inside the GUI)
joularcore -g

# Subtract idle CPU baseline when attributing process power
joularcore -p 1234 --calibrate-cpu-idle-baseline
```

---

## 🟢 Systemd service on Linux

A ready-to-use systemd unit file is included in the `systemd/` directory. It runs Joular Core CLI as a daemon that continuously overwrites `/tmp/joularcore-service.csv` with the latest power reading. Because overwrite mode truncates before each write, the default service file contains only the latest data row, without a CSV header.

```bash
sudo cp systemd/joularcore.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable joularcore
sudo systemctl start joularcore
```

To use a different output path or add options, edit the `ExecStart` line in the service file before copying it.

---

## 📜 License

Joular Core CLI is licensed under the GNU General Public License 3 license only (GPL-3.0-only).

Copyright © 2025-2026, Adel Noureddine.
All rights reserved. This program and the accompanying materials are made available under the terms of the [GNU General Public License v3.0 (GPL-3.0-only)](https://www.gnu.org/licenses/gpl-3.0.en.html) which accompanies this distribution.

Author: Prof. Adel Noureddine
