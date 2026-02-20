# Perfectly Balanced

A Rust-powered Unraid plugin that balances disk usage across array drives using parallel filesystem scanning, a greedy bin-packing algorithm, and rsync-based file transfers with real-time progress.

## Features

- **Parallel filesystem scanning** via jwalk for fast catalog building across multiple disks
- **Greedy largest-first balancing algorithm** with a configurable tolerance slider (fewest moves ↔ perfect balance)
- **rsync-based file transfers** with `--remove-source-files` for atomic moves
- **Real-time progress** via Server-Sent Events streamed to the Unraid WebGUI
- **Safety first**: hard rejection of `/mnt/user/` FUSE paths to prevent data corruption
- **Open file detection** via `lsof` before each move
- **Parity check awareness** — warns if a parity check is running
- **SQLite catalog** stored on the USB flash for persistence across reboots

## Architecture

```
Browser (Unraid WebGUI)
  ├── PHP .page file loads in Settings menu
  ├── REST API calls to Rust daemon
  └── SSE connection for real-time progress
        │
        │ HTTP on 127.0.0.1:7091
        ▼
Rust Daemon (static musl binary)
  ├── axum HTTP server
  ├── Scanner (jwalk parallel walk)
  ├── Balancer (greedy largest-first)
  ├── Executor (rsync subprocess)
  └── SQLite catalog (rusqlite bundled)
```

## Development

### Prerequisites

- Rust toolchain (stable)
- [`cross`](https://github.com/cross-rs/cross) for cross-compilation to musl (optional, for packaging)

### Local development

```bash
# Run locally with test configuration
make run

# Run tests
make test

# Format and lint
make fmt
make lint
```

### Building for Unraid

```bash
# Build static musl binary
make build

# Create .txz Slackware package
make package VERSION=0.1.0
```

### Project structure

```
src/
├── main.rs          # Daemon entry point, graceful shutdown
├── api/             # REST API routes, handlers, SSE endpoint
├── balancer/        # Greedy largest-first balancing algorithm
├── config/          # Configuration loading (INI + env vars)
├── db/              # SQLite database, models, queries
├── events/          # Broadcast event hub for SSE
├── executor/        # rsync process spawning, progress parsing
└── scanner/         # jwalk parallel filesystem scanner

plugin/
├── pages/           # Unraid .page files (PHP + JS UI)
├── scripts/         # Daemon start/stop scripts
├── event/           # Array start/stop hooks
└── pkg/             # .plg manifest for Unraid plugin system

migrations/
└── 001_initial.sql  # SQLite schema
```

## API

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/status` | Daemon status |
| `GET` | `/api/disks` | List all array disks |
| `POST` | `/api/scan` | Start filesystem scan |
| `POST` | `/api/plan` | Generate balance plan |
| `GET` | `/api/plan/:id` | Get plan details |
| `POST` | `/api/plan/:id/execute` | Execute a plan |
| `POST` | `/api/plan/:id/cancel` | Cancel execution |
| `GET` | `/api/settings` | Read settings |
| `POST` | `/api/settings` | Update settings |
| `GET` | `/api/events` | SSE event stream |

## Configuration

Settings are stored in `/boot/config/plugins/perfectly-balanced/perfectly-balanced.cfg`:

```ini
PORT="7091"
SCAN_THREADS="2"
SLIDER_ALPHA="0.5"
MAX_TOLERANCE="0.15"
MIN_FREE_HEADROOM="1073741824"
EXCLUDED_DISKS=""
WARN_PARITY_CHECK="yes"
```

Environment variable overrides: `PB_PORT`, `PB_DB_PATH`, `PB_CONFIG_PATH`, `PB_MNT_BASE`.

## Safety

- All paths are validated to reject `/mnt/user/` (Unraid FUSE layer)
- Only `/mnt/diskX/` and `/mnt/cache/` paths are permitted
- Open files are detected via `lsof` before each move
- Parity check detection prevents moves during rebuilds
- rsync `--remove-source-files` ensures atomic moves
- Daemon binds to `127.0.0.1` only (network-unreachable)

## License

MIT
