{
  description = "Solace: a macOS-style operating system on a minimal Linux base";

  # Channel tarball rather than github: so locking works without the GitHub
  # API; flake.lock still pins the exact revision by hash.
  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz";

  outputs =
    { self, nixpkgs }:
    let
      # The development host is x86_64 NixOS; so is the only target.
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        name = "solace-dev";

        packages = with pkgs; [
          # Rust (every component written in Rust)
          cargo
          rustc
          clippy
          rustfmt
          rust-analyzer

          # Build drivers
          just
          git
          pkg-config
          cmake
          ninja
          ccache

          # Package builds (Phase 1): sandbox, archive format, and the build
          # tools recipes expect besides stdenv (the sandbox sees only
          # /nix/store, never the host's /usr/bin).
          bubblewrap
          zstd
          perl
          gnum4
          bison
          python3

          # Brand assets: tools/brand/gen-logo.py (python3, above) + SVG
          # rendering (resvg is also the icon engine's renderer, CLAUDE.md 6.9)
          resvg

          # Images and boot (Phase 2+): QEMU + UEFI firmware, ESP + btrfs tooling
          qemu
          OVMF
          grub2_efi
          dosfstools
          mtools
          btrfs-progs
          cryptsetup
          util-linux
        ];

        # Paths the image/ scripts need without guessing at store paths.
        OVMF_FV = "${pkgs.OVMF.fd}/FV";
        GRUB_EFI_LIB = "${pkgs.grub2_efi}/lib/grub/x86_64-efi";
        RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

        shellHook = ''
          export CCACHE_DIR="$PWD/.ccache"
          echo "Solace dev shell. Run 'just' for recipes, 'just check' to verify."
        '';
      };

      packages.${system}.solcheck = pkgs.rustPlatform.buildRustPackage {
        pname = "solcheck";
        version = "0.1.0";
        src = self;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "solcheck" ];
        cargoTestFlags = [ "-p" "solcheck" ];
      };

      checks.${system}.solcheck = self.packages.${system}.solcheck;

      formatter.${system} = pkgs.nixfmt;
    };
}
