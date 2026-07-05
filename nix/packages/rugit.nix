{ pkgs, ... }:
let
  inherit (pkgs) lib;
  cargoToml = lib.importTOML ../../Cargo.toml;
in
pkgs.rustPlatform.buildRustPackage {
  pname = "rugit";
  version = cargoToml.package.version;

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.toml
      ../../Cargo.lock
      ../../src
      ../../tests
    ];
  };

  cargoLock.lockFile = ../../Cargo.lock;

  # integration tests shell out to a real `git` binary
  nativeCheckInputs = [ pkgs.git ];

  meta = {
    description = cargoToml.package.description;
    license = lib.licenses.mit;
    mainProgram = "rugit";
  };
}
