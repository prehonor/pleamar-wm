{
  description = "pleamar-wm: a Wayland compositor whose window manager is a pleamar scene";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    pleamar = {
      url = "path:/space/github/devshell/pleamar";
      # url = "github:prehonor/pleamar/no-layouts";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    marea = {
      url = "path:/space/github/devshell/marea-plm";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.pleamar.follows = "pleamar";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      pleamar,
      marea,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (
        system: pkgs: rec {
          pleamar-wm = pkgs.callPackage ./nix/package.nix { pleamarSrc = pleamar.outPath; };
          default = pleamar-wm;
        }
      );

      # `nix run github:k4ditano/pleamar-wm -- headless` (or `session` from a TTY)
      apps = forAll (
        system: _pkgs: {
          default = {
            type = "app";
            program = "${self.packages.${system}.pleamar-wm}/bin/pleamar-wm";
          };
        }
      );

      # Development shell

      devShells = forAll (system: pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.pleamar-wm ];
        };
      });

      # NixOS: `programs.pleamar-wm.enable = true;`
      nixosModules.default = import ./nix/nixos.nix self;
    };
}