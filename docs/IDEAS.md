# Ideas for more disk information

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
