# Recipe format (`recipes/<name>/recipe.toml`)

A recipe describes how to build one upstream source into one or more
`.solpkg` packages. `spk build recipes/<name>` reads it.

```toml
name = "xz"                 # [a-z0-9][a-z0-9+._-]*, unique across recipes/
version = "5.8.1"           # upstream version (see "Versions" below)
release = 1                 # our revision of this version, starts at 1
summary = "XZ/LZMA compression tools"
license = "0BSD AND GPL-2.0-or-later"
homepage = "https://tukaani.org/xz/"

[[source]]
url = "https://github.com/tukaani-project/xz/releases/download/v5.8.1/xz-5.8.1.tar.xz"
sha256 = "0b54f79df85912504de0b14aec7971e3f964491af1812d83447005807513cd9e"
# extract = true            # default: unpack into the build dir
# dir = "xz-5.8.1"          # directory the archive unpacks to
#                           # (default: the single top-level dir)

[deps]
build = ["zlib"]            # installed into the build sysroot
runtime = []                # added to the main package's `depends`

[build]
system = "autotools"        # autotools | cmake | make | custom
configure_args = ["--disable-static"]
make_args = []
install_args = []
# Optional shell snippets. Each runs with `set -euo pipefail` in the
# source directory and overrides that phase of `system`:
# prepare = "patch -p1 < $RECIPE/fix.patch"
# configure = "..."
# compile = "..."
# install = "make DESTDIR=$DESTDIR install"
# post_install = "rm -rf $DESTDIR/usr/share/doc"

[[split]]                   # optional extra packages carved out of the output
name = "liblzma"
summary = "LZMA compression library"
files = ["usr/lib/liblzma.so*", "usr/include/lzma*", "usr/lib/pkgconfig/liblzma.pc"]
depends = []

[package]                   # optional, the main package
provides = []
conflicts = []
replaces = []

[triggers]                  # optional; run by the transaction (Phase 3)
post_install = []
```

## Fields

| Field | Required | Meaning |
|---|---|---|
| `name`, `version`, `release` | yes | identity; the main package has this name |
| `summary`, `license` | yes | shown by `spk info`; license is an SPDX expression |
| `homepage` | no | |
| `source[]` | yes, ≥1 | `url` (`https://` only) + `sha256`; extra mirrors via `mirrors = [...]` |
| `deps.build` | no | dependency specs installed into the sysroot before building |
| `deps.runtime` | no | dependency specs of the main package |
| `build.system` | yes | preset phases (below) |
| `split[]` | no | sub-packages; each file glob claims matching paths first |
| `package.provides/conflicts/replaces` | no | for the solver |

### Dependency specs

`name`, `name>=1.2`, `name>1.2`, `name<=1.2`, `name<1.2`, `name=1.2`,
`name=1.2-3` (pins the release too). Versions compare segment by segment:
runs of digits compare numerically, runs of letters lexically, a `~`
sorts before everything (pre-releases: `1.0~rc1 < 1.0`). Releases compare
numerically after the version.

## Build presets

All presets build out of the unpacked source dir with `PREFIX=/usr`.

| system | configure | compile | install |
|---|---|---|---|
| `autotools` | `./configure --prefix=/usr --sysconfdir=/etc --localstatedir=/var --libdir=/usr/lib <configure_args>` | `make <make_args>` | `make DESTDIR=$DESTDIR install <install_args>` |
| `cmake` | `cmake -S . -B _build -G Ninja -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_BUILD_TYPE=Release <configure_args>` | `ninja -C _build` | `DESTDIR=$DESTDIR ninja -C _build install` |
| `make` | (none) | `make PREFIX=/usr <make_args>` | `make PREFIX=/usr DESTDIR=$DESTDIR install <install_args>` |
| `custom` | every phase must be given as a snippet | | |

## Build environment

`spk build` runs every phase, and the source extraction, inside a
bubblewrap sandbox:

- **Never as root.** If spk runs as uid 0 it drops to the unprivileged
  build user (default `nobody`, 65534) with no supplementary groups
  *before* starting the sandbox. Inside, the build runs as uid 1000 in a
  fresh user namespace with no capabilities.
- **No network.** Sources are fetched and verified by spk *before* the
  sandbox starts; the sandbox has its own empty network namespace.
- Fresh mount, PID, IPC, UTS and cgroup namespaces; dies with spk.
- Filesystem: tmpfs `/`; the build sysroot (build deps installed from
  signed packages) read-only at `/usr`; the toolchain read-only; the
  work dir read-write at `/build`; private `/tmp`, `/proc`, `/dev`.
- Environment: `DESTDIR=/build/dest`, `RECIPE=/build/recipe` (read-only
  copy of the recipe dir, for patches), `MAKEFLAGS=-j<ncpu>`,
  `SOURCE_DATE_EPOCH`, `HOME=/build/home`, `TMPDIR=/tmp`, `LC_ALL=C`.

**Toolchain (Phase 1).** Until Phase 2 builds our own toolchain, the
sandbox uses the toolchain from the Nix devShell: `/nix/store` is bound
read-only and `PATH` is the devShell's. Binaries built this way link
against the host glibc and are not bootable; they exercise the package
pipeline. See DECISIONS.md 2026-10-18 "Phase 1 host toolchain".

## Output normalisation

After `install`, spk checks `DESTDIR` before packaging:

1. `etc/` is moved to `usr/share/factory/etc/` (CLAUDE.md section 4).
2. Libtool `.la` files and `usr/share/info/dir` are removed.
3. Any path outside `usr/` is an error. So are device nodes, FIFOs,
   sockets, and symlinks whose target escapes the package root. Hard
   links are packaged as independent files. setuid/setgid bits on files
   are kept and recorded in the manifest; on directories they are an
   error.
