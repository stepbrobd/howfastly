{ config, crane, ... }:

{
  cargoArtifacts = crane.lib.buildDepsOnly (crane.commonArgs // {
    pname = "compute";
    cargoExtraArgs = "--package compute";
    doCheck = false;
    env.CARGO_BUILD_TARGET = "wasm32-wasip1";
  });

  src = crane.fileSetForCrates [ ../common ../compute ];

  env.CARGO_BUILD_TARGET = "wasm32-wasip1";
  env.WEB_DIST = "${config.legacyPackages.web}";
}
