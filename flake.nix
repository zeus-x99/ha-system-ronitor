{
  description = "Home Assistant system monitor published over MQTT";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        lib.genAttrs systems (
          system:
          let
            pkgs = import nixpkgs { inherit system; };
          in
          f pkgs
        );
    in
    {
      overlays.default = final: _: {
        ha-system-ronitor = final.callPackage ./nix/package.nix { };
      };

      packages = forAllSystems (pkgs: {
        default = pkgs.callPackage ./nix/package.nix { };
        ha-system-ronitor = pkgs.callPackage ./nix/package.nix { };
      });

      apps = forAllSystems (
        pkgs:
        let
          package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in
        {
          default = {
            type = "app";
            program = lib.getExe package;
            meta = package.meta;
          };
          ha-system-ronitor = {
            type = "app";
            program = lib.getExe package;
            meta = package.meta;
          };
        }
      );

      nixosModules = {
        default = import ./nix/module.nix { inherit self; };
        ha-system-ronitor = import ./nix/module.nix { inherit self; };
      };

      formatter = forAllSystems (pkgs: pkgs.nixfmt);

      checks = forAllSystems (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      });
    };
}
