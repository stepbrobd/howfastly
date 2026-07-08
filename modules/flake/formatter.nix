{
  perSystem = { pkgs, ... }: {
    # find workspace root based on git
    # and call formatting tools (called tools must be put in dev shell)
    formatter = pkgs.writeShellScriptBin "formatter" ''
      set -eoux pipefail
      root="$PWD"
      while [[ ! -f "$root/.git/index" ]]; do
        if [[ "$root" == "/" ]]; then
          exit 1
        fi
        root="$(dirname "$root")"
      done

      pushd "$root" > /dev/null
      shopt -s dotglob

      cargo clippy --all-features -- -D warnings
      cargo fmt --all
      deno fmt **/*.md **/*.yaml
      nixpkgs-fmt .
      taplo format

      shopt -u dotglob
      popd
    '';
  };
}
