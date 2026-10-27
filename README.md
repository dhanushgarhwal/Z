<p align="center">
  <img src="assets/banners/readme-1920x480.png" alt="Z banner" width="100%">
</p>


<div align="center">

**Fast & Open Source Archiver for Windows.**

[Features](#features) &nbsp;·&nbsp; [Formats](#supported-formats) &nbsp;·&nbsp; [Install](#installation) &nbsp;·&nbsp; [Usage](#usage) &nbsp;·&nbsp; [Build](#building-from-source) &nbsp;·&nbsp; [Security](#security-and-reliability)

</div>


<br>


## Overview

Z opens, creates and extracts the archives you meet every day, through a clean native window, a command-line tool and your right-click menu. It ships as a single self-contained program: no runtime to install, no web view, no background services, and no advertising or telemetry.


<br>

## Features

| | |
|---|---|
| **Native interface** | Browse archives as folders, sort and search, drag and drop to open or to create. Double-click a file to open it straight from the archive. |
| **Create with control** | Choose the format and compression level, and protect ZIP and 7z archives with AES-256 encryption. |
| **Fast extraction** | Multi-threaded ZIP extraction, streaming everywhere, low memory use. Extract everything or only your selection. |
| **Explorer integration** | Right-click to extract or to add files and folders to an archive. Per-user by default, so no administrator rights are needed. |
| **Command line** | `zc` brings the same engine to scripts, CI jobs and terminals. |
| **Safe by design** | Hostile archive paths can never write outside the destination. See [Security and reliability](#security-and-reliability). |

<br>

## Supported formats

| Format | Browse | Extract | Create | Encryption |
|---|:---:|:---:|:---:|:---:|
| ZIP (also JAR, APK, DOCX, XLSX, PPTX, EPUB, ODT) | ✓ | ✓ | ✓ | AES-256 |
| 7z | ✓ | ✓ | ✓ | AES-256 |
| TAR, TAR.GZ, TAR.BZ2, TAR.XZ, TAR.ZST | ✓ | ✓ | ✓ | — |
| GZ, BZ2, XZ, ZST | ✓ | ✓ | ✓ | — |
| RAR | ✓ ¹ | ✓ ¹ | — ² | — |

¹ Optional: build with `--features rar`. The UnRAR licence is not OSI-approved, so RAR support is opt-in.  
² RAR is a proprietary format that no free tool can create.

Formats are detected from file contents first and from the name second, so a renamed archive still opens.

<br>

## Installation

Download **`Z-Setup-<version>.exe`** (installer) or **`Z-portable-<version>.zip`** (no installation) from the Releases page.

The installer is per-user by default and offers an all-users install on request. During setup you can choose to:

- install the `zc` command-line tool and add it to `PATH`
- add **Extract** and **Add to archive** entries to the Explorer right-click menu
- open ZIP, 7z, RAR, TAR and single-file compressed formats with Z
- create Start menu and desktop shortcuts

On Windows 11 the Z entries appear under **Show more options**.

<br>

## Usage

### Desktop app

Launch **Z**, then open an archive, or drop one onto the window. Dropping anything else starts a new archive.

| Action | How |
|---|---|
| Open a folder or file inside an archive | Double-click, or select and press `Enter` |
| Go up one level | `Backspace` or **Up** |
| Select a range / several items / everything | `Shift`-click / `Ctrl`-click / `Ctrl`+`A` |
| Extract | Select items (or nothing, for everything) and click **Extract** |
| Create an archive | **Add** or **Add folder**, then choose format, level and password |

### Command line

```text
zc a -f 7z -l 3 -p secret -o backup.7z photos\ docs\
zc x --to-folder big.tar.zst
zc l archive.zip
zc t *.7z
```

| Command | Purpose |
|---|---|
| `zc a [options] <path>...` | Add files and folders to an archive |
| `zc x [options] <archive>...` | Extract |
| `zc t [options] <archive>...` | Test integrity |
| `zc l <archive>` | List contents |

| Option | Meaning |
|---|---|
| `-f <format>` | `zip` `7z` `tar` `tar.gz` `tar.bz2` `tar.xz` `tar.zst` `gz` `bz2` `xz` `zst` |
| `-l <0-3>` | Compression level, from store to maximum |
| `-o <file or folder>` | Output archive (`a`) or destination folder (`x`) |
| `-p <password>` | Password for encryption or decryption |
| `--here` · `--to-folder` · `--ask` | Extract next to the archive, into a folder named after it, or choose a folder |
| `-y` · `-s` | Overwrite existing files · skip existing files (default: rename) |

<br>

## Building from source

Requirements: the stable Rust toolchain (MSVC on Windows) and, for the installer, [Inno Setup 6](https://jrsoftware.org/isinfo.php).

```text
cargo test --workspace
cargo build --release
iscc /DVersion=0.1.0 installer\z.iss
```

| Output | Location |
|---|---|
| Desktop app | `target\release\Z.exe` |
| Command-line tool | `target\release\zc.exe` |
| Installer | `dist\Z-Setup-<version>.exe` |

Optional steps:

```text
cargo build --release --features rar     # RAR browsing and extraction
python tools/build_assets.py             # regenerate icons, banners, installer artwork (pillow, numpy, fonttools)
python tools/gen_registry.py             # regenerate Explorer menu and file-association entries
```

### Project layout

| Path | Contents |
|---|---|
| `crates/zcore` | Archive engine: detection, listing, extraction, creation, integrity tests |
| `crates/z` | Desktop app (`Z.exe`) and command-line tool (`zc.exe`) |
| `installer` | Inno Setup script and wizard artwork |
| `assets` | Logo, icons, banners and the UI font |
| `tools` | Asset and registry generators |

<br>

## Security and reliability

- **Safe extraction.** Every archive path is validated before use: no `..` segments, absolute paths, drive letters or NTFS stream names. Symbolic links and device entries inside archives are never created.
- **Atomic creation.** Archives are written to a temporary `.part` file and renamed on success, so a cancelled or failed job leaves nothing behind.
- **Low memory.** Data is streamed throughout. ZIP extraction uses one file handle per worker thread. Wide-script fonts (CJK, Arabic, Thai) are loaded from Windows only when an archive actually contains such names.
- **Small and self-contained.** Release builds use full LTO, `opt-level = 3`, `panic = abort` and stripped symbols. The interface font is a 22 KB subset per weight.
- **Per-user integration.** Explorer entries use classic cascading verbs under `SystemFileAssociations`, so Z never takes over another application's file type. Windows launches classic verbs once per selected file; Z merges them into a single job (Windows caps classic multi-select at 15 items).

<br>

## Fonts and licences

Z is released under the [MIT License](LICENSE).

The interface font, *Z Sans*, is a subset of Poppins (SIL Open Font License 1.1, no Reserved Font Name) renamed for the app. Its licence text ships alongside it in [`assets/font/OFL.txt`](assets/font/OFL.txt).

<br>

---

<div align="center">

<img src="assets/icons/z-64.png" width="32" alt="Z">

<sub>Designed and Developed by <b>Dhanush Garhwal</b></sub>

<sub>Released under the MIT License</sub>

</div>
