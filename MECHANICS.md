# Mechanics sheet (from DiskInfo64.exe, Ghidra pseudo-C)

## Health status (function 0x1400125b4, 0x14005a064)
Status codes: 0 = Unknown, 1 = Good, 2 = Caution, 3 = Bad. Good has a green variant ("GreenMode").

ATA / SATA drives (0x14005a064):
- No usable attributes → Unknown.
- Any attribute with threshold != 0 and current < threshold → Bad.
- Attributes 0x05 (reallocated), 0xC5 (pending), 0xC6 (uncorrectable): if the drive is NOT an SSD,
  raw value (low 16 bits) >= user threshold and threshold != 0 → Caution. Raw all 0xFF → ignored.
  Default thresholds: 05 = 1, C5 = 1, C6 = 1 (0x140013d50).
- SSD remaining-life attribute (vendor dependent: A9, AD, B1, CA, E7, E8, E9...):
  life == 0 → Bad; life <= threshold FF (default 10) → Caution. Life is clamped to 0..100.
  (Which attribute per vendor: simplified here, see "estimated" below.)
- Result: Bad if any bad, else Unknown if nothing checked, else Caution if any caution, else Good.

NVMe drives (0x14005a064, branch for interface type 0x13):
- Critical warning != 0 → Bad.
- If spare threshold in 1..100: available spare < spare threshold → Bad; equal (and threshold != 100) → Caution.
- Life = 100 - percentage used; life <= threshold FF (10) → Caution; otherwise Good.
- Virtual disks (model starting with Parallels, VMware, QEMU) → Unknown.

## Temperature (0x140013cfc, 0x140013d50)
- Unknown sentinel -1000.
- temperature >= alarm → "temperatureBad" (red); else Good (blue, or green in GreenMode).
- Alarm default: 60 °C for SSDs, 50 °C for HDDs (per-disk setting "AlarmTemperature").

## Settings (DiskInfo.ini next to the original)
- AutoRefresh = 10 minutes. Window height 640.

## Report text (0x14000e970)
Copy format: Model, Firmware, Serial Number, Disk Size, Interface, Standard, Transfer Mode,
Power On Hours, Power On Count, Host Reads/Writes, Temperature, Health Status, Features, then the attribute table.

## Linux side (own design)
- Disk list: `lsblk -J -d -b`; NVMe temperature without root from /sys/class/hwmon (name = nvme / drivetemp).
- S.M.A.R.T.: `smartctl -j -a /dev/X`; if denied, one `pkexec` call reads all disks.
- Estimated: per-vendor SSD life attribute choice (we use the first of known life attributes present).
- Estimated: NVMe host reads/writes = data units * 512000 bytes (NVMe spec), shown in GB.
