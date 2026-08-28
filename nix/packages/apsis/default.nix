{
  inputs,
  pkgs,
  ...
}: let
  inherit (pkgs) lib;

  # Same toolchain as the dev shell (edition 2024 + let-chains); no wasm target needed for the
  # native daemons, unlike apsis-web.
  fenix = inputs.fenix.packages.${pkgs.system};
  rustToolchain = fenix.stable.toolchain;
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };

  src = lib.cleanSourceWith {
    src = ../../..;
    filter = path: _type: let
      base = baseNameOf path;
    in
      base != "target" && base != ".git" && !(lib.hasInfix "/target/" path);
  };
in
  rustPlatform.buildRustPackage {
    pname = "apsis";
    version = "0.0.0";
    inherit src;

    cargoLock.lockFile = ../../../Cargo.lock;

    # Build only the two daemons — not the whole workspace (apsis-web needs cargo-leptos, the
    # engine/common are libraries). Their deps don't pull in leptos, so nixpkgs C toolchain is
    # all that's required (ring/rustls compile against it).
    cargoBuildFlags = ["-p" "apsis-coordinator" "-p" "apsis-worker"];

    # The suite is gated on a live nats-server (and ffmpeg); it can't run in the sandbox.
    doCheck = false;

    meta = {
      description = "apsis distributed media-transcode scheduler — coordinator + worker daemons";
      homepage = "https://github.com/aldoborrero/apsis";
      license = lib.licenses.mit;
      mainProgram = "apsis-coordinator";
      platforms = lib.platforms.linux;
    };
  }
