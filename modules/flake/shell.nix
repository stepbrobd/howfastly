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

        cargo-audit
        cargo-nextest
        cargo-outdated

        # fastly
        fastly
        viceroy

        # web
        binaryen
        tailwindcss_4
        trunk
        wasm-bindgen-cli
      ];

      # the compute embeds both at build time, the dist from the tree and the cells from nix
      shellHook = ''
        export WEB_DIST="$PWD/crates/howfastly-web/dist"
        export CELLS="${pkgs.howfastly.cells}"
      '';
    };
  };
}
