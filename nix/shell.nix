{
  perSystem = {
    config,
    pkgs,
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
          mold

          # Build tools
          pkg-config
        ]
        ++ [(python3.withPackages (ps: [ps.polars ps.ipython])) config.packages.seqtable];

      shellHook = ''
        export ROOT=$(git rev-parse --show-toplevel)
        export CARGO_HOME="$ROOT/.cargo"
        export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
        export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}";
      '';
    };
  };
}
