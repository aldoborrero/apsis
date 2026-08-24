{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.apsis-web;
in {
  options.services.apsis-web = {
    enable = lib.mkEnableOption "the apsis operator console (spec 006)";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The apsis-web package to run (the SSR server + bundled site assets).";
    };

    natsUrl = lib.mkOption {
      type = lib.types.str;
      default = "nats://127.0.0.1:4222";
      description = ''
        NATS URL the console reads the KV / publishes control intents on. The credentials this
        URL carries should be scoped like the operator's — publish `apsis.control.*`, subscribe
        `apsis.progress.*`, read the KV, and nothing else. The console is not a privilege path.
      '';
    };

    address = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1:3000";
      description = ''
        Bind address for the SSR server. Keep it on loopback (or a mesh interface) and put an
        auth reverse-proxy in front — the console does not authenticate itself and expects the
        proxy to inject `X-Forwarded-User` / `X-Auth-Request-User` (absent → 401).
      '';
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Optional EnvironmentFile for secrets (e.g. a NATS-credential-bearing NATS_URL).";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.apsis-web = {
      description = "apsis operator console (spec 006)";
      wantedBy = ["multi-user.target"];
      after = ["network-online.target"];
      wants = ["network-online.target"];

      environment = {
        NATS_URL = cfg.natsUrl;
        LEPTOS_SITE_ADDR = cfg.address;
        # Serve the hydrate bundle + assets from the package's bundled site dir.
        LEPTOS_SITE_ROOT = "${cfg.package}/share/apsis-web/site";
      };

      serviceConfig = {
        ExecStart = lib.getExe cfg.package;
        Restart = "on-failure";
        RestartSec = 5;

        DynamicUser = true;
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;

        # Read-only projection of NATS state — it needs no writable paths of its own.
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = ["AF_INET" "AF_INET6"];
      };
    };
  };
}
