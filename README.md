# PicoCalc OS (Rust)

A simple kernel and applications for the **Clockwork PicoCalc**, written in Rust.  
This project provides a minimal kernel, syscall table, and user-space applications to experiment with kernel development on constrained hardware.

## Status

Basic synchronous applications are working great.  
Current focus is on exanding applications and porting software, finding bugs in ffi, and making sure the kernel is as stable as possible.

## Project Structure

- **`kernel/`** – The core OS kernel
- **`userlib_sys/`** – C FFI bindings for kernel syscall
- **`userlib/`** – Rust wrapper on top of `userlib_sys` 
- **`user_apps/picolibc/`** – App-only C library submodule, built with `just newlib`; the kernel does not use it.
- **`user_apps/`** – Collection of userspace programs (gif player, wav player, calculator, snake, etc.)

## Features

- Minimal Rust-based kernel targeting the PicoCalc
- Custom ABI for *Mostly* safe communication between kernel and applications
- Support for multiple user-space applications
- Hardware drivers tailored for the PicoCalc( Audio, Display, Keyboard, ans Storage )

## Getting Started

Launcher: Up/Down selects a program; Enter/Right launches it. Del prompts to
permanently delete the selected `.bin` from the SD card; Enter confirms and Esc
cancels. Failed deletions show an error and keep the program in the list.

The current `embedded-sdmmc` 0.9 delete API removes the directory entry but does
not reclaim the file's allocated clusters. Reclaim space using a computer.

```bash
git clone https://github.com/LegitCamper/picocalc-os-rs.git
cd picocalc-os-rs
just userapps
# copy the build applications from target/thumbv8m.main-none-eabihf/release-binary/application to the sdcard and rename them to app.bin

# has builds for the official rp2350 board and the pimoroni2w board
just kernel-release rp235xa # keep in mind that latest is required because of https://github.com/JoNil/elf2uf2-rs/pull/41 
```

## Doom Game Data

Doom requires a **pimoroni2w board with 8 MiB PSRAM** and your own trusted
Doom shareware IWAD. Copy it as `DOOM1.WAD` into the **SD card root**, next to
`DOOM.BIN`, not inside a `doom/` folder:

```text
SD Card/
  DOOM.BIN    (built as target/doom/doom.bin)
  DOOM1.WAD  (Doom shareware game data)
```

Over USB MTP, the destination is **PicoCalc > SD Card > DOOM1.WAD**.
The app opens `/doom1.wad`; FAT filenames are case-insensitive.
The WAD is not bundled or downloaded by the build. Renaming another game's
WAD does not make it compatible. A missing WAD prevents Doom from starting.
See [Doom build and run instructions](user_apps/doom/README.md).

## USB File Transfer

USB exposes an MTP device named **PicoCalc**, with storage named **SD Card**.
Uploads currently target the SD Card root and require ASCII 8.3 filenames
(up to eight characters plus a three-character extension, such as `snake.bin`).
Subdirectory uploads and long filenames are rejected, not silently renamed.
New uploads cannot truncate existing files. Use a new name to preserve an older copy.

`just copy-userapps` rebuilds all five apps and replaces their root-directory
copies over MTP using GIO/GVfs (the host's GIO when running in a toolbox).
The copy step uses `CALC.BIN` for `calculator` and `WAVPLAY.BIN` for `wav_player`;
other app names are uppercased. Build target names remain unchanged.
Existing files are backed up locally before deletion, and uploads are downloaded
and compared byte-for-byte. A failed transfer stops the recipe and retains the
printed temporary directory for recovery; replacement is not atomic.
Identical files are verified without deleting or rewriting them.

`just copy-userapp <app>` also copies that app's bundled assets, with the same
backup and readback checks. MTP remains root-only: gallery BMPs in
`assets/gallery/` use `IMG0001.BMP` through `IMG0005.BMP`, and
`assets/gif/bad_apple.gif` uses `BADAPPLE.GIF`. Additional GIFs and WAVs in
`assets/gif/` and `assets/wav_player/` must already have valid 8.3 names.
No WAV sample is bundled. Gallery, GIF, and WAV apps scan both the root and their
existing `/images`, `/gifs`, and `/music` folders. The GIF player accepts files
up to 2 MiB and one megapixel, centering and clipping larger-than-screen images.

Snake advances every 125 ms (eight steps per second), independent of rendering
speed. Input stays responsive during waits.

MTP object handles remain stable when other root entries are deleted or created.
Deleted handles are retired rather than reassigned to another file. After flashing
a kernel with this fix, reconnect USB and remount MTP to discard old host metadata.

Connect a data cable to the Pico board's native USB port; the debug probe alone
does not provide MTP. Leave the device at its launcher (MTP pauses while an app
runs). In an MTP-capable file manager, open **PicoCalc > SD Card** and copy files
into that root folder. This is file transfer, not a USB disk or drive letter.
