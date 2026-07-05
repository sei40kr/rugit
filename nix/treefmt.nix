_: {
  projectRootFile = "flake.nix";

  programs.nixfmt.enable = true;
  programs.rustfmt = {
    enable = true;
    edition = "2021"; # keep in sync with Cargo.toml
  };
}
