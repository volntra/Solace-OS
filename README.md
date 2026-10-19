# Solace

A macOS-style operating system on a minimal Linux base. The desktop is
**Koi**; version 1 is *SolOS 1: Koi*.

- Project brief and rules: [`CLAUDE.md`](CLAUDE.md)
- Where things stand: [`docs/STATUS.md`](docs/STATUS.md)
- Why things are the way they are: [`docs/DECISIONS.md`](docs/DECISIONS.md)

```sh
nix develop      # or: direnv allow
just             # list recipes
just check       # fmt, clippy -D warnings, tests, repo lint
```

## License

Solace's own code is licensed under the GNU General Public License v3.0 or
later; see [`LICENSE`](LICENSE). Upstream packages built from `recipes/`
keep their own licenses.
