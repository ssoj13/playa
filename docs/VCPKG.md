# vcpkg + FFmpeg setup for playa

This document is the single source of truth for how playa consumes FFmpeg via
vcpkg, how the baseline is pinned, how `xtask` wires up the build environment,
and how to recover when something breaks.

> Other docs reference this one — keep it accurate.

---

## TL;DR

Run **once per machine** from the project root:

```powershell
vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet x64-windows-static-md-release
```

Then build forever with:

```powershell
cargo xtask build         # release
cargo xtask build --debug # debug
python bootstrap.py build # delegates to xtask
```

`xtask` detects `.vcpkg/installed/` and pins `VCPKG_ROOT` at it; the global
`c:\vcpkg\installed\` becomes irrelevant for playa.

---

## What's in the repo

| File | Role |
|---|---|
| `vcpkg.json` | Manifest. Declares `ffmpeg` with feature set `[avcodec, avformat, swresample, swscale, nvcodec]`. **No `avdevice` / `avfilter`** — see below. |
| `vcpkg-configuration.json` | Pins `microsoft/vcpkg` to a specific git commit (the *baseline*). All ports resolve at exactly that revision regardless of the local vcpkg checkout state. |
| `crates/xtask/src/env_setup.rs` | Detects manifest install at `.vcpkg/installed/<triplet>/lib/`, sets `VCPKG_ROOT` accordingly, falls back to the global vcpkg if missing. Also activates the MSVC toolchain via `vcv-rs`. |
| `.gitignore` | `.vcpkg/` is local-only build state (multi-GB); the two manifest JSONs are whitelisted past the catch-all `*.json` rule. |

Current pinned baseline: see `vcpkg-configuration.json` → `default-registry.baseline`.

---

## Why manifest mode

Without a pin, every fresh clone or every `cd $VCPKG_ROOT && git pull` may bring
in a new FFmpeg port revision that breaks the build (new C++ runtime
requirements, new enum variants, removed features, …). With manifest mode:

- `vcpkg install` consults `vcpkg-configuration.json` → resolves ports at the
  pinned baseline commit, ignoring the global vcpkg working copy.
- Output goes to `<project>/.vcpkg/installed/<triplet>/` instead of the global
  `<vcpkg-root>/installed/<triplet>/`. CI and dev machines get bit-identical
  installs.
- The Rust `vcpkg` build-dep (used by both `crates/playa-ffmpeg/build.rs` and
  the upstream `ffmpeg-sys-next/build.rs`) only knows how to look at
  `$VCPKG_ROOT/installed/<triplet>/`. `xtask::env_setup` overrides
  `VCPKG_ROOT` to point at our local `.vcpkg/`, so both build scripts find the
  pinned libs without any patching.

---

## Bumping the baseline

```powershell
# 1. Get the SHA you want
cd C:\vcpkg
git pull
git rev-parse HEAD          # copy the SHA

# 2. Replace it in the project
cd C:\projects\projects.rust.cg\playa
# edit vcpkg-configuration.json → default-registry.baseline = "<new-SHA>"

# 3. Wipe the cached install and reinstall
rmdir /s /q .vcpkg\installed\x64-windows-static-md-release
vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet x64-windows-static-md-release

# 4. Rebuild + commit
cargo xtask build --debug
git add vcpkg-configuration.json
git commit -m "build: bump vcpkg baseline to <SHA>"
```

If the new baseline introduces FFmpeg ABI changes, the wildcard `_ =>` arms in
`crates/playa-ffmpeg/src/codec/id.rs`, `codec/packet/side_data.rs`,
`util/frame/side_data.rs`, `util/color/primaries.rs`, and
`util/color/transfer_characteristic.rs` keep the conversions
forward-compatible. New variants map to a sensible default
(`Id::None`, `Type::DataNb`, `Type::PanScan`, `Unspecified`).

---

## Install commands per platform

| Triplet | Install command |
|---|---|
| Windows static-md | `vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet x64-windows-static-md-release` |
| Linux | `vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet x64-linux-release` |
| macOS Intel | `vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet x64-osx-release` |
| macOS Apple Silicon | `vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet arm64-osx-release` |

`x64-windows-static-md-release` is a custom triplet shipped by the
`crates/playa-ffmpeg/README.md` setup. If you don't have it yet, copy the
recipe from there or from the upstream `playa-ffmpeg/docs/VCPKG.md`.

---

## Why `avdevice` / `avfilter` are excluded

vcpkg's FFmpeg 8.1+ `avfilter` build now compiles `vsrc_gfxcapture_winrt` —
a Windows.Graphics.Capture WinRT source filter that uses C++ `<regex>`.

Symptom: link fails with

```
avfilter.lib(vsrc_gfxcapture_winrt.o) :
  error LNK2019: unresolved external symbol __std_regex_transform_primary_char
```

The symbol lives in `msvcp140.lib` (MSVC C++ runtime), and is only present in
**recent** MSVC tools. If vcpkg builds avfilter under one MSVC version and
your `cargo build` links against a different MSVC C++ STL, the symbol is
missing and the link fails. There is no vcpkg feature flag to opt out of
`vsrc_gfxcapture` — the only fix is to **not build avfilter at all**.

Playa never calls into the FFmpeg filter graph API anyway (the only
`filter` references in playa-engine / playa-ui are image filters with the same
word). `avdevice` is similarly unused.

This is why `vcpkg.json` declares `default-features: false` and selects only
the audio/video codec/format/scaling subset, and why `crates/playa-ffmpeg`
also drops `device` and `filter` from its default cargo features.

---

## How `xtask` wires the env

`crates/xtask/src/env_setup.rs::prepare_build_environment()` runs before
`cargo build` is forked, in this order:

1. `try_manifest_mode_vcpkg()` — if `vcpkg.json` exists and
   `.vcpkg/installed/<triplet>/lib/` is populated, set
   `VCPKG_ROOT=<workspace>/.vcpkg` and `VCPKGRS_TRIPLET=<triplet>`. Done.
2. Otherwise: print the install command, fall back to whatever
   `VCPKG_ROOT` / `VCPKGRS_TRIPLET` were set globally (or default
   `C:/vcpkg` + `x64-windows-static-md-release` on Windows).
3. `prepend_pkg_config_path()` — derives `PKG_CONFIG_PATH` from
   `<VCPKG_ROOT>/installed/<triplet>/lib/pkgconfig`.
4. (Windows only) `windows_msvc_paths()` — uses `vcv-rs` to query the
   active Visual Studio install + Windows SDK + UCRT, prepends the
   resulting `INCLUDE` / `LIB` / `LIBPATH` / `PATH`.
5. `fix_libclang()` — clears `LIBCLANG_PATH` if it points at ESP/Xtensa
   clang (those break bindgen).

After all this the parent process forks `cargo`, which inherits the
populated env. Every downstream `build.rs` (including `ffmpeg-sys-next`'s
bindgen step) sees the right paths.

---

## Troubleshooting

### "manifest-mode vcpkg install not populated"

That's expected on a fresh clone. Run the install command from "TL;DR" and
rebuild.

### "fatal error: 'errno.h' file not found" / "called `Result::unwrap()` on an `Err` value … pkg-config exited with status code 1"

MSVC env not active in the shell. Always run via `cargo xtask build` —
running plain `cargo build` from a non-Developer shell skips the env setup.

### Link error: `__std_regex_transform_primary_char`

You re-enabled `avfilter` somewhere. Check:
- `vcpkg.json` features list shouldn't include `avfilter`
- `crates/playa-ffmpeg/Cargo.toml` `default` shouldn't include `filter`
- No downstream crate in the workspace depends on
  `playa-ffmpeg/filter`

### `Cargo.lock` re-resolves vfx-rs from a different revision

`vfx-rs` is a public github HTTPS dep declared in
`crates/playa-io/Cargo.toml`. There's no `[patch]` block; Cargo uses
whatever's in `Cargo.lock`. If you need to bump:

```powershell
cargo update -p vfx-exr -p vfx-io -p vfx-core
```

---

## Where this came from

| Date | Change |
|---|---|
| 2026-05-09 | Pinned baseline `4bc07e3eb00c5a9539a5a7a83415150a9260f8db`; manifest mode added |
| 2026-05-09 | Switched `vfx-rs` source from `ssh://` to `https://`; removed dead `[patch]` block |
| 2026-05-09 | Vendored `playa-ffmpeg` as workspace member; dropped `avdevice` / `avfilter` from default features; added forward-compat wildcard arms for FFmpeg 8.x point releases |
| 2026-05-09 | `xtask::env_setup` introduced (vcv-rs-based MSVC env + vcpkg env auto-discovery) |
