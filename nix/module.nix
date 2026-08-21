{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.baijia-suo;
in
{
  options.programs.baijia-suo = {
    enable = lib.mkEnableOption "baijia-suo, a secure Wayland screen locker";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "The baijia-suo package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    # Empty attrset gets the NixOS defaults (unix auth + account), which is
    # all baijia-suo uses. Without this it falls back to /etc/pam.d/login.
    security.pam.services.baijia-suo = { };
  };
}
