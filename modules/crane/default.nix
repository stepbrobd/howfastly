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

    # crane cannot read a workspace version, versionOf resolves it below
    pname = "howfastly";
    version = crane.versionOf "howfastly";
  };

  # dependencies built once and shared by every crate derivation
  cargoArtifacts = crane.lib.buildDepsOnly crane.commonArgs;

  individualCrateArgs = crane.commonArgs // {
    inherit (crane) cargoArtifacts;
    # tests run in the nextest check, never inside a package build
    doCheck = false;
  };

  fileSetForCrates = crates: lib.fileset.toSource {
    root = ../..;

    # assets hold what the sources embed with include_str
    fileset = lib.fileset.unions ([
      ../../.cargo/config.toml
      ../../Cargo.toml
      ../../Cargo.lock
    ] ++ lib.map crane.lib.fileset.commonCargoSources crates
    ++ lib.map (crate: lib.fileset.maybeMissing (crate + "/assets")) crates);
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
