{ inputs }:

let inherit (inputs.nixpkgs) lib; in

pkgs: # pass from call site

lib.fix (crane: {
  toolchain = with inputs.fenix.packages.${pkgs.stdenv.hostPlatform.system}; combine (lib.flatten [
    (with stable; [
      cargo
      clippy
      rust-analyzer
      rust-src
      rust-std
      rustc
      rustfmt
    ])

    (with targets.wasm32-unknown-unknown.stable; [ rust-std ])

    (with targets.wasm32-wasip1.stable; [ rust-std ])
  ]);

  lib = (inputs.crane.mkLib pkgs).overrideToolchain crane.toolchain;

  src = crane.lib.cleanCargoSource inputs.self.outPath;

  commonArgs = {
    inherit (crane) src;
    strictDeps = true;
    __structuredAttrs = true;

    # crane cant infer pname/version
    # set a placeholder and override in per crate drv
    pname = "howfastly";
    version = "2001.717.0";
  };

  # pre-build/cache deps
  cargoArtifacts = crane.lib.buildDepsOnly crane.commonArgs;

  individualCrateArgs = crane.commonArgs // {
    inherit (crane) cargoArtifacts;
    # test with cargo-nextest
    doCheck = false;
  };

  fileSetForCrates = crates: lib.fileset.toSource {
    root = ../..;

    fileset = lib.fileset.unions ([
      ../../Cargo.toml
      ../../Cargo.lock
    ] ++ lib.map crane.lib.fileset.commonCargoSources crates);
  };

  builder = crate: override: crane.lib.buildPackage (
    crane.individualCrateArgs
    //
    {
      pname = crate;
      inherit (crane.lib.crateNameFromCargoToml {
        cargoToml = ../../crates/${crate}/Cargo.toml;
      }) version;

      cargoExtraArgs = "--package ${crate}";

      src = crane.fileSetForCrates [ ../../crates/${crate} ];
    }
    //
    override
  );
})
