# Kernel module lists (fern-init)

fern-init loads kernel modules during early setup, before any service
starts. There is no kmod userspace (no modprobe, no depmod): packages list
what they need, and fern-init loads exactly that.

Lists are files named `*.conf` in:

1. `/usr/lib/fern/modules.d/` (shipped by packages)
2. `/etc/fern/modules.d/` (administrator: a file with the same name
   replaces the package's)

Files are read in name order (use a number prefix to order them). Each
line is a module name, then its parameters; `#` starts a comment:

```text
# 20-nvidia.conf: the core first, then what builds on it.
nvidia NVreg_PreserveVideoMemoryAllocations=1
nvidia-modeset
nvidia-drm modeset=1 fbdev=1
```

- Modules are looked up by name under `/usr/lib/modules/<release>/`
  (`-` and `_` are interchangeable; `.ko`, `.ko.zst`, `.ko.xz`, `.ko.gz`).
  Compressed modules are decompressed by the kernel
  (`CONFIG_MODULE_DECOMPRESS`).
- There is no dependency resolution: a list names the modules a module
  needs before it.
- A module that is already loaded, or built into the kernel, counts as
  loaded. Any other failure is logged (`fernctl logs fern-init`) and boot
  continues without it.
- Not supported: loading modules on hotplug (udev's `kmod` builtin), and
  blacklists. Hardware Fern supports is built in or listed.
