_: {
  perSystem = {
    self',
    pkgs,
    ...
  }: {
    apps.benchmark = {
      type = "app";
      program = "${self'.packages.benchmark-script}/bin/seqtable-benchmark";
    };

    packages.benchmark-script = pkgs.writeShellApplication {
      name = "seqtable-benchmark";

      runtimeInputs = with pkgs; [
        hyperfine
        jq
        gawk
        gzip
        bc
        parallel
        seqkit
        self'.packages.seqtable
      ];

      text = builtins.readFile ../scripts/benchmark.sh;
    };
  };
}
