{ inputs }:

let inherit (inputs.nixpkgs) lib; in

pkgs: # pass from call site

lib.fix (crane: {
  toolchain = pkgs.rust-bin.stable.latest.minimal.override {
    extensions = [ "clippy" "rust-analyzer" "rust-src" "rustfmt" ];
    targets = [ "wasm32-unknown-unknown" "wasm32-wasip1" ];
  };

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
      ../../.cargo/config.toml
      ../../Cargo.toml
      ../../Cargo.lock
    ] ++ lib.map crane.lib.fileset.commonCargoSources crates);
  };

  # crateNameFromCargoToml parses a manifest literally, so a crate inheriting
  # version.workspace = true reads back as an attrset and crane falls to 0.0.1
  # take the literal when there is one and defer to the workspace otherwise
  versionOf = crate:
    let version = (lib.importTOML ../../crates/${crate}/Cargo.toml).package.version or null; in
    if lib.isString version
    then version
    else (lib.importTOML ../../Cargo.toml).workspace.package.version;

  builder = crate: override: crane.lib.buildPackage (
    crane.individualCrateArgs
    //
    {
      pname = crate;
      version = crane.versionOf crate;

      cargoExtraArgs = "--package ${crate}";

      src = crane.fileSetForCrates [ ../../crates/${crate} ];
    }
    //
    override
  );
})
