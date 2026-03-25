{
  perSystem = {pkgs, ...}: {
    devShells.default = pkgs.mkShell {
      packages = with pkgs; [
        # Rust toolchain
        cargo
        rustc
        rustfmt
        clippy
        rust-analyzer

        # Build tools
        pkg-config
      ];

      shellHook = ''
        export ROOT=$(git rev-parse --show-toplevel)
        export CARGO_HOME="$ROOT/.cargo"
        export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}";
      '';
    };
  };
}
