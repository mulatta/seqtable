{inputs, ...}: {
  perSystem = {
    lib,
    pkgs,
    self',
    ...
  }: {
    packages = {
      seqtable = pkgs.rustPlatform.buildRustPackage {
        pname = "seqtable";
        version = "0.1.1";

        src = inputs.gitignore.lib.gitignoreSource ../.;

        cargoLock.lockFile = ../Cargo.lock;

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        meta = with lib; {
          description = "High-performance parallel FASTA/FASTQ sequence counter";
          homepage = "https://github.com/mulatta/seqtable";
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
