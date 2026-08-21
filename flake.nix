{
  description = "baijia-suo — a secure Wayland screen locker with xlockmore animation support";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      forAllSystems = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
      ];
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          baijia-suo = pkgs.callPackage ./nix/package.nix { };
          default = self.packages.${system}.baijia-suo;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.baijia-suo ];
            packages = with pkgs; [
              rustfmt
              clippy
            ];
          };
        }
      );

      overlays.default = final: prev: {
        baijia-suo = final.callPackage ./nix/package.nix { };
      };

      nixosModules.default = import ./nix/module.nix;
    };
}
