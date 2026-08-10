{
  perSystem = { crane, pkgs, ... }: {
    devShells.default = crane.lib.devShell {
      packages = with pkgs; [
        nushell

        # formatter stuff
        deno
        nixpkgs-fmt
        taplo

        # cargo         # from crane
        # clippy        # from crane
        # rust-analyzer # from crane
        # rustc         # from crane
        # rustfmt       # from crane

        cargo-hakari
        cargo-nextest

        # fastly
        fastly
        viceroy

        # web
        binaryen
        tailwindcss_4
        trunk
        wasm-bindgen-cli
      ];

      shellHook = ''
        export WEB_DIST="$PWD/crates/howfastly-web/dist"
      '';
    };
  };
}
