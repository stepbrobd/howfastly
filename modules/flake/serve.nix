{ lib, ... }:

{
  perSystem = { config, pkgs, ... }: {
    apps.serve.program = lib.getExe (pkgs.writeShellApplication {
      name = "serve";
      runtimeInputs = [ pkgs.viceroy ];
      text = ''
        exec viceroy serve ${config.legacyPackages.crates.compute}/bin/compute.wasm "$@"
      '';
    });
  };
}
