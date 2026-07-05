{ inputs, pkgs, ... }:
let
  treefmtEval = inputs.treefmt.lib.evalModule pkgs ../treefmt.nix;
in
inputs.git-hooks.lib.${pkgs.stdenv.hostPlatform.system}.run {
  src = inputs.self;

  # Vendor the crate dependencies so the clippy hook can also run inside
  # the sandboxed `nix flake check` derivation (no network, no ~/.cargo).
  settings.rust.check.cargoDeps = pkgs.rustPlatform.importCargoLock {
    lockFile = ../../Cargo.lock;
  };

  hooks = {
    nil.enable = true;
    statix.enable = true;
    treefmt = {
      enable = true;
      package = treefmtEval.config.build.wrapper;
    };
    clippy = {
      enable = true;
      settings = {
        denyWarnings = true;
        extraArgs = "--all-targets";
      };
    };
  };
}
