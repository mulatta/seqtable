_: {
  perSystem = {pkgs, ...}: let
    rustToolchain = pkgs.rust-bin.stable.latest.default.override {
      extensions = ["rust-src" "rust-analyzer"];
    };
  in {
    devShells.default = pkgs.mkShell {
      packages = [
        rustToolchain
        pkgs.pkg-config
      ];
    };
  };
}
