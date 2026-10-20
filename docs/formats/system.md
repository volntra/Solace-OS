# System snapshots and transactions

How Fern updates its sealed system (CLAUDE.md 6.2). Implemented by
`libspk::system`; driven by `spk install|remove|upgrade|rollback|systems`
on a running Fern.

## Layout

The root filesystem is one btrfs volume:

| Path (top level) | Mounted at | What |
|---|---|---|
| `@snapshots/<id>` | `/usr` (read-only) when booted | one bootable system; its content is the `/usr` tree |
| `@etc` | `/etc` | writable configuration |
| `@var` | `/var` | writable state |
| `@users` | `/Users` | home directories |

`<id>` is a positive integer; a newer system has a larger id. A system is
**sealed** when its subvolume is read-only. Only sealed systems are
bootable; an unsealed one is a transaction that did not finish.

At boot fern-init also mounts, for spk:

| Mount | What |
|---|---|
| `/run/fern/btrfs` | the volume's top level (`subvolid=5`), under the root-only `/run/fern` |
| `/boot` | the ESP (FAT32), found by `fern.esp=UUID=` on the kernel command line |

and writes `/run/fern/booted` (the booted subvolume, e.g. `@snapshots/2`).

## Inside each system

| Path (under the snapshot = `/usr`) | What |
|---|---|
| `lib/spk/db.sqlite` | the package database of this system |
| `lib/fern/boot/vmlinuz` | its kernel (from the `linux` package) |
| `lib/fern/boot/initramfs.img` | its initramfs (made by the transaction's initramfs trigger) |
| `lib/fern/system.toml` | metadata, below |

```toml
id = "2"
version = "0.1"          # os-release VERSION_ID of this system
created = 1792454400     # unix seconds
parent = "1"             # the system it was made from ("" for the first)
summary = "upgrade: fern-base 0.1-1 -> 0.1-2"
```

The initramfs is the only file in a system that no package owns: it is
derived, by the transaction, from the packaged `fern-init`.

## A transaction

`spk install|remove|upgrade` on a running Fern never changes the booted
system. Steps (any failure before step 8 deletes the new snapshot; the
running system and `grub.cfg` are untouched):

1. Take the system lock (`/run/fern/spk.lock`).
2. Delete unsealed snapshots left by an interrupted transaction.
3. Snapshot the **booted** system as `@snapshots/<max id + 1>`, writable.
4. Bind-mount it as `usr` of a staging root and run the ordinary package
   transaction there (resolve, download, verify signatures, apply, update
   its database), with the keys trusted by the running system. Nothing to
   do: stop and delete the snapshot.
5. Triggers: rebuild the initramfs from the new system's `fern-init`.
6. Write `lib/fern/system.toml`.
7. Seal the snapshot (read-only).
8. Run fern-bootgen with every sealed system and the boot settings, the new
   system as default: its kernel and initramfs are copied to the ESP and
   `grub.cfg` is replaced atomically.
9. Save the boot settings (`default` cleared: the newest boots) and delete
   snapshots fern-bootgen no longer keeps (never the booted one).

The change takes effect on reboot.

## Rollback

`spk rollback [ID]` makes `ID` (default: the newest sealed system older
than the current default) the default: it sets `default` in
`/etc/fern/boot.toml` and reruns fern-bootgen. Any kept system can also be
picked in the boot menu for a single boot.

## Boot settings

`/etc/fern/boot.toml` holds the boot settings domain
(docs/formats/bootgen.md); fern-settingsd will own it (Phase 6).
