{
  description = "pyflows — media transcode engine packaged as an Unmanic plugin (VAAPI HEVC)";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    blueprint = {
      url = "github:numtide/blueprint";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Rust toolchain with the wasm32 target for the Leptos console (spec 006);
    # nixpkgs' rustc ships no wasm32-unknown-unknown std.
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs:
    inputs.blueprint {
      inherit inputs;
      prefix = "nix/";
      systems = ["x86_64-linux" "aarch64-linux"];
    };
}
