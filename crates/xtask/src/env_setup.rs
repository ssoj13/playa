//! Build-time environment bootstrap for FFmpeg / MSVC.
//!
//! - **Windows**: set `VCPKG_*`, then prepend MSVC `PATH`, `INCLUDE`, `LIB`, `LIBPATH` via [`vcv_rs`].
//! - **Linux / macOS**: set `VCPKG_ROOT` from common defaults (if absent), set
//!   `VCPKGRS_TRIPLET` from the platform default, and prepend `PKG_CONFIG_PATH`
//!   to vcpkg's pkgconfig dir so ffmpeg-sys-next picks up vcpkg-curated headers
//!   instead of system `/usr/include` (which can carry deprecated/incompatible
//!   declarations bindgen chokes on).

use anyhow::{Context, Result};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Default Windows triplet (release-only, dynamic CRT, static FFmpeg libs).
#[cfg(windows)]
const WIN_DEFAULT_TRIPLET: &str = "x64-windows-static-md-release";
/// Default Linux triplet (release-only, static FFmpeg).
#[cfg(target_os = "linux")]
const LINUX_DEFAULT_TRIPLET: &str = "x64-linux-release";
/// Default macOS triplet — Apple Silicon vs Intel.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const MAC_DEFAULT_TRIPLET: &str = "arm64-osx-release";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const MAC_DEFAULT_TRIPLET: &str = "x64-osx-release";

/// Platform-default vcpkg triplet — matches `crates/playa-ffmpeg/build.rs::get_vcpkg_triplet`
/// and the README install matrix. Returns `None` only on truly unsupported targets.
fn default_triplet() -> Option<&'static str> {
    #[cfg(windows)]
    { Some(WIN_DEFAULT_TRIPLET) }
    #[cfg(target_os = "linux")]
    { Some(LINUX_DEFAULT_TRIPLET) }
    #[cfg(target_os = "macos")]
    { Some(MAC_DEFAULT_TRIPLET) }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    { None }
}

#[cfg(windows)]
use vcv_rs::Arch;
#[cfg(windows)]
use vcv_rs::detect::{detect_sdk, detect_ucrt, detect_vs};
#[cfg(windows)]
use vcv_rs::env::build_env;

fn env_set<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, val: V) {
    // SAFETY: `xtask` mutates env only once on the main thread before spawning Cargo.
    unsafe {
        std::env::set_var(key, val);
    }
}

fn env_remove_var(key: &str) {
    unsafe {
        std::env::remove_var(key);
    }
}

pub fn prepare_build_environment() -> Result<()> {
    setup_vcpkg();
    #[cfg(windows)]
    windows_msvc_paths()?;
    fix_libclang();
    Ok(())
}

fn setup_vcpkg() {
    // Manifest-mode pin (vcpkg.json + vcpkg-configuration.json at workspace root,
    // FFmpeg installed locally under .vcpkg/installed/<triplet>/) takes precedence.
    // Falls through to global VCPKG_ROOT discovery if manifest install isn't populated yet.
    if try_manifest_mode_vcpkg() {
        prepend_pkg_config_path();
        return;
    }

    let prev_root = std::env::var_os("VCPKG_ROOT");

    if prev_root.is_none() {
        let candidate: PathBuf = if cfg!(windows) {
            PathBuf::from("C:/vcpkg")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join("vcpkg")
        } else {
            PathBuf::new()
        };

        if candidate.exists() {
            env_set("VCPKG_ROOT", candidate.as_os_str());
            eprintln!("xtask: VCPKG_ROOT -> {}", candidate.display());
        }

        #[cfg(not(windows))]
        if std::env::var_os("VCPKG_ROOT").is_none() {
            let usr = Path::new("/usr/local/share/vcpkg");
            if usr.exists() {
                env_set("VCPKG_ROOT", usr.as_os_str());
                eprintln!("xtask: VCPKG_ROOT -> {}", usr.display());
            }
        }
    }

    if std::env::var_os("VCPKGRS_TRIPLET").is_none() {
        if let Some(t) = default_triplet() {
            env_set("VCPKGRS_TRIPLET", t);
            eprintln!("xtask: VCPKGRS_TRIPLET -> {t}");
        }
    }

    prepend_pkg_config_path();
}

/// If the workspace ships a `vcpkg.json` and `.vcpkg/installed/<triplet>/lib/` is
/// populated, point `VCPKG_ROOT` at the local manifest-mode install root. This
/// pins FFmpeg to the baseline declared in `vcpkg-configuration.json` so CI and
/// local dev always link the same versions, regardless of the global vcpkg HEAD.
fn try_manifest_mode_vcpkg() -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    if !cwd.join("vcpkg.json").exists() || !cwd.join("Cargo.toml").exists() {
        return false;
    }

    let triplet = std::env::var("VCPKGRS_TRIPLET")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| default_triplet().map(str::to_owned))
        .unwrap_or_default();
    if triplet.is_empty() {
        return false;
    }

    let vcpkg_dir = cwd.join(".vcpkg");
    let lib_dir = vcpkg_dir.join("installed").join(&triplet).join("lib");
    if !lib_dir.exists() {
        eprintln!(
            "xtask: manifest-mode vcpkg install not populated at {}",
            lib_dir.display()
        );
        eprintln!(
            "xtask: run once to install pinned FFmpeg (~5–10 GB, ~20 min on first build):"
        );
        eprintln!(
            "xtask:   vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet {triplet}"
        );
        eprintln!("xtask: falling back to global VCPKG_ROOT for now.");
        return false;
    }

    env_set("VCPKG_ROOT", vcpkg_dir.as_os_str());
    env_set("VCPKGRS_TRIPLET", &triplet);
    eprintln!("xtask: manifest-mode VCPKG_ROOT -> {}", vcpkg_dir.display());
    eprintln!("xtask: triplet (manifest) -> {triplet}");
    true
}

fn prepend_pkg_config_path() {
    let Some(root_os) = std::env::var_os("VCPKG_ROOT") else {
        return;
    };
    let Ok(triplet) = std::env::var("VCPKGRS_TRIPLET") else {
        return;
    };
    if triplet.is_empty() {
        return;
    }
    let pc_dir = PathBuf::from(root_os)
        .join("installed")
        .join(&triplet)
        .join("lib")
        .join("pkgconfig");
    if !pc_dir.exists() {
        return;
    }
    let needle = pc_dir.to_string_lossy().into_owned();
    let sep = pkg_config_sep();
    let merged = match std::env::var_os("PKG_CONFIG_PATH") {
        Some(existing) => {
            let ex = existing.to_string_lossy();
            if split_path_list(ex.as_ref(), sep).any(|p| Path::new(p) == pc_dir) {
                OsString::from(ex.into_owned())
            } else if ex.is_empty() {
                OsString::from(needle)
            } else {
                OsString::from(format!("{needle}{sep}{ex}"))
            }
        }
        None => OsString::from(needle),
    };
    env_set("PKG_CONFIG_PATH", merged.as_os_str());
    eprintln!("xtask: PKG_CONFIG_PATH prepend -> {}", pc_dir.display());
}

fn split_path_list(s: &str, sep: char) -> impl Iterator<Item = &str> {
    s.split(sep).map(str::trim).filter(|x| !x.is_empty())
}

fn pkg_config_sep() -> char {
    if cfg!(windows) { ';' } else { ':' }
}

#[cfg(windows)]
fn windows_msvc_paths() -> Result<()> {
    let vs = detect_vs(None).with_context(|| {
        format!(
            "Visual Studio MSVC not detected (need vswhere + VC tools). Versions found: {:?}",
            vcv_rs::detect::list_vs_versions()
        )
    })?;
    let sdk = detect_sdk().context("Windows SDK (10.x) not found in registry")?;
    let ucrt = detect_ucrt().context("Universal CRT (Windows Kits 10.x) not found")?;

    let assembled = build_env(&vs, Some(&sdk), Some(&ucrt), Arch::X64, Arch::X64);

    for (k, v) in &assembled.vars {
        env_set(k, v);
    }

    prepend_sem_paths("PATH", &assembled.path);
    prepend_sem_paths("INCLUDE", &assembled.include);
    prepend_sem_paths("LIB", &assembled.lib);
    prepend_sem_paths("LIBPATH", &assembled.libpath);

    eprintln!("xtask: MSVC toolchain environment applied (via vcv-rs)");
    Ok(())
}

#[cfg(windows)]
fn prepend_sem_paths(var: &'static str, extra: &[PathBuf]) {
    if extra.is_empty() {
        return;
    }
    let prefix = paths_to_string(extra);
    merge_with_sep(var, &prefix, ';');
}

#[cfg(windows)]
fn paths_to_string(extra: &[PathBuf]) -> String {
    extra
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(windows)]
fn merge_with_sep(key: &'static str, prefix: &str, sep: char) {
    if prefix.is_empty() {
        return;
    }
    let merged = match std::env::var(key) {
        Ok(rest) if !rest.is_empty() => format!("{prefix}{sep}{rest}"),
        _ => prefix.to_owned(),
    };
    env_set(key, merged.as_str());
}

/// Clear `LIBCLANG_PATH` if it points at ESP-IDF / Xtensa clang (same as bootstrap.py).
fn fix_libclang() {
    let Ok(lcp) = std::env::var("LIBCLANG_PATH") else {
        return;
    };
    let lower = lcp.to_lowercase();
    if lower.contains("esp") || lower.contains("xtensa") {
        eprintln!("xtask: clearing LIBCLANG_PATH (ESP/Xtensa clang breaks bindgen/msvc-sys)");
        env_remove_var("LIBCLANG_PATH");
    }
}
