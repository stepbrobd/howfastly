{
  perSystem = { crane, pkgs, ... }: {
    devShells.default = crane.lib.devShell {
      packages = with pkgs; [
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

        # fastly + web
        fastly
        nushell
        tailwindcss_4
        trunk
        viceroy
        wasm-bindgen-cli
      ];

      shellHook = ''
        export WEB_DIST="$PWD/crates/web/dist"
      '';
    };
  };
}
