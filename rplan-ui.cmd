@echo off
cd /d "%~dp0"
cargo run --release -q -- ui %1
