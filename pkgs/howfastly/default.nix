{ inputs, stdenv }:

inputs.self.legacyPackages.${stdenv.hostPlatform.system}.crates.howfastly.overrideAttrs {
  meta.mainProgram = "howfastly";
}
