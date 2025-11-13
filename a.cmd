@echo off
setlocal enabledelayedexpansion

:: Set FFmpeg/vcpkg environment variables for this script session
if exist "C:\vcpkg" (
    set "VCPKG_ROOT=C:\vcpkg"
    set "VCPKGRS_TRIPLET=x64-windows-static-md-release"
    set "PKG_CONFIG_PATH=C:\vcpkg\installed\x64-windows-static-md-release\lib\pkgconfig"
)

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
            echo ? Visual Studio environment configured
        )
    )
)

:: Verify vcpkg configuration
if defined VCPKG_ROOT (
    echo ? vcpkg configured: %VCPKG_ROOT%
    echo ? triplet: %VCPKGRS_TRIPLET%
)
echo.

::cargo build --release --features openexr
cargo xtask build --openexr --release