<p align="center"><img src="assets/brand/fern-icon-512.png" width="160" alt="Fern"></p>

# Fern

A macOS-style operating system on a minimal Linux base. The desktop is
**Koi**; version 1 is *Fern 1: Koi*.

- Project brief and rules: [`CLAUDE.md`](CLAUDE.md)
- Where things stand: [`docs/STATUS.md`](docs/STATUS.md)
- Why things are the way they are: [`docs/DECISIONS.md`](docs/DECISIONS.md)

```sh
nix develop      # or: direnv allow
just             # list recipes
just check       # fmt, clippy -D warnings, tests, repo lint
```

## License

Fern's own code is licensed under the GNU General Public License v3.0 or
later; see [`LICENSE`](LICENSE). Upstream packages built from `recipes/`
keep their own licenses.
