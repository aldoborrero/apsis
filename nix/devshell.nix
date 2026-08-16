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
  ];

  shellHook = ''
    export PRJ_ROOT="$PWD"
  '';
}
