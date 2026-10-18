# Package format (`.solpkg`)

A system package is two files:

```
<name>-<version>-<release>.x86_64.solpkg       tar, zstd-compressed
<name>-<version>-<release>.x86_64.solpkg.sig   detached signature
```

## Archive

A POSIX (ustar/pax) tar stream compressed with zstd. Entries, in order:

1. `.MANIFEST`: the manifest (TOML, below). Always first.
2. The payload: every path listed in the manifest, sorted, relative
   (`usr/bin/xz`), never absolute, never containing `..`.

Archives are reproducible: entries sorted, owner `0:0`, mtime =
`SOURCE_DATE_EPOCH` of the build.

## Manifest

```toml
format = 1
name = "xz"
version = "5.8.1"
release = 1
arch = "x86_64"
summary = "XZ/LZMA compression tools"
license = "0BSD AND GPL-2.0-or-later"
depends = ["liblzma=5.8.1-1"]
provides = []
conflicts = []
replaces = []
build_date = 1792368000          # SOURCE_DATE_EPOCH
recipe_sha256 = "..."            # hash of the recipe dir that produced it
installed_size = 123456

[[files]]
path = "usr/bin/xz"
kind = "file"                    # file | dir | symlink
mode = 0o755                     # stored as an integer
size = 87432
sha256 = "..."

[[files]]
path = "usr/bin/unxz"
kind = "symlink"
target = "xz"
```

## Signature

`.sig` is a small text file:

```
spk-signature 1
key <key-id>
sig <base64 ed25519 signature, 64 bytes>
```

- `key-id` = the first 16 hex chars of `sha256(public key)`.
- The signed message is `b"spk-package-v1\0" || sha256(archive bytes)`.
  The domain prefix stops a package signature from ever being accepted as
  an index signature, and the reverse.

## Verification (security-critical)

Install, and the build sysroot setup, perform these steps in this order
and refuse on any failure. **No flag skips verification.**

1. The `.sig` exists, parses, and names a key in the trusted key set:
   `<root>/usr/share/spk/keys/*.pub` (shipped with the system) plus
   `<root>/etc/spk/keys/*.pub` (added by the administrator).
2. The ed25519 signature over the archive hash verifies (`verify_strict`).
3. If the package came from a repo, its sha256 and size match the
   signed index entry.
4. Only then is the archive decompressed. `.MANIFEST` must be the
   first entry and must describe the same name/version/release as the
   index entry and the file name.
5. Every payload entry must be listed in the manifest with the same kind,
   mode, size, sha256, or symlink target; every manifest entry must be
   present; paths must be relative, normalised, under `usr/`, and unique.
   Payload files are written only into a fresh staging directory, never
   through a symlink.

## Keys

`spk key gen <name>` writes `<name>.key` (secret, mode 0600: `spk-seckey 1` + base64 32-byte seed)
and `<name>.pub` (`spk-pubkey 1` + base64 public key) files.
`spk key trust <file.pub> --root R` copies a public key into
`R/etc/spk/keys/`.
