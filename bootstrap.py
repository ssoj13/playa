#!/usr/bin/env python3
"""
bootstrap.py - Unified build/test/package script for Playa.

Cross-platform (Python 3, stdlib only). Replaces obsolete bootstrap.ps1 / .sh.

Commands:
    build         Build playa via `cargo xtask` (default: release profile)
    test          Run all tests via xtask
    check         Run clippy and fmt check
    clean         Clean build artifacts
    python        Build Python wheel via maturin
    python-reqs   Install Python dev dependencies
    publish       Publish crate to crates.io
    install       Install playa from crates.io (checks FFmpeg deps)
    package       Package for distribution via cargo-packager

Usage:
    python bootstrap.py build
    python bootstrap.py build -d
    python bootstrap.py test
    python bootstrap.py python --install
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

# ============================================================
# CONSTANTS & CONFIG
# ============================================================

ROOT_DIR = Path(__file__).parent.resolve()
IS_WINDOWS = platform.system() == "Windows"
IS_MACOS = platform.system() == "Darwin"
IS_LINUX = platform.system() == "Linux"

DEFAULT_VCPKG_ROOT = Path("C:/vcpkg") if IS_WINDOWS else Path.home() / "vcpkg"
DEFAULT_TRIPLET = "x64-windows-static-md-release" if IS_WINDOWS else ""

# Cargo tools that need to be installed
CARGO_TOOLS = [
    ("cargo-binstall", "cargo binstall --version", "cargo install cargo-binstall"),
    ("cargo-release", "cargo release --version", "cargo binstall cargo-release --no-confirm"),
    ("cargo-packager", "cargo packager --version", "cargo binstall cargo-packager --version 0.11.7 --no-confirm"),
]


# ANSI colors
class C:
    """ANSI color codes."""

    RST = "\033[0m"
    RED = "\033[91m"
    GRN = "\033[92m"
    YLW = "\033[93m"
    CYN = "\033[96m"
    WHT = "\033[97m"
    DIM = "\033[90m"

    @classmethod
    def init(cls) -> None:
        """Enable ANSI on Windows."""
        if IS_WINDOWS:
            os.system("")


# ============================================================
# UTILITY FUNCTIONS
# ============================================================

def fmt_time(ms: float) -> str:
    """Format milliseconds nicely."""
    if ms < 1000:
        return f"{ms:.0f}ms"
    elif ms < 60000:
        return f"{ms/1000:.1f}s"
    else:
        mins = int(ms // 60000)
        secs = (ms % 60000) / 1000
        return f"{mins}m{secs:.0f}s"


def header(text: str) -> None:
    """Print section header."""
    line = "=" * 60
    print(f"\n{C.CYN}{line}\n{text}\n{line}{C.RST}")


def step(text: str) -> None:
    """Print step indicator."""
    print(f"  {C.WHT}{text}{C.RST}")


def ok(text: str) -> None:
    """Print success message."""
    print(f"  {C.GRN}[OK] {text}{C.RST}")


def err(text: str) -> None:
    """Print error message."""
    print(f"  {C.RED}[ERR] {text}{C.RST}")


def warn(text: str) -> None:
    """Print warning message."""
    print(f"  {C.YLW}[WARN] {text}{C.RST}")


def run(args: list[str], cwd: Path | None = None,
        capture: bool = False, env: dict | None = None) -> tuple[int, str, float]:
    """Run command and return (exit_code, output, time_ms)."""
    start = time.perf_counter()
    result = subprocess.run(
        args,
        cwd=cwd or ROOT_DIR,
        capture_output=capture,
        text=True,
        env=env,
    )
    elapsed_ms = (time.perf_counter() - start) * 1000
    output = (result.stdout or "") + (result.stderr or "") if capture else ""
    return result.returncode, output, elapsed_ms


def which(cmd: str) -> Path | None:
    """Find executable in PATH."""
    result = shutil.which(cmd)
    return Path(result) if result else None


def cmd_exists(check_cmd: str) -> bool:
    """Check if a shell command succeeds (for version checks)."""
    try:
        subprocess.run(
            check_cmd.split(),
            capture_output=True,
            timeout=10,
        )
        return True
    except (subprocess.SubprocessError, FileNotFoundError):
        return False


# ============================================================
# ENVIRONMENT SETUP
# ============================================================

def setup_vcpkg() -> None:
    """Configure vcpkg environment variables."""
    vcpkg_root = os.environ.get("VCPKG_ROOT", "")
    if not vcpkg_root:
        if DEFAULT_VCPKG_ROOT.exists():
            os.environ["VCPKG_ROOT"] = str(DEFAULT_VCPKG_ROOT)
            vcpkg_root = str(DEFAULT_VCPKG_ROOT)

    if vcpkg_root and not os.environ.get("VCPKGRS_TRIPLET"):
        os.environ["VCPKGRS_TRIPLET"] = DEFAULT_TRIPLET

    triplet = os.environ.get("VCPKGRS_TRIPLET", "")
    if vcpkg_root and triplet:
        os.environ["PKG_CONFIG_PATH"] = str(
            Path(vcpkg_root) / "installed" / triplet / "lib" / "pkgconfig"
        )

    if vcpkg_root:
        ok(f"vcpkg: {vcpkg_root}")
        ok(f"triplet: {triplet}")
    else:
        warn("vcpkg not found (set VCPKG_ROOT or install to C:\\vcpkg)")


def setup_vs_env() -> None:
    """Setup Visual Studio build environment (Windows only)."""
    if not IS_WINDOWS:
        return

    step("Setting up build environment...")

    # Try vcv-rs first (fast)
    if which("vcv-rs"):
        try:
            code, output, _ = run(["vcv-rs", "-q", "-f", "json"], capture=True)
            if code == 0:
                data = json.loads(output)
                for key, val in data.items():
                    if isinstance(val, list):
                        # Append existing env value for list vars (PATH, INCLUDE, LIB, etc.)
                        existing = os.environ.get(key, "")
                        merged = ";".join(val)
                        if existing:
                            merged += ";" + existing
                        os.environ[key] = merged
                    else:
                        os.environ[key] = str(val)
                ok("Visual Studio environment (vcv-rs)")
                return
        except Exception:
            pass

    # Fallback: vcvars64.bat
    vswhere = Path(os.environ.get("ProgramFiles(x86)", "")) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if vswhere.exists():
        result = subprocess.run(
            [str(vswhere), "-latest", "-property", "installationPath"],
            capture_output=True, text=True,
        )
        install_path = result.stdout.strip()
        if install_path:
            vcvars = Path(install_path) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
            if vcvars.exists():
                code, output, _ = run(
                    ["cmd", "/c", f'"{vcvars}" && set'],
                    capture=True,
                )
                if code == 0:
                    for line in output.splitlines():
                        m = re.match(r"^([^=]+)=(.*)$", line)
                        if m:
                            os.environ[m.group(1)] = m.group(2)
                    ok("Visual Studio environment (vcvars64.bat)")
                    return

    warn("Visual Studio environment not configured")


def fix_libclang() -> None:
    """Clear LIBCLANG_PATH if it points to non-MSVC clang (e.g. ESP32 Xtensa)."""
    lcp = os.environ.get("LIBCLANG_PATH", "")
    if lcp and re.search(r"esp|xtensa", lcp, re.IGNORECASE):
        warn(f"Clearing LIBCLANG_PATH (ESP32 clang incompatible with MSVC)")
        del os.environ["LIBCLANG_PATH"]


def setup_env() -> None:
    """Full environment setup."""
    setup_vcpkg()
    setup_vs_env()
    fix_libclang()
    print()


# ============================================================
# DEPENDENCY CHECKS
# ============================================================

def check_cargo() -> bool:
    """Check if cargo is installed."""
    if not which("cargo"):
        err("Rust/Cargo not found!")
        step("Install from: https://rustup.rs/")
        return False
    return True


def ensure_cargo_tools() -> bool:
    """Install required cargo tools if missing."""
    step("Checking dependencies...")
    print()

    for i, (name, check_cmd, install_cmd) in enumerate(CARGO_TOOLS, 1):
        if cmd_exists(check_cmd):
            ok(f"[{i}/{len(CARGO_TOOLS)}] {name}")
        else:
            step(f"[{i}/{len(CARGO_TOOLS)}] Installing {name}...")
            code, _, _ = run(install_cmd.split())
            if code != 0:
                # Fallback to cargo install
                fallback = f"cargo install {name}"
                code, _, _ = run(fallback.split())
                if code != 0:
                    err(f"Failed to install {name}")
                    return False
            ok(f"{name} installed")

    print()
    ok("Dependencies ready!")
    print()
    return True


def ensure_xtask() -> bool:
    """Build xtask if not already built."""
    xtask = ROOT_DIR / "target" / "debug" / ("xtask.exe" if IS_WINDOWS else "xtask")
    if not xtask.exists():
        step("Building xtask...")
        code, _, _ = run(["cargo", "build", "-p", "xtask"])
        if code != 0:
            err("Failed to build xtask")
            return False
        ok("xtask built")
        print()
    return True


# ============================================================
# BUILD COMMAND
# ============================================================

def run_build(args: argparse.Namespace) -> int:
    """Build playa via xtask.

    Always delegates to ``cargo xtask build`` so the MSVC + vcpkg environment
    is set up by ``xtask::env_setup`` (single source of truth — no duplicate
    env-detection logic in this script).
    """
    header("BUILD")

    cmd = ["cargo", "xtask", "build"]
    if args.debug:
        cmd.append("--debug")
        step("Mode: debug")
    else:
        cmd.append("--release")
        step("Mode: release")
    if args.features:
        cmd.extend(["--features", args.features])
        step(f"Features: {args.features}")

    print()
    step("Building...")

    code, _, elapsed = run(cmd)

    print()
    if code == 0:
        ok(f"Build successful ({fmt_time(elapsed)})")
    else:
        err("Build failed")
    print()
    return code


# ============================================================
# TEST COMMAND
# ============================================================

def run_test(args: argparse.Namespace) -> int:
    """Run tests via xtask."""
    header("TEST")

    cmd = ["cargo", "xtask", "test"]

    # xtask `test`: release profile is the default; `--debug` selects debug for `cargo test`.
    if args.debug:
        cmd.append("--debug")

    if args.nocapture:
        cmd.append("--nocapture")

    print()
    step("Running tests...")
    print()

    code, _, elapsed = run(cmd)

    print()
    if code == 0:
        ok(f"All tests passed ({fmt_time(elapsed)})")
    else:
        err("Some tests failed")
    print()
    return code


# ============================================================
# CHECK COMMAND
# ============================================================

def run_check(args: argparse.Namespace) -> int:
    """Run clippy and fmt check."""
    header("CHECK")

    passed = True

    step("Running clippy...")
    code, _, elapsed = run(["cargo", "clippy", "--", "-D", "warnings"])
    if code == 0:
        ok(f"Clippy OK ({fmt_time(elapsed)})")
    else:
        err("Clippy found issues")
        passed = False

    print()

    step("Checking formatting...")
    code, _, elapsed = run(["cargo", "fmt", "--check"])
    if code == 0:
        ok(f"Format OK ({fmt_time(elapsed)})")
    else:
        warn("Format issues found. Run: cargo fmt")
        passed = False

    print()
    if passed:
        ok("All checks passed!")
    else:
        err("Some checks failed")
    print()
    return 0 if passed else 1


# ============================================================
# CLEAN COMMAND
# ============================================================

def run_clean(args: argparse.Namespace) -> int:
    """Clean build artifacts."""
    header("CLEAN")

    step("Running cargo clean...")
    code, _, _ = run(["cargo", "clean"])

    # Clean Python artifacts
    py_crate = ROOT_DIR / "crates" / "playa-py"
    if py_crate.exists():
        for pattern in ["*.so", "*.pyd", "*.egg-info"]:
            for f in py_crate.glob(pattern):
                step(f"Removing {f.name}")
                if f.is_dir():
                    shutil.rmtree(f)
                else:
                    f.unlink()

    print()
    ok("Clean complete")
    print()
    return code


# ============================================================
# PYTHON BUILD
# ============================================================

VENV_DIR = ROOT_DIR / ".venv"


def get_venv_paths() -> tuple[Path, Path, Path]:
    """Get paths for venv python, pip, and maturin."""
    if IS_WINDOWS:
        bin_dir = VENV_DIR / "Scripts"
        return bin_dir / "python.exe", bin_dir / "pip.exe", bin_dir / "maturin.exe"
    else:
        bin_dir = VENV_DIR / "bin"
        return bin_dir / "python", bin_dir / "pip", bin_dir / "maturin"


def ensure_venv() -> tuple[Path, Path, Path]:
    """Ensure virtualenv exists."""
    python, pip, maturin = get_venv_paths()
    if not VENV_DIR.exists():
        step("Creating virtualenv (.venv)...")
        result = subprocess.run([sys.executable, "-m", "venv", str(VENV_DIR)])
        if result.returncode != 0:
            raise RuntimeError("Failed to create virtualenv")
        ok("Virtualenv created")
    return python, pip, maturin


def ensure_maturin(pip: Path, maturin: Path) -> None:
    """Ensure maturin is installed in venv."""
    if not maturin.exists():
        step("Installing maturin...")
        result = subprocess.run([str(pip), "install", "maturin"])
        if result.returncode != 0:
            raise RuntimeError("Failed to install maturin")
        ok("maturin installed")


def run_python_reqs(args: argparse.Namespace) -> int:
    """Install Python dev dependencies."""
    header("PYTHON DEPENDENCIES")
    print()

    try:
        _, pip, _ = ensure_venv()
    except RuntimeError as e:
        err(str(e))
        return 1

    packages = ["maturin", "pytest"]

    step("Installing packages...")
    result = subprocess.run([str(pip), "install"] + packages)

    print()
    if result.returncode == 0:
        ok("Done!")
        step(f"Virtualenv: {VENV_DIR}")
    else:
        err("Failed to install dependencies")
        return 1
    print()
    return 0


def run_python_build(args: argparse.Namespace) -> int:
    """Build Python wheel via maturin."""
    header("PYTHON BUILD")
    print()

    py_crate = ROOT_DIR / "crates" / "playa-py"
    manifest = py_crate / "Cargo.toml"
    if not manifest.exists():
        err("playa-py crate not found")
        return 1

    try:
        python, pip, maturin = ensure_venv()
        ensure_maturin(pip, maturin)
    except RuntimeError as e:
        err(str(e))
        return 1

    build_type = "debug" if args.debug else "release"
    step(f"Mode: {build_type}")
    step(f"Install: {args.install}")
    print()

    start = time.perf_counter()

    # Environment with venv
    env = os.environ.copy()
    env["VIRTUAL_ENV"] = str(VENV_DIR)
    bin_dir = "Scripts" if IS_WINDOWS else "bin"
    env["PATH"] = str(VENV_DIR / bin_dir) + os.pathsep + env.get("PATH", "")

    if args.install:
        cmd = [str(maturin), "develop", "--manifest-path", str(manifest)]
        if not args.debug:
            cmd.append("--release")
        msg = f"Building and installing ({build_type})..."
    else:
        cmd = [str(maturin), "build", "--manifest-path", str(manifest)]
        if not args.debug:
            cmd.append("--release")
        msg = f"Building wheel ({build_type})..."

    step(msg)
    result = subprocess.run(cmd, env=env)

    elapsed_ms = (time.perf_counter() - start) * 1000

    print()
    if result.returncode == 0:
        ok(f"Done! ({fmt_time(elapsed_ms)})")
        if args.install:
            step(f"Installed to: {VENV_DIR}")
        print()
        step("Usage:")
        step("  import playa")
        step("  playa.run(file='path/to/image.exr', autoplay=True)")
    else:
        err("Build failed")
        return 1
    print()
    return 0


# ============================================================
# INSTALL COMMAND
# ============================================================

def run_install(args: argparse.Namespace) -> int:
    """Install playa from crates.io with FFmpeg dependency check."""
    header("INSTALL")

    vcpkg_root = Path(os.environ.get("VCPKG_ROOT", str(DEFAULT_VCPKG_ROOT)))
    triplet = os.environ.get("VCPKGRS_TRIPLET", DEFAULT_TRIPLET)

    # Check vcpkg
    if not vcpkg_root.exists():
        err(f"vcpkg not found at {vcpkg_root}")
        step("Install vcpkg: git clone https://github.com/microsoft/vcpkg.git C:\\vcpkg")
        return 1
    ok("vcpkg found")

    # Check FFmpeg
    ffmpeg_pc = vcpkg_root / "installed" / triplet / "lib" / "pkgconfig" / "libavutil.pc"
    if not ffmpeg_pc.exists():
        err("FFmpeg not found in vcpkg")
        step(f"Install: vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:{triplet}")
        return 1
    ok("FFmpeg found")

    # Check pkg-config
    if not which("pkg-config"):
        err("pkg-config not found")
        step(f"Install: vcpkg install pkgconf:{triplet}")
        return 1
    ok("pkg-config found")

    print()
    step("Installing playa from crates.io...")
    print()
    code, _, elapsed = run(["cargo", "install", "playa"])

    if code == 0:
        ok(f"Installed ({fmt_time(elapsed)})")
    else:
        err("Install failed")
    print()
    return code


# ============================================================
# PUBLISH / PACKAGE
# ============================================================

def run_publish(args: argparse.Namespace) -> int:
    """Publish crate to crates.io."""
    header("PUBLISH")
    step("Publishing crate to crates.io...")
    print()
    code, _, _ = run(["cargo", "publish"])
    return code


def run_package(args: argparse.Namespace) -> int:
    """Package for distribution."""
    header("PACKAGE")
    step("Packaging for distribution...")
    print()
    code, _, elapsed = run(["cargo", "packager", "--release"])
    if code == 0:
        ok(f"Package complete ({fmt_time(elapsed)})")
    else:
        err("Packaging failed")
    print()
    return code


# ============================================================
# XTASK PASSTHROUGH
# ============================================================

def run_xtask(extra_args: list[str]) -> int:
    """Pass arguments directly to cargo xtask."""
    cmd = ["cargo", "xtask"] + extra_args
    code, _, _ = run(cmd)
    return code


# ============================================================
# HELP
# ============================================================

HELP_TEXT = f"""
 PLAYA BUILD SYSTEM

 COMMANDS
   build           Build playa (via xtask)
   test            Run all tests (via xtask)
   check           Run clippy and fmt check
   clean           Clean build artifacts
   python          Build Python wheel via maturin
   python-reqs     Install Python dev dependencies
   install         Install playa from crates.io (checks FFmpeg deps)
   publish         Publish crate to crates.io
   package         Package for distribution

 BUILD OPTIONS
   -d, --debug     Debug mode (default: release)
   -f, --features  Cargo features to enable

 TEST OPTIONS
   -n, --nocapture Show test output
   -d, --debug     Test debug build (default: release)

 PYTHON OPTIONS
   -i, --install   Build and install into .venv
   -d, --debug     Build debug instead of release

 XTASK COMMANDS (forwarded to cargo xtask — see crates/xtask)
   changelog       Regenerate CHANGELOG.md from git
   deploy          Copy playa binary to install prefix (--install-dir)
   tag-dev         Create dev tag (triggers Build workflow)
   tag-rel         Create release tag (triggers Release workflow)
   pr              Create PR: dev -> main
   wipe            Remove stale binaries from target/
   wipe-wf         Delete GitHub Actions workflow runs (needs gh CLI)

 EXAMPLES
   python bootstrap.py build                     # Release build (default)
   python bootstrap.py build -d                 # Debug build
   python bootstrap.py build -f profiler        # Extra Cargo features
   python bootstrap.py test                       # Run tests
   python bootstrap.py check                      # Clippy + fmt
   python bootstrap.py python --install           # Build & install Python wheel in .venv
   python bootstrap.py install                    # Install from crates.io
"""


# ============================================================
# MAIN
# ============================================================

COMMANDS = [
    "build", "test", "check", "clean",
    "python", "python-reqs",
    "install", "publish", "package",
    "help",
]

# Xtask-forwarded commands (no special handling needed)
XTASK_COMMANDS = [
    # Forwarded to `cargo xtask`; do NOT list `build` / `test` here — those are
    # handled by `bootstrap.py` so release/debug and extra flags work.
    "changelog",
    "tag-dev",
    "tag-rel",
    "pr",
    "deploy",
    "wipe",
    "wipe-wf",
]


def main() -> int:
    C.init()

    # Check for xtask-forwarded commands before argparse
    if len(sys.argv) > 1 and sys.argv[1] in XTASK_COMMANDS:
        if not check_cargo():
            return 1
        setup_env()
        if not ensure_xtask():
            return 1
        return run_xtask(sys.argv[1:])

    parser = argparse.ArgumentParser(
        description="Playa build system",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )

    parser.add_argument(
        "command",
        nargs="?",
        choices=COMMANDS,
        default="help",
        help="Command to run",
    )

    # Build options
    parser.add_argument("-d", "--debug", action="store_true", help="Debug mode (default: release)")
    parser.add_argument("-f", "--features", help="Cargo features to enable")

    # Test options
    parser.add_argument("-n", "--nocapture", action="store_true", help="Show test output")

    # Python options
    parser.add_argument("-i", "--install", action="store_true", help="Install into .venv")

    args = parser.parse_args()

    # Help needs no setup
    if args.command == "help" or args.command is None:
        print(HELP_TEXT)
        return 0

    # Environment setup for real commands
    if not check_cargo():
        return 1
    setup_env()

    # Ensure tools + xtask for build/test/package commands
    if args.command in ("build", "test", "package", "publish"):
        if not ensure_cargo_tools():
            return 1
        if not ensure_xtask():
            return 1

    # Dispatch
    dispatch = {
        "build": run_build,
        "test": run_test,
        "check": run_check,
        "clean": run_clean,
        "python": run_python_build,
        "python-reqs": run_python_reqs,
        "install": run_install,
        "publish": run_publish,
        "package": run_package,
    }

    handler = dispatch.get(args.command or "help")
    if handler:
        return handler(args)

    print(HELP_TEXT)
    return 0


if __name__ == "__main__":
    sys.exit(main())
