{
  description = "virtual environments";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    devshell.url = "github:numtide/devshell";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = {
    self,
    flake-utils,
    devshell,
    nixpkgs,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: {
      devShells.default = let
        pkgs = import nixpkgs {
          inherit system;

          overlays = [devshell.overlays.default (import rust-overlay)];
        };
      in
        pkgs.devshell.mkShell {
          language.rust = let
            rust-toolchain = {
              toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);
            };
          in {
            packageSet = rust-toolchain;
            tools = [
              "toolchain"
            ];
          };
          
          packages = with pkgs; [
            libllvm
            lit
            clang
          ];

          imports = [
            "${devshell}/extra/language/rust.nix"
            (pkgs.devshell.importTOML ./devshell.toml)
          ];
        };
    });
}
