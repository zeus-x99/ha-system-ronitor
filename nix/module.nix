{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.ha-system-ronitor;
  inherit (lib)
    literalExpression
    mkEnableOption
    mkIf
    mkOption
    optionalAttrs
    types
    ;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
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

    mqtt = {
      host = mkOption {
        type = types.str;
        default = "127.0.0.1";
        description = "MQTT broker host.";
      };

      port = mkOption {
        type = types.port;
        default = 1883;
        description = "MQTT broker port.";
      };

      username = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Optional MQTT username.";
      };

      password = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Optional MQTT password. Prefer environmentFile for secrets.";
      };
    };

    discoveryPrefix = mkOption {
      type = types.str;
      default = "homeassistant";
      description = "Home Assistant MQTT discovery prefix.";
    };

    homeAssistantStatusTopic = mkOption {
      type = types.str;
      default = "homeassistant/status";
      description = "Home Assistant MQTT birth topic.";
    };

    topicPrefix = mkOption {
      type = types.str;
      default = "monitor/system";
      description = "Prefix for state and availability topics.";
    };

    nodeId = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Stable Home Assistant node id. Defaults to hostname when unset.";
    };

    deviceName = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Device name shown in Home Assistant.";
    };

    enableShutdownButton = mkOption {
      type = types.bool;
      default = false;
      description = "Expose a Home Assistant shutdown button.";
    };

    shutdownPayload = mkOption {
      type = types.str;
      default = "shutdown";
      description = "Expected MQTT payload for the shutdown button.";
    };

    shutdownDryRun = mkOption {
      type = types.bool;
      default = false;
      description = "Log shutdown requests without powering off the host.";
    };

    cpuIntervalSecs = mkOption {
      type = types.ints.positive;
      default = 1;
      description = "CPU publish interval in seconds.";
    };

    gpuIntervalSecs = mkOption {
      type = types.ints.positive;
      default = 1;
      description = "GPU publish interval in seconds.";
    };

    memoryIntervalSecs = mkOption {
      type = types.ints.positive;
      default = 5;
      description = "Memory publish interval in seconds.";
    };

    diskIntervalSecs = mkOption {
      type = types.ints.positive;
      default = 30;
      description = "Disk publish interval in seconds.";
    };

    cpuChangeThresholdPct = mkOption {
      type = types.float;
      default = 1.0;
      description = "Minimum CPU usage change before publishing.";
    };

    gpuUsageChangeThresholdPct = mkOption {
      type = types.float;
      default = 1.0;
      description = "Minimum GPU usage change before publishing.";
    };

    gpuMemoryChangeThresholdMiB = mkOption {
      type = types.ints.positive;
      default = 8;
      description = "Minimum GPU memory delta before publishing.";
    };

    memoryChangeThresholdMiB = mkOption {
      type = types.ints.positive;
      default = 8;
      description = "Minimum memory delta before publishing.";
    };

    diskChangeThresholdMiB = mkOption {
      type = types.ints.positive;
      default = 32;
      description = "Minimum disk delta before publishing.";
    };

    cpuSmoothingWindow = mkOption {
      type = types.ints.positive;
      default = 5;
      description = "CPU smoothing sample window size.";
    };

    cpuMaxSilenceSecs = mkOption {
      type = types.ints.positive;
      default = 30;
      description = "Force CPU publish after this silence window.";
    };

    gpuMaxSilenceSecs = mkOption {
      type = types.ints.positive;
      default = 30;
      description = "Force GPU publish after this silence window.";
    };

    memoryMaxSilenceSecs = mkOption {
      type = types.ints.positive;
      default = 120;
      description = "Force memory publish after this silence window.";
    };

    diskMaxSilenceSecs = mkOption {
      type = types.ints.positive;
      default = 900;
      description = "Force disk publish after this silence window.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = !(cfg.mqtt.password != null && cfg.environmentFile != null);
        message = "Use either services.ha-system-ronitor.mqtt.password or environmentFile, not both.";
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
        HA_MONITOR_MQTT_HOST = cfg.mqtt.host;
        HA_MONITOR_MQTT_PORT = builtins.toString cfg.mqtt.port;
        HA_MONITOR_DISCOVERY_PREFIX = cfg.discoveryPrefix;
        HA_MONITOR_HOME_ASSISTANT_STATUS_TOPIC = cfg.homeAssistantStatusTopic;
        HA_MONITOR_TOPIC_PREFIX = cfg.topicPrefix;
        HA_MONITOR_ENABLE_SHUTDOWN_BUTTON = lib.boolToString cfg.enableShutdownButton;
        HA_MONITOR_SHUTDOWN_PAYLOAD = cfg.shutdownPayload;
        HA_MONITOR_SHUTDOWN_DRY_RUN = lib.boolToString cfg.shutdownDryRun;
        HA_MONITOR_CPU_INTERVAL_SECS = builtins.toString cfg.cpuIntervalSecs;
        HA_MONITOR_GPU_INTERVAL_SECS = builtins.toString cfg.gpuIntervalSecs;
        HA_MONITOR_MEMORY_INTERVAL_SECS = builtins.toString cfg.memoryIntervalSecs;
        HA_MONITOR_DISK_INTERVAL_SECS = builtins.toString cfg.diskIntervalSecs;
        HA_MONITOR_CPU_CHANGE_THRESHOLD_PCT = builtins.toString cfg.cpuChangeThresholdPct;
        HA_MONITOR_GPU_USAGE_CHANGE_THRESHOLD_PCT = builtins.toString cfg.gpuUsageChangeThresholdPct;
        HA_MONITOR_GPU_MEMORY_CHANGE_THRESHOLD_MIB = builtins.toString cfg.gpuMemoryChangeThresholdMiB;
        HA_MONITOR_MEMORY_CHANGE_THRESHOLD_MIB = builtins.toString cfg.memoryChangeThresholdMiB;
        HA_MONITOR_DISK_CHANGE_THRESHOLD_MIB = builtins.toString cfg.diskChangeThresholdMiB;
        HA_MONITOR_CPU_SMOOTHING_WINDOW = builtins.toString cfg.cpuSmoothingWindow;
        HA_MONITOR_CPU_MAX_SILENCE_SECS = builtins.toString cfg.cpuMaxSilenceSecs;
        HA_MONITOR_GPU_MAX_SILENCE_SECS = builtins.toString cfg.gpuMaxSilenceSecs;
        HA_MONITOR_MEMORY_MAX_SILENCE_SECS = builtins.toString cfg.memoryMaxSilenceSecs;
        HA_MONITOR_DISK_MAX_SILENCE_SECS = builtins.toString cfg.diskMaxSilenceSecs;
      }
      // optionalAttrs (cfg.mqtt.username != null) {
        HA_MONITOR_MQTT_USERNAME = cfg.mqtt.username;
      }
      // optionalAttrs (cfg.mqtt.password != null) {
        HA_MONITOR_MQTT_PASSWORD = cfg.mqtt.password;
      }
      // optionalAttrs (cfg.nodeId != null) {
        HA_MONITOR_NODE_ID = cfg.nodeId;
      }
      // optionalAttrs (cfg.deviceName != null) {
        HA_MONITOR_DEVICE_NAME = cfg.deviceName;
      }
      // cfg.extraEnvironment;

      serviceConfig = {
        ExecStart = execStart;
        User = cfg.user;
        Group = cfg.group;
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
