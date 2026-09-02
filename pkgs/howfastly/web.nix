{ lib
, pkgsFinal
, binaryen
, tailwindcss_4
, wasm-bindgen-cli
}:

let
  crane = lib.crane.mkLib pkgsFinal;

  version = crane.versionOf "howfastly-web";

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.lock
      ../../Cargo.toml
      (lib.fileset.difference
        ../../crates/howfastly-web
        (lib.fileset.maybeMissing ../../crates/howfastly-web/dist))
      (crane.lib.fileset.commonCargoSources ../../crates/howfastly)
      (crane.lib.fileset.commonCargoSources ../../crates/howfastly-map)
      ../../crates/howfastly-map/assets
    ];
  };

  cargoArtifacts = crane.lib.buildDepsOnly {
    pname = "howfastly-web";
    inherit src version;
    strictDeps = true;
    cargoExtraArgs = "--package howfastly-web";
    doCheck = false;
    env.CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
  };
in
crane.lib.buildTrunkPackage {
  pname = "howfastly-web";
  inherit src version cargoArtifacts;

  # the clippy check lints against the same dependencies
  passthru = { inherit cargoArtifacts; };

  strictDeps = true;
  cargoExtraArgs = "--package howfastly-web";
  inherit wasm-bindgen-cli;
  nativeBuildInputs = [ binaryen tailwindcss_4 ];

  # trunk resolves the target crate from cwd
  # the workspace root is a virtual manifest
  buildPhaseCargoCommand = ''
    (cd crates/howfastly-web && trunk build --release=true index.html)
  '';
  installPhaseCommand = ''
    cp -r crates/howfastly-web/dist $out
  '';
}
