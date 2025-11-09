@echo off
setlocal enabledelayedexpansion
REM Run encoding integration test
REM Usage: test.cmd
REM
REM Note: This runs tests in release mode using already compiled binaries.
REM If you need to rebuild with FFmpeg changes, run: cargo clean
echo.

setlocal enabledelayedexpansion

:: Check if cargo is installed
where cargo >nul 2>&1
if errorlevel 1 (
    echo Error: Rust/Cargo not found!
    echo.
    echo Please install Rust from: https://rustup.rs/
    exit /b 1
)

:: Setup Visual Studio environment
echo Setting up build environment...
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSWHERE%" (
    for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -property installationPath`) do (
        set "VCVARS=%%i\VC\Auxiliary\Build\vcvars64.bat"
        if exist "!VCVARS!" (
            call "!VCVARS!" >nul 2>&1
            echo ✓ Visual Studio environment configured
        )
    )
)

:: Setup vcpkg
if exist "C:\vcpkg" (
    set "VCPKG_ROOT=C:\vcpkg"
    set "PKG_CONFIG_PATH=C:\vcpkg\installed\x64-windows-static-md\lib\pkgconfig"
    echo ✓ vcpkg configured
)
echo.



echo Running encoding integration test...
echo.
cargo test --release test_encode_placeholder_frames -- --nocapture
echo.
echo Test completed.
pause
