{ lib
, pkgsFinal
, tailwindcss_4
, wasm-bindgen-cli
}:

let
  crane = lib.crane.mkLib pkgsFinal;

  inherit (crane.lib.crateNameFromCargoToml {
    cargoToml = ../../crates/web/Cargo.toml;
  }) version;

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.lock
      ../../Cargo.toml
      (lib.fileset.difference
        ../../crates/web
        (lib.fileset.maybeMissing ../../crates/web/dist))
      (crane.lib.fileset.commonCargoSources ../../crates/common)
    ];
  };
in
crane.lib.buildTrunkPackage {
  pname = "web";
  inherit src version;

  cargoArtifacts = crane.lib.buildDepsOnly {
    pname = "web";
    inherit src version;
    strictDeps = true;
    cargoExtraArgs = "--package web";
    doCheck = false;
    env.CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
  };

  strictDeps = true;
  cargoExtraArgs = "--package web";
  inherit wasm-bindgen-cli;
  nativeBuildInputs = [ tailwindcss_4 ];

  # trunk resolves the target crate from cwd
  # the workspace root is a virtual manifest
  buildPhaseCargoCommand = ''
    (cd crates/web && trunk build --release=true index.html)
  '';
  installPhaseCommand = ''
    cp -r crates/web/dist $out
  '';
}
