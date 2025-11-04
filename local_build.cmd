@echo off
:: Build script using cargo xtask
:: This automatically handles header patching and dependency copying

:: Check if xtask is available
cargo xtask --help >nul 2>&1
if errorlevel 1 (
    echo Error: cargo xtask is not available.
    echo The xtask crate may not be built yet.
    echo.
    echo Solution: Run 'cargo build -p xtask' first to build the xtask tool.
    exit /b 1
)

cargo xtask build --release
