{
  description = "seqtable - Blazingly Fast Sequence Count Table generator";

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = import inputs.systems;

      imports = [
        ./nix/packages.nix
        ./nix/overlays.nix
        ./nix/shell.nix
        ./nix/formatter.nix
      ];

      perSystem = {
        lib,
        self',
        system,
        ...
      }: {
        _module.args.pkgs = import inputs.nixpkgs {
          inherit system;
          config.allowUnfree = true;
          overlays = [
            inputs.rust-overlay.overlays.default
          ];
        };

        checks = let
          packages = lib.mapAttrs' (n: lib.nameValuePair "package-${n}") self'.packages;
          devShells = lib.mapAttrs' (n: lib.nameValuePair "devShell-${n}") self'.devShells;
        in
          {inherit (self') formatter;} // packages // devShells;
      };
    };

  inputs = {
    # keep-sorted start
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    flake-parts.url = "github:hercules-ci/flake-parts";
    gitignore.inputs.nixpkgs.follows = "nixpkgs";
    gitignore.url = "github:hercules-ci/gitignore.nix";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
    systems.url = "github:nix-systems/default";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    # keep-sorted end
  };
}
