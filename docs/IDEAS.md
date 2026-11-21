# Ideas for more disk information

Done in 0.5.0 (taken from the official CrystalDiskInfo): hide serial numbers and the S.M.A.R.T.
table, interface size, English S.M.A.R.T. names, hide disks without S.M.A.R.T., save the report as
text or picture, alert history, start with the system.

Not done from the official CrystalDiskInfo:

- **Tray icon** with a temperature icon per disk (needs a tray library on each system).
- **AAM/APM control** (noise and power level of a hard disk): it changes disk settings, so it would need its own, stricter permission rule.
- **Alert by email** (the alert sound is done; choosing your own sound file is not).
- **Graph of any S.M.A.R.T. attribute** (today: temperature, life, data written, bad sectors).
- **USB bridge workarounds** for enclosures `smartctl` does not recognise by itself, and RAID controllers (Intel RST/VROC, AMD RAIDXpert2, JMicron).
- **Windows-only extras**: event log entries, desktop gadget, drive-letter sorting.

Done in 0.3.0: history graphs, SSD life estimate, self-tests, desktop alerts, live speed,
NVMe/ATA extended facts and partition state (encryption, read-only, inodes, almost full).

Still open:

- **Tray icon** so alerts keep running with the window closed (today the app has to be open,
  minimised is fine).
- **Manufacturer TBW** per model, to compare the life estimate with the rated endurance.
- **File-system checks** – last `fsck`/scrub (btrfs/ZFS) per partition (needs administrator rights).
- **RAID and USB bridges** – member disks of mdadm/LVM/Storage Spaces, and S.M.A.R.T. behind
  USB enclosures that need a specific `-d` type.
- **Export** – save the report as text/HTML/JSON, and compare two reports to see what changed.
