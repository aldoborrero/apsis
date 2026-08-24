{
  inputs,
  pkgs,
  ...
}: let
  fenix = inputs.fenix.packages.${pkgs.system};
  # The stable toolchain + the wasm32-unknown-unknown std (Leptos hydrate target, spec 006).
  rustToolchain = fenix.combine [
    fenix.stable.toolchain
    fenix.targets.wasm32-unknown-unknown.stable.rust-std
  ];
in
  pkgs.mkShell {
    name = "pyflows";
    packages = with pkgs; [
      # Python (pyflows Unmanic plugin)
      python313
      ruff
      mypy
      ffmpeg-full

      # Rust (apsis) — fenix stable toolchain with the wasm32 target (for apsis-web/Leptos);
      # replaces the plain nixpkgs rustc, which has no wasm32 std. mkShell (not NoCC) so a C
      # linker is present for rustc.
      rustToolchain
      rust-analyzer

      # apsis-web (spec 006): the Leptos build tool, wasm optimizer, and the wasm-bindgen CLI
      # (its version must match the `wasm-bindgen` crate pin in apsis-web/Cargo.toml).
      cargo-leptos
      binaryen
      wasm-bindgen-cli

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
