{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.ha-system-ronitor;
  tomlFormat = pkgs.formats.toml { };
  inherit (lib)
    attrByPath
    literalExpression
    mkEnableOption
    mkIf
    mkOption
    optionalAttrs
    optionalString
    recursiveUpdate
    types
    ;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
  mqttPasswordPlaceholder = "__HA_SYSTEM_RONITOR_MQTT_PASSWORD__";
  renderedSettings =
    if cfg.mqttPasswordFile == null then
      cfg.settings
    else
      recursiveUpdate cfg.settings {
        mqtt.password = mqttPasswordPlaceholder;
      };
  renderedConfig = tomlFormat.generate "ha-system-ronitor-config.toml" renderedSettings;
  storeConfigDir = pkgs.runCommand "ha-system-ronitor-config-dir" { } ''
    mkdir -p $out
    ln -s ${renderedConfig} $out/config.toml
  '';
  runtimeConfigDir = "/run/ha-system-ronitor";
  effectiveConfigDir = if cfg.mqttPasswordFile == null then storeConfigDir else runtimeConfigDir;
  execStart = lib.concatStringsSep " " (
    [
      (lib.getExe cfg.package)
      "--config-dir"
      effectiveConfigDir
    ]
    ++ map lib.escapeShellArg cfg.extraArgs
  );
in
{
  options.services.ha-system-ronitor = {
    enable = mkEnableOption "ha-system-ronitor MQTT system monitor";

    package = mkOption {
      type = types.package;
      default = defaultPackage;
      defaultText = literalExpression "inputs.ha-system-ronitor.packages.${pkgs.system}.default";
      description = "Package used for the service.";
    };

    user = mkOption {
      type = types.str;
      default = "ha-system-ronitor";
      description = "User account for the service.";
    };

    group = mkOption {
      type = types.str;
      default = "ha-system-ronitor";
      description = "Group for the service.";
    };

    createUser = mkOption {
      type = types.bool;
      default = true;
      description = "Whether to create the service user and group automatically.";
    };

    environmentFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/run/secrets/ha-system-ronitor-runtime.env";
      description = "Optional environment file for runtime environment variables such as RUST_LOG or HA_MONITOR_PAWNIO_AUTO_INSTALL.";
    };

    mqttPasswordFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      example = "/run/secrets/ha-system-ronitor-mqtt-password";
      description = "Optional file containing only the MQTT password. This avoids storing the password in the Nix store.";
    };

    extraEnvironment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      description = "Additional environment variables passed to the service.";
    };

    extraArgs = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Additional CLI arguments passed to the binary.";
    };

    path = mkOption {
      type = types.listOf types.package;
      default = [
        pkgs.systemd
        pkgs.util-linux
      ];
      description = "Extra PATH entries for helper commands such as systemctl.";
    };

    after = mkOption {
      type = types.listOf types.str;
      default = [ "network-online.target" ];
      description = "Additional systemd units ordered before this service.";
    };

    wants = mkOption {
      type = types.listOf types.str;
      default = [ "network-online.target" ];
      description = "Additional systemd units wanted by this service.";
    };

    wantedBy = mkOption {
      type = types.listOf types.str;
      default = [ "multi-user.target" ];
      description = "Targets that should start this service.";
    };

    serviceConfig = mkOption {
      type = types.attrsOf types.anything;
      default = { };
      description = "Extra systemd.serviceConfig overrides merged into the unit.";
    };

    settings = mkOption {
      type = tomlFormat.type;
      default = {
        mqtt.port = 1883;
        home_assistant = {
          discovery_prefix = "homeassistant";
          status_topic = "homeassistant/status";
          topic_prefix = "monitor/system";
        };
        sampling = {
          cpu.interval_secs = 1;
          gpu.interval_secs = 1;
          memory.interval_secs = 5;
          network.interval_secs = 1;
          uptime.interval_secs = 300;
          disk.interval_secs = 30;
        };
        thresholds = {
          cpu.usage_pct = 1.0;
          gpu = {
            usage_pct = 1.0;
            memory_change_mib = 8;
          };
          memory.change_mib = 8;
          disk.change_mib = 32;
        };
        shutdown = {
          enable_button = false;
          payload = "shutdown";
          dry_run = false;
        };
      };
      defaultText = literalExpression ''
        {
          mqtt.port = 1883;
          home_assistant = {
            discovery_prefix = "homeassistant";
            status_topic = "homeassistant/status";
            topic_prefix = "monitor/system";
          };
          sampling = {
            cpu.interval_secs = 1;
            gpu.interval_secs = 1;
            memory.interval_secs = 5;
            network.interval_secs = 1;
            uptime.interval_secs = 300;
            disk.interval_secs = 30;
          };
          thresholds = {
            cpu.usage_pct = 1.0;
            gpu = {
              usage_pct = 1.0;
              memory_change_mib = 8;
            };
            memory.change_mib = 8;
            disk.change_mib = 32;
          };
          shutdown = {
            enable_button = false;
            payload = "shutdown";
            dry_run = false;
          };
        }
      '';
      example = literalExpression ''
        {
          mqtt = {
            host = "127.0.0.1";
            port = 1883;
            username = "homeassistant";
          };
          home_assistant = {
            discovery_prefix = "homeassistant";
            status_topic = "homeassistant/status";
            topic_prefix = "monitor/system";
          };
          device = {
            node_id = "router";
            name = "Router System Monitor";
          };
          network.include_interfaces = [ "Ethernet" "Wi-Fi" ];
          sampling.cpu.interval_secs = 1;
          sampling.gpu.interval_secs = 1;
          sampling.memory.interval_secs = 5;
          sampling.network.interval_secs = 1;
          sampling.uptime.interval_secs = 300;
          sampling.disk.interval_secs = 30;
          thresholds.cpu.usage_pct = 1.0;
          thresholds.gpu = {
            usage_pct = 1.0;
            memory_change_mib = 8;
          };
          thresholds.memory.change_mib = 8;
          thresholds.disk.change_mib = 32;
          shutdown = {
            enable_button = false;
            payload = "shutdown";
            dry_run = false;
          };
        }
      '';
      description = "Configuration rendered to config.toml. This is the only application configuration source.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion =
          !(attrByPath [ "mqtt" "password" ] null cfg.settings != null && cfg.mqttPasswordFile != null);
        message = "Use either services.ha-system-ronitor.settings.mqtt.password or services.ha-system-ronitor.mqttPasswordFile, not both.";
      }
    ];

    users.users = mkIf cfg.createUser {
      ${cfg.user} = {
        isSystemUser = true;
        group = cfg.group;
      };
    };

    users.groups = mkIf cfg.createUser {
      ${cfg.group} = { };
    };

    systemd.services.ha-system-ronitor = {
      description = "Home Assistant system monitor";
      inherit (cfg)
        after
        wants
        wantedBy
        path
        ;

      environment = cfg.extraEnvironment;

      preStart = optionalString (cfg.mqttPasswordFile != null) ''
        install -d -m 0750 ${runtimeConfigDir}
        cp ${renderedConfig} ${runtimeConfigDir}/config.toml
        ${pkgs.python3}/bin/python - <<'PY'
from pathlib import Path
import json

config_path = Path("${runtimeConfigDir}/config.toml")
password_path = Path("${cfg.mqttPasswordFile}")
placeholder = "\"${mqttPasswordPlaceholder}\""

password = password_path.read_text(encoding="utf-8").rstrip("\r\n")
content = config_path.read_text(encoding="utf-8")
config_path.write_text(content.replace(placeholder, json.dumps(password)), encoding="utf-8")
PY
      '';

      serviceConfig = {
        ExecStart = execStart;
        User = cfg.user;
        Group = cfg.group;
        PermissionsStartOnly = cfg.mqttPasswordFile != null;
        AmbientCapabilities = [ "CAP_PERFMON" ];
        CapabilityBoundingSet = [ "CAP_PERFMON" ];
        Restart = "always";
        RestartSec = "5s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectHome = true;
        ProtectSystem = "strict";
        ProtectClock = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
      }
      // optionalAttrs (cfg.mqttPasswordFile != null) {
        RuntimeDirectory = "ha-system-ronitor";
        RuntimeDirectoryMode = "0750";
      }
      // optionalAttrs (cfg.environmentFile != null) {
        EnvironmentFile = cfg.environmentFile;
      }
      // cfg.serviceConfig;
    };
  };
}
