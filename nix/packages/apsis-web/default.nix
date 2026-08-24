{
  inputs,
  pkgs,
  ...
}: let
  inherit (pkgs) lib;

  fenix = inputs.fenix.packages.${pkgs.system};
  # Same toolchain as the dev shell: stable + the wasm32 std the hydrate bundle needs.
  rustToolchain = fenix.combine [
    fenix.stable.toolchain
    fenix.targets.wasm32-unknown-unknown.stable.rust-std
  ];
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };

  # The whole workspace is the build input (cargo-leptos compiles the SSR bin from it), minus
  # the build/VCS dirs so a doc edit doesn't invalidate the derivation.
  src = lib.cleanSourceWith {
    src = ../../..;
    filter = path: _type: let
      base = baseNameOf path;
    in
      base != "target" && base != ".git" && !(lib.hasInfix "/target/" path);
  };
in
  rustPlatform.buildRustPackage {
    pname = "apsis-web";
    version = "0.0.0";
    inherit src;

    cargoLock.lockFile = ../../../Cargo.lock;

    # cargo-leptos drives two cargo builds (native SSR bin + wasm hydrate bundle); the CLI
    # version must match the `wasm-bindgen` crate pin in apsis-web/Cargo.toml.
    nativeBuildInputs = [
      pkgs.cargo-leptos
      pkgs.binaryen
      pkgs.wasm-bindgen-cli
    ];

    buildPhase = ''
      runHook preBuild
      cargo leptos build --release -vv
      runHook postBuild
    '';

    # cargo-leptos runs its own build; the standard cargo test hook would rebuild differently.
    doCheck = false;

    installPhase = ''
      runHook preInstall
      mkdir -p $out/bin $out/share/apsis-web
      install -Dm755 target/release/apsis-web $out/bin/apsis-web
      cp -r target/site $out/share/apsis-web/site
      runHook postInstall
    '';

    meta = {
      description = "apsis operator console — a Leptos read-mostly UI over the spec 005 control protocol";
      mainProgram = "apsis-web";
      platforms = lib.platforms.linux;
    };
  }
