# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

High-performance web radio streaming server in Rust using Axum framework. Designed to handle thousands of simultaneous listeners efficiently through a shared buffer architecture.

## Architecture

**Core Pattern**: Single MP3 reader → Shared buffer → Multiple listeners
- `RadioStation` in `radio.rs`: Manages broadcast state and shared audio buffer
- Single track player reads MP3s sequentially and writes to shared buffer
- Each listener gets their own stream from the shared buffer (memory efficient)
- Recent migration from Rocket to Axum framework for better async performance

**Key Components**:
- `main.rs`: Axum server setup, route definitions, middleware
- `radio.rs`: Core broadcasting logic with shared buffer system
- `playlist.rs`: MP3 scanning, metadata extraction, playlist management
- `config.rs`: Environment-based configuration
- `error.rs`: Custom error types with HTTP status mapping

## Commands

### Development
```bash
# Run development server
cargo run

# Run with release optimizations
cargo run --release

# Build for production
cargo build --release

# Check code
cargo check

# Format code
cargo fmt

# Lint code
cargo clippy
```

### Testing & Utilities
```bash
# Test API endpoints (server must be running)
./test.sh

# Force rescan MP3 files and rebuild playlist
./rescan.sh
```

### Production
```bash
# Run production build
./target/release/webradio

# Service management
sudo systemctl start webradio
sudo systemctl status webradio
sudo systemctl restart webradio
sudo journalctl -u webradio -f  # View logs
```

## API Endpoints

- `/` - Web interface
- `/stream` - Audio stream (MP3)
- `/api/now-playing` - Current track (SSE)
- `/api/listeners` - Listener count (SSE)
- `/api/playlist` - Full playlist JSON
- `/api/stats` - Detailed statistics
- `/api/health` - Health check

## Configuration

Environment variables:
- `HOST` (default: "0.0.0.0")
- `PORT` (default: 8000)
- `MUSIC_DIR` (default: "music")

## Important Notes

- MP3 files go in `music/` directory
- Playlist cache stored in `music/playlist.json`
- Static files served from `static/`
- HTML templates in `templates/`
- Production deployment uses NGINX reverse proxy (config in README)
- System requires tuning for high concurrent connections (see README)