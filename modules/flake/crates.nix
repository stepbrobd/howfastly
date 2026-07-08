{
  perSystem = { config, crane, lib, pkgs, ... }: {
    legacyPackages.crates =
      let
        directories = lib.attrNames (
          lib.filterAttrs
            (_: type: type == "directory")
            (lib.readDir ../../crates));

        override = crate:
          let
            file = ../../crates/${crate}/crane.nix;
            imported = if lib.pathExists file then import file else { };
          in
          if lib.isFunction imported
          then imported { inherit config crane lib pkgs; }
          else imported;
      in
      # force export cargo deps, i.e. there must NOT be a crate called drac-deps
      { drac-deps = crane.cargoArtifacts; }
      //
      lib.genAttrs
        # drop crates w/ { disable = true; }
        (lib.filter (crate: !((override crate).disable or false)) directories)
        (crate: crane.builder crate (lib.removeAttrs (override crate) [ "disable" ]));
  };
}
