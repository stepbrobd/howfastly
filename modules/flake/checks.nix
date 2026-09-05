{ inputs, ... }:

{
  perSystem = { config, crane, lib, pkgs, ... }:
    let
      crate = name: ../../crates/${name};
      workspace = crane.fileSetForCrates (lib.map crate [
        "howfastly"
        "howfastly-compute"
        "howfastly-map"
        "howfastly-web"
      ]);
      web = pkgs.howfastly.web;
      compute = config.legacyPackages.crates.howfastly-compute;

      # the same dependency build the compute package links against
      computeDeps = crane.lib.buildDepsOnly (crane.commonArgs // {
        pname = "howfastly-compute";
        cargoExtraArgs = "--package howfastly-compute";
        doCheck = false;
        env.CARGO_BUILD_TARGET = "wasm32-wasip1";
      });

      # a tool that only reads the tree
      over = name: tools: command: pkgs.runCommand "howfastly-${name}" { nativeBuildInputs = tools; } ''
        export HOME="$TMPDIR"
        cd ${inputs.self}
        ${command}
        touch "$out"
      '';
    in
    {
      checks = {
        fmt = crane.lib.cargoFmt (crane.commonArgs // { src = workspace; });
        taplo = over "taplo" [ pkgs.taplo ] "taplo fmt --check";
        nixpkgs-fmt = over "nixpkgs-fmt" [ pkgs.nixpkgs-fmt ] "nixpkgs-fmt --check .";

        clippy = crane.lib.cargoClippy (crane.individualCrateArgs // {
          src = workspace;
          cargoClippyExtraArgs = "--workspace --all-targets -- -D warnings";
        });

        # the compute modules only exist for wasm32, the host lint never sees them
        # clippy runs locked, so the whole workspace must be present for the lock to match
        clippy-compute = crane.lib.cargoClippy (crane.commonArgs // {
          pname = "howfastly-compute";
          cargoArtifacts = computeDeps;
          src = workspace;
          cargoClippyExtraArgs = "--package howfastly-compute -- -D warnings";
          env.CARGO_BUILD_TARGET = "wasm32-wasip1";
          env.WEB_DIST = "${web}";
        });

        clippy-web = crane.lib.cargoClippy {
          pname = "howfastly-web";
          version = crane.versionOf "howfastly-web";
          inherit (web) cargoArtifacts;
          src = workspace;
          strictDeps = true;
          cargoClippyExtraArgs = "--package howfastly-web --all-targets -- -D warnings";
          env.CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
          env.CARGO_PROFILE = "wasm";
        };

        # the compute crate has nothing but the e2e, which runs below with its tools
        test = crane.lib.cargoNextest (crane.individualCrateArgs // {
          src = workspace;
          cargoNextestExtraArgs = "--workspace --exclude howfastly-compute";
        });

        e2e = pkgs.runCommand "howfastly-e2e"
          {
            nativeBuildInputs = with pkgs; [ curl nushell viceroy ];
            # viceroy binds the loopback address, the darwin sandbox forbids that by default
            __darwinAllowLocalNetworking = true;
          } ''
          # viceroy exits without a native certificate bundle even though nothing leaves the sandbox
          export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt
          export HOWFASTLY_WASM=${compute}/bin/howfastly-compute.wasm
          export HOWFASTLY_CONFIG=${../../fastly.toml}
          nu ${../../crates/howfastly-compute/tests/e2e.nu}
          touch "$out"
        '';
      };
    };
}
