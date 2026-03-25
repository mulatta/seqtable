{
  perSystem = {
    config,
    pkgs,
    lib,
    ...
  }: {
    devShells.default = pkgs.mkShell {
      packages = with pkgs;
        [
          # Rust DevDeps
          cargo
          rustc
          rustfmt
          clippy
          rust-analyzer
          cargo-flamegraph
          cargo-bloat
          just

          # Build tools
          pkg-config
        ]
        ++ lib.optionals stdenv.isLinux [mold]
        ++ [(python3.withPackages (ps: [ps.polars ps.ipython])) config.packages.seqtable];

      shellHook =
        ''
          export ROOT=$(git rev-parse --show-toplevel)
          export CARGO_HOME="$ROOT/.cargo"
          export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}";
        ''
        + lib.optionalString pkgs.stdenv.isLinux ''
          export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
        '';
    };
  };
}
