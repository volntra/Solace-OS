# Repository format

A repository is a directory, served over HTTPS or read from a local path:

```
index.toml
index.toml.sig
packages/<name>-<version>-<release>.x86_64.fernpkg
packages/<name>-<version>-<release>.x86_64.fernpkg.sig
```

## Index

```toml
format = 1
name = "fern-dev"
generated = 1792368000

[[package]]
name = "liblzma"
version = "5.8.1"
release = 1
arch = "x86_64"
summary = "LZMA compression library"
depends = []
provides = []
conflicts = []
replaces = []
file = "packages/liblzma-5.8.1-1.x86_64.fernpkg"
size = 81234
sha256 = "..."
installed_size = 234567
```

`index.toml.sig` uses the signature format from `package.md` with the
message `b"spk-index-v1\0" || sha256(index.toml bytes)`.

## Rules

- Repositories are fetched only over `https://` or from a local path.
  `http://` URLs are rejected.
- The index signature is verified against the trusted keys **before** the
  index is parsed. An unsigned or badly signed index is refused.
- Packages are then verified by both their index entry (size + sha256)
  and their own signature (`package.md`). Both must pass.
- `spk repo-add <repo> <pkg>...` verifies each package's signature,
  copies it into `packages/`, replaces older entries with the same name,
  and re-signs the index with `--key`.

## Configuration

`<root>/etc/spk/repos.d/<name>.toml`:

```toml
url = "https://repo.example/fern/x86_64"   # or a local path
```

`--repo <url-or-path>` on the command line adds a repository for that
invocation. The trusted keys always come from the root being operated on.
