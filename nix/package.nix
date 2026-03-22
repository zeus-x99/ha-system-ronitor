{ lib, rustPlatform }:

rustPlatform.buildRustPackage {
  pname = "ha-system-ronitor";
  version = "0.1.0";
  src = lib.cleanSource ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  meta = {
    description = "Home Assistant system monitor over MQTT";
    homepage = "https://github.com/zeus-x99/ha-system-ronitor";
    mainProgram = "ha-system-ronitor";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
