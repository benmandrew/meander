{
  description = "Meander: a live browser simulation of a meandering river";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );

      # The wasm-bindgen CLI must match the crate version exactly, so take it
      # from Cargo.lock rather than pinning it twice.
      lock = builtins.fromTOML (builtins.readFile ./Cargo.lock);
      wasmBindgenVersion =
        (nixpkgs.lib.findFirst (p: p.name == "wasm-bindgen") null lock.package).version;
      wasmBindgenCli =
        pkgs: pkgs."wasm-bindgen-cli_${builtins.replaceStrings [ "." ] [ "_" ] wasmBindgenVersion}";

      toolchain =
        pkgs: extensions:
        pkgs.rust-bin.stable.latest.minimal.override {
          targets = [ "wasm32-unknown-unknown" ];
          inherit extensions;
        };
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          rust = toolchain pkgs [ ];
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };
          web-dist = rustPlatform.buildRustPackage {
            pname = "meander-web";
            version = "0.1.0";
            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./meander-core
                ./meander-wasm
                ./web
              ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [
              (wasmBindgenCli pkgs)
              pkgs.binaryen
            ];
            buildPhase = ''
              runHook preBuild
              cargo build --release --frozen -p meander-wasm \
                --target wasm32-unknown-unknown --target-dir target
              wasm-bindgen --target web --out-dir pkg \
                target/wasm32-unknown-unknown/release/meander_wasm.wasm
              wasm-opt -O pkg/meander_wasm_bg.wasm -o pkg/meander_wasm_bg.wasm
              runHook postBuild
            '';
            doCheck = false;
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cp web/index.html web/style.css web/favicon.svg web/*.js $out/
              cp -r web/fonts $out/fonts
              cp -r pkg $out/pkg
              runHook postInstall
            '';
          };
        in
        {
          inherit web-dist;
          default = web-dist;
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (toolchain pkgs [
              "rust-src"
              "rust-analyzer"
              "clippy"
              "rustfmt"
            ])
            (wasmBindgenCli pkgs)
            pkgs.wasm-pack
            pkgs.binaryen
            pkgs.gnumake
            pkgs.python3
          ];
        };
      });
    };
}
