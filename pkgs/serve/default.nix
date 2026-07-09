{ inputs
, stdenv
, viceroy
, writeShellApplication
}:

let
  # compute is a flake-parts crate output, not part of the overlay
  compute = inputs.self.legacyPackages.${stdenv.hostPlatform.system}.crates.compute;
in
writeShellApplication {
  name = "serve";
  runtimeInputs = [ viceroy ];
  text = ''
    exec viceroy serve ${compute}/bin/compute.wasm "$@"
  '';
}
