{ inputs, pkgs, ... }:
let
  pre-commit-check = import ./checks/pre-commit-check.nix { inherit inputs pkgs; };
in
pkgs.mkShell {
  packages = [
    pkgs.cargo
    pkgs.rustc
    pkgs.clippy
    pkgs.rustfmt
    pkgs.rust-analyzer
  ]
  # Runtime clipboard tools for the `y s` / `y b` copy commands. macOS ships
  # `pbcopy`, so these are only needed on Linux (Wayland / X11).
  ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
    pkgs.wl-clipboard
    pkgs.xclip
    pkgs.xsel
  ]
  ++ pre-commit-check.enabledPackages;

  shellHook = ''
    ${pre-commit-check.shellHook}
  '';
}
