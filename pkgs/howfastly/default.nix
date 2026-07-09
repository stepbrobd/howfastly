{ inputs, stdenv }:

inputs.self.legacyPackages.${stdenv.hostPlatform.system}.crates.cli.overrideAttrs {
  pname = "howfastly";
  meta.mainProgram = "howfastly";
}
