{
  perSystem = {
    self',
    pkgs,
    lib,
    ...
  }: let
    inherit (pkgs.stdenv) isLinux;
    linkerArgs = lib.optionalString isLinux "-C link-arg=-fuse-ld=mold";
  in {
    packages = {
      seqtable = pkgs.rustPlatform.buildRustPackage {
        pname = "seqtable";
        version = "0.1.0";

        src = lib.cleanSourceWith {
          src = ../.;
          filter = path: _type: let
            baseName = baseNameOf path;
          in
            !(lib.hasSuffix ".nix" baseName)
            && baseName != "target"
            && baseName != "result";
        };

        cargoLock = {
          lockFile = ../Cargo.lock;
        };

        nativeBuildInputs = with pkgs;
          [
            pkg-config
          ]
          ++ lib.optionals isLinux [mold];

        RUSTFLAGS = lib.concatStringsSep " " (
          lib.filter (x: x != "") [
            linkerArgs
          ]
        );

        # Parallel jobs
        buildPhase = ''
          runHook preBuild
          export CARGO_BUILD_JOBS=$NIX_BUILD_CORES
          cargo build --release
          runHook postBuild
        '';

        installPhase = ''
          runHook preInstall
          install -Dm755 target/release/seqtable $out/bin/seqtable
          runHook postInstall
        '';

        # Tests
        checkPhase = ''
          runHook preCheck
          cargo test --release
          runHook postCheck
        '';

        doCheck = true;

        meta = with lib; {
          description = "High-performance parallel FASTA/FASTQ sequence counter";
          longDescription = ''
            A blazingly fast sequence counter for FASTA/FASTQ/FASTQ.gz files
            with parallel processing support and Parquet output format.
          '';
          license = licenses.mit;
          maintainers = [];
          platforms = platforms.linux ++ platforms.darwin;
          mainProgram = "seqtable";
        };
      };

      default = self'.packages.seqtable;
    };
  };
}
