{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        pname = cargoToml.package.name;
        version = cargoToml.package.version;

        cargoHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = [
            "x86_64-unknown-linux-musl"
            "x86_64-pc-windows-gnu"
          ];
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.pkg-config
            pkgs.openssl
          ];

          shellHook = ''
            # Set up git hooks if .git exists
            if [ -d .git ]; then
              mkdir -p .git/hooks
              cat > .git/hooks/pre-commit << 'HOOK'
#!/usr/bin/env bash
set -e
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
HOOK
              chmod +x .git/hooks/pre-commit
            fi
          '';
        };

        packages = {
          # Native linux build (glibc)
          default = pkgs.rustPlatform.buildRustPackage {
            inherit pname version;
            src = ./.;
            inherit cargoHash;

            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.openssl ];

            meta = with pkgs.lib; {
              description = "Kibana/Elasticsearch MCP server for log access";
              license = licenses.mit;
            };
          };

          # Static linux build (musl)
          musl = let
            muslPkgs = pkgs.pkgsCross.musl64;
          in muslPkgs.rustPlatform.buildRustPackage {
            pname = "${pname}-musl";
            inherit version;
            src = ./.;
            inherit cargoHash;

            nativeBuildInputs = [ pkgs.pkg-config ];

            CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";

            meta = with pkgs.lib; {
              description = "Kibana/Elasticsearch MCP server for log access (musl static)";
              license = licenses.mit;
            };
          };
        };
      }
    );
}
