{
  description = "bop - Hardware-aware battery optimization for Linux laptops";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "bop";
          version =
            let
              cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
            in
            cargoToml.package.version;

          src = pkgs.lib.cleanSource ./.;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.installShellFiles ];
          buildInputs = [ pkgs.makeWrapper ];

          postBuild = ''
            cargo run --bin manpage
          '';

          postInstall = ''
            installManPage man/*.1

            $out/bin/bop completions bash > bop.bash
            $out/bin/bop completions zsh > _bop
            $out/bin/bop completions fish > bop.fish
            installShellCompletion bop.bash _bop bop.fish

            wrapProgram $out/bin/bop \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.iw ]}
          '';

          meta = with pkgs.lib; {
            description = "Hardware-aware battery optimization for Linux laptops";
            homepage = "https://github.com/yv-was-taken/bop";
            license = licenses.mit;
            platforms = platforms.linux;
            mainProgram = "bop";
          };
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [
            rust-analyzer
            clippy
            rustfmt
            cargo-deb
            iw
          ];
        };
      }
    );
}
