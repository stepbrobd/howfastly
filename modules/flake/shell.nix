{
  perSystem = { crane, pkgs, ... }: {
    devShells.default = crane.lib.devShell {
      packages = with pkgs; [
        # formatter stuff
        deno
        nixpkgs-fmt
        taplo

        # cargo   # from crane
        # clippy  # from crane
        # rustc   # from crane
        # rustfmt # from crane
        cargo-hakari
        cargo-nextest
        rust-analyzer
      ];
    };
  };
}
