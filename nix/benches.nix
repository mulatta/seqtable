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
        coreutils
        gnugrep
        gawk
        gzip
        bc
        hyperfine
        jq
        parallel
        seqkit
        util-linux # column
        self'.packages.seqtable
      ];

      text = builtins.readFile ../benches/benchmark.sh;
    };
  };
}
