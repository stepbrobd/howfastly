{ callPackage, inputs, lib, pkgsFinal, stdenv }:

inputs.self.legacyPackages.${stdenv.hostPlatform.system}.crates.howfastly.overrideAttrs (old: {
  meta.mainProgram = "howfastly";

  # the dist is a trunk artifact rather than a crate output
  # hang it off the cli so it stays out of the top level package set
  passthru = (old.passthru or { }) // {
    web = callPackage ./web.nix { inherit lib pkgsFinal; };
  };
})
