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
    types
    ;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
  renderedConfig = tomlFormat.generate "ha-system-ronitor-config.toml" cfg.settings;
  configDir = pkgs.runCommand "ha-system-ronitor-config-dir" { } ''
    mkdir -p $out
    ln -s ${renderedConfig} $out/config.toml
  '';
  execStart = lib.concatStringsSep " " (
    [ (lib.getExe cfg.package) ] ++ map lib.escapeShellArg cfg.extraArgs
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
      example = "/run/secrets/ha-system-ronitor.env";
      description = "Optional environment file for secrets such as MQTT password.";
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
          cpu = {
            interval_secs = 1;
            smoothing_window = 5;
            max_silence_secs = 30;
          };
          gpu = {
            interval_secs = 1;
            max_silence_secs = 30;
          };
          memory = {
            interval_secs = 5;
            max_silence_secs = 120;
          };
          disk = {
            interval_secs = 30;
            max_silence_secs = 900;
          };
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
            cpu = {
              interval_secs = 1;
              smoothing_window = 5;
              max_silence_secs = 30;
            };
            gpu = {
              interval_secs = 1;
              max_silence_secs = 30;
            };
            memory = {
              interval_secs = 5;
              max_silence_secs = 120;
            };
            disk = {
              interval_secs = 30;
              max_silence_secs = 900;
            };
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
          sampling.cpu = {
            interval_secs = 1;
            smoothing_window = 5;
            max_silence_secs = 30;
          };
          sampling.gpu = {
            interval_secs = 1;
            max_silence_secs = 30;
          };
          sampling.memory = {
            interval_secs = 5;
            max_silence_secs = 120;
          };
          sampling.disk = {
            interval_secs = 30;
            max_silence_secs = 900;
          };
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
      description = "Configuration rendered to config.toml. Put non-secret settings here; use environmentFile for secrets such as mqtt.password.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion =
          !(attrByPath [ "mqtt" "password" ] null cfg.settings != null && cfg.environmentFile != null);
        message = "Use either services.ha-system-ronitor.settings.mqtt.password or environmentFile, not both.";
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

      environment = {
        HA_MONITOR_CONFIG_DIR = configDir;
      }
      // cfg.extraEnvironment;

      serviceConfig = {
        ExecStart = execStart;
        User = cfg.user;
        Group = cfg.group;
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
      // optionalAttrs (cfg.environmentFile != null) {
        EnvironmentFile = cfg.environmentFile;
      }
      // cfg.serviceConfig;
    };
  };
}
