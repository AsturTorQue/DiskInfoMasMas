# DiskInfo

Disk health (S.M.A.R.T.) and temperature for **Linux, Windows and macOS**, in the style of
[CrystalDiskInfo](https://github.com/hiyohiyo/CrystalDiskInfo) by hiyohiyo.

*Español más abajo.*

![Screenshot (sample data)](docs/screenshot.png)

## Download

Get the package for your system from the
[Releases page](https://github.com/AsturTorQue/Diskinfo-for-linux/releases):

| System | Package |
|--------|---------|
| Linux, Intel/AMD 64-bit | `DiskInfo-linux-x86_64.AppImage` |
| Linux, ARM 64-bit | `DiskInfo-linux-aarch64.AppImage` |
| Windows, Intel/AMD 64-bit | `DiskInfo-windows-x64.zip` |
| Windows on ARM | `DiskInfo-windows-arm64.zip` |
| macOS 11+, Intel and Apple silicon | `DiskInfo-macos-universal.zip` |

Every package carries its own `smartctl` (smartmontools 7.5), so nothing else needs installing.
When the system already has `smartctl`, that one is used.

- **Linux:** make the AppImage executable (right click → Properties → *Is executable*, or
  `chmod +x DiskInfo-linux-*.AppImage`) and open it. Works on immutable systems (Bazzite,
  Bluefin, Silverblue, Kinoite, SteamOS…) without `rpm-ostree`.
- **Windows:** unzip and open `DiskInfo.exe` (keep `smartctl.exe` beside it). Reading disk
  health needs administrator rights: press **Restart as administrator**.
- **macOS:** unzip and move `DiskInfo.app` to Applications. The app isn't notarised by Apple, so
  the first time right click it → **Open** → **Open** (or run
  `xattr -dr com.apple.quarantine /Applications/DiskInfo.app`). **Read S.M.A.R.T.** asks for your
  password.

## Features

- One tab per disk (SATA, NVMe, USB) with its health status and temperature.
- Health status **Good / Caution / Bad / Unknown**, with remaining life for SSDs, using the same
  rules as CrystalDiskInfo (see [MECHANICS.md](MECHANICS.md)).
- Firmware, serial number, interface, transfer mode, mount points or drive letters, host
  reads/writes, rotation rate, power-on count and hours, standard and features.
- Full S.M.A.R.T. attribute table, with a colour per attribute and hover help.
- **Partitions** tab: a map of the disk and, for each partition, label, file system (also
  through encryption/LVM layers), size, used and free space, mount point or drive letter and type;
  plus the partition table (GPT/MBR) and unallocated space.
- **Details** tab: exact capacity, device path, model family, WWN, sector sizes, form factor,
  S.M.A.R.T. self-assessment, last self-test, logged errors, NVMe controller facts, Windows'
  own health/boot/system flags and more, whatever the system reports.
- Adjustable warning thresholds per disk (reallocated / pending / uncorrectable sectors,
  SSD life, temperature alarm).
- Auto refresh, copy a text report, Fahrenheit, green mode.
- 6 languages (Español, English, Português, Français, Deutsch, Italiano) and 6 themes.
- Linux: optional passwordless reading through a narrow polkit rule (see below).

## Administrator rights

Every system only lets administrators talk to disks directly.

- **Linux:** the app asks once per read through `pkexec`. Without it you still see model,
  serial, firmware, size, mount points and the temperature of NVMe drives.
  **Function → Read without password…** installs `/etc/polkit-1/rules.d/49-diskinfo.rules`
  (asking for the password once). The rule lets only your user, at a local active session, run
  exactly `smartctl -j -a /dev/<disk>`: no other programs, no changing disk settings, no
  self-tests. The same window removes it.
  The bundled `smartctl` lives inside the AppImage, which root cannot read; root only runs a copy
  whose SHA-256 matches the bundled original. For passwordless mode on a system without
  `smartctl`, a verified root-owned copy is installed in `/usr/local/libexec/diskinfo/`.
- **Windows:** without administrator rights the app lists disks, sizes and drive letters;
  **Restart as administrator** opens it again with a UAC prompt to read S.M.A.R.T.
- **macOS:** **Read S.M.A.R.T.** runs `smartctl` as administrator through the standard macOS
  password dialog.

## Build

Install Rust from <https://rustup.rs>, then:

```sh
cargo build --release
./target/release/diskinfo
```

Options: `--lang es|en|pt|fr|de|it`, `--theme light|dark|mint|ocean|sakura|contrast`,
`--demo` (two sample disks), `--screenshot file.png` (draw, save a picture and exit).
Run the tests with `cargo test`.

### Packages

Pushing a tag such as `v0.2.0` runs [`.github/workflows/release.yml`](.github/workflows/release.yml),
which builds all five packages on GitHub's machines and attaches them, with `SHA256SUMS.txt`, to
that tag's release. By hand:

| Package | Command | Runs on |
|---------|---------|---------|
| AppImage | `packaging/build-appimage.sh /path/to/appimagetool` | Linux (x86_64 or aarch64) |
| Windows zip | `packaging/build-smartctl.sh windows-x86_64` (Linux with mingw-w64), then `packaging\build-windows.ps1 -Target x86_64-pc-windows-msvc -Arch x64 -Smartctl target\smartctl.exe` | Windows |
| macOS app | `packaging/build-macos.sh` | macOS |

`packaging/build-smartctl.sh` builds `smartctl` from the official smartmontools source and checks
its SHA-256. Windows on ARM uses the x64 `smartctl.exe` through Windows' built-in emulation.

## Project layout

| Path | What it is |
|------|------------|
| `src/main.rs` | Window, menus and settings |
| `src/disks.rs` | Finding disks (`lsblk`/`/sys` on Linux, `smartctl --scan` and Windows' disk list elsewhere) and reading `smartctl` JSON |
| `src/health.rs` | Health and temperature rules |
| `src/i18n.rs` | Translations and help texts |
| `src/theme.rs` | Colour themes |
| `src/platform.rs` | Settings folders, administrator checks, Windows console-less commands |
| `src/smartctl.rs` | Finding the system or bundled `smartctl` |
| `src/polkit.rs` | Optional passwordless rule (Linux) |
| `src/integrate.rs` | Application-menu entry and icon (Linux) |
| `src/demo.rs` | Sample disks for `--demo` |
| `packaging/` | Icons, desktop entry, macOS `Info.plist` and packaging scripts |

## Credits and license

Based on CrystalDiskInfo, Copyright (c) 2008-2023 hiyohiyo (MIT, see
[LICENSE-CrystalDiskInfo.txt](LICENSE-CrystalDiskInfo.txt)). This program is a new implementation
in Rust and is not affiliated with the original author. Released under the MIT License
(see [LICENSE](LICENSE)).

The packages also include `smartctl` from [smartmontools](https://www.smartmontools.org/)
(GPL-2.0-or-later), unmodified and run as a separate program; its license and source link travel
with each package.

---

## Español

**DiskInfo** muestra la salud (S.M.A.R.T.) y la temperatura de tus discos en **Linux, Windows y
macOS**, al estilo de CrystalDiskInfo de hiyohiyo. Descarga el paquete de tu sistema desde la
página de versiones (Releases): <https://github.com/AsturTorQue/Diskinfo-for-linux/releases>

- **Linux** (Intel/AMD y ARM): marca el AppImage como ejecutable (clic derecho → Propiedades →
  «Es ejecutable») y ábrelo. Funciona en sistemas inmutables (Bazzite, Silverblue…) sin
  `rpm-ostree`. La lectura pide la contraseña con `pkexec`, o actívala sin contraseña en
  **Función → Lectura sin contraseña…**.
- **Windows** (x64 y ARM): descomprime y abre `DiskInfo.exe` (deja `smartctl.exe` al lado). Para
  ver la salud pulsa **Reiniciar como administrador**.
- **macOS** (Intel y Apple silicon): descomprime y mueve `DiskInfo.app` a Aplicaciones. La primera
  vez, clic derecho → **Abrir** → **Abrir**, porque no está firmada por Apple. **Leer S.M.A.R.T.**
  pide tu contraseña.

Además del estado S.M.A.R.T., las pestañas **Particiones** (mapa del disco, sistema de archivos,
espacio usado y libre, montaje, tipo) y **Detalles** (capacidad exacta, WWN, sectores, última
autoprueba, errores registrados…) muestran más información de cada disco.

Cada paquete incluye su propio `smartctl`, así que no hace falta instalar nada más.
6 idiomas y 6 temas. Licencia MIT; basado en CrystalDiskInfo (MIT, © hiyohiyo).
