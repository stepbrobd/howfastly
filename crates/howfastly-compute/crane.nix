{ crane, pkgs, ... }:

{
  cargoArtifacts = crane.lib.buildDepsOnly (crane.commonArgs // {
    pname = "howfastly-compute";
    cargoExtraArgs = "--package howfastly-compute";
    doCheck = false;
    env.CARGO_BUILD_TARGET = "wasm32-wasip1";
  });

  src = crane.fileSetForCrates [ ../howfastly ../howfastly-compute ];

  env.CARGO_BUILD_TARGET = "wasm32-wasip1";
  # web comes from the flake self overlay
  env.WEB_DIST = "${pkgs.howfastly.web}";
}
