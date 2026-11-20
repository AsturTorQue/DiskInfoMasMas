# Credits

## CrystalDiskInfo — the original

**DiskInfo++** is inspired by and modelled on **[CrystalDiskInfo](https://github.com/hiyohiyo/CrystalDiskInfo)**,
the Windows disk health tool created by **hiyohiyo** (Noriyuki Miyazaki) of
[Crystal Dew World](https://crystalmark.info/).

- The health rules (Good / Caution / Bad, the 05/C5/C6 and remaining-life thresholds, the
  temperature alarms) follow CrystalDiskInfo's behaviour; see [MECHANICS.md](MECHANICS.md).
- The window layout (disk tabs, health and temperature boxes, attribute table) follows its design.
- CrystalDiskInfo is released under the MIT License, Copyright (c) 2008-2023 hiyohiyo. Its notice is
  kept in [LICENSE-CrystalDiskInfo.txt](LICENSE-CrystalDiskInfo.txt).

DiskInfo++ is an independent rewrite in Rust for Linux, Windows and macOS. It contains no code copied
from CrystalDiskInfo and is not affiliated with or endorsed by its author. If you find CrystalDiskInfo
useful on Windows, please support the original project.

## smartmontools

The packages include `smartctl` from **[smartmontools](https://www.smartmontools.org/)**
(GPL-2.0-or-later), unmodified and run as a separate program. Its licence and a link to its source
travel with each package.

## Libraries

Built with [egui/eframe](https://github.com/emilk/egui), [serde](https://serde.rs/),
[image](https://github.com/image-rs/image) and other Rust crates listed in `Cargo.toml`, under their
own licences.

---

## Créditos (español)

**DiskInfo++** está inspirado y basado en **[CrystalDiskInfo](https://github.com/hiyohiyo/CrystalDiskInfo)**,
creado por **hiyohiyo** (Noriyuki Miyazaki, Crystal Dew World), publicado con licencia MIT
(© 2008-2023 hiyohiyo). Sus reglas de salud y su diseño son el modelo de esta app. DiskInfo++ es una
reescritura independiente en Rust, sin código copiado del original y sin relación oficial con su autor.
Incluye `smartctl` de smartmontools (GPL-2.0 o posterior), sin modificar.
