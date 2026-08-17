{
  pkgs,
  perSystem,
  ...
}:
pkgs.mkShell {
  name = "pyflows";
  packages = with pkgs; [
    # Python (pyflows Unmanic plugin)
    python313
    ruff
    mypy
    ffmpeg-full

    # Rust (apsis) — modern toolchain + linter, version-coherent from nixpkgs.
    # mkShell (not NoCC) so a C linker is present for rustc.
    rustc
    cargo
    clippy
    rustfmt
    rust-analyzer

    # Local dev orchestration: `process-compose up` starts a JetStream NATS for
    # the coordinator/worker and their integration tests (see process-compose.yaml).
    process-compose
    nats-server
  ];

  shellHook = ''
    export PRJ_ROOT="$PWD"
    # Point apsis + its integration tests at the process-compose NATS.
    export NATS_URL="''${NATS_URL:-nats://127.0.0.1:4222}"
    export APSIS_TEST_NATS="''${APSIS_TEST_NATS:-$NATS_URL}"
  '';
}
