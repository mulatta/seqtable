{inputs, ...}: {
  perSystem = {
    self',
    pkgs,
    lib,
    ...
  }: let
    rustToolchain = pkgs.rust-bin.stable.latest.default;
    craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;

    src = craneLib.cleanCargoSource ../.;

    commonArgs = {
      inherit src;
      pname = "seqtable";
      version = "0.2.0";
      nativeBuildInputs = with pkgs; [pkg-config cmake];
    };

    # Build only dependencies (cached separately)
    cargoArtifacts = craneLib.buildDepsOnly commonArgs;
  in {
    packages = {
      seqtable = craneLib.buildPackage (commonArgs
        // {
          inherit cargoArtifacts;
          doCheck = pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform;
          meta = with lib; {
            description = "High-performance parallel FASTQ sequence counter";
            license = licenses.mit;
            platforms = platforms.linux ++ platforms.darwin;
            mainProgram = "seqtable";
          };
        });

      default = self'.packages.seqtable;
    };

    checks = {
      clippy = craneLib.cargoClippy (commonArgs
        // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
        });
    };
  };
}
