{
  description = "Agent Computer Use Platform development shell and installable Rust binaries";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = lib.genAttrs systems;
      version = "0.1.0-alpha.1";
      sourceFilter = path: type:
        let
          name = baseNameOf (toString path);
        in
        lib.cleanSourceFilter path type
        && !(lib.elem name [
          ".jj"
          ".omx"
          ".private"
          ".nix-root"
          ".playwright-cli"
          ".tmp"
          "artifacts"
          "flake.lock"
          "flake.nix"
          "node_modules"
          "output"
          "target"
        ])
        && !(lib.hasPrefix "result" name);
      src = lib.cleanSourceWith {
        src = ./.;
        filter = sourceFilter;
      };
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          commonArgs = {
            inherit src version;
            cargoLock.lockFile = ./Cargo.lock;
            strictDeps = true;
            nativeCheckInputs = [ pkgs.python311 ];
          };
          mkRustBinary = {
            pname,
            cargoPackage,
            cargoBuildFlags ? [ "--package" cargoPackage ],
            cargoInstallFlags ? cargoBuildFlags,
            cargoTestFlags ? [ "--package" cargoPackage ],
            description,
            mainProgram,
          }:
            pkgs.rustPlatform.buildRustPackage (commonArgs // {
              inherit pname cargoBuildFlags cargoInstallFlags cargoTestFlags;
              meta = {
                inherit description mainProgram;
                license = lib.licenses.mit;
                platforms = systems;
                sourceProvenance = [ lib.sourceTypes.fromSource ];
              };
            });
          guest-runtime = mkRustBinary {
            pname = "guest-runtime";
            cargoPackage = "guest-runtime";
            description = "Linux guest runtime service for the Agent Computer Use Platform";
            mainProgram = "guest-runtime";
          };
          export-schemas = mkRustBinary {
            pname = "export-schemas";
            cargoPackage = "desktop-core";
            cargoBuildFlags = [ "--package" "desktop-core" "--bin" "export-schemas" ];
            cargoInstallFlags = [ "--package" "desktop-core" "--bin" "export-schemas" ];
            cargoTestFlags = [ "--package" "desktop-core" ];
            description = "Schema exporter for the Agent Computer Use Platform desktop contract";
            mainProgram = "export-schemas";
          };
        in
        {
          inherit guest-runtime export-schemas;
          default = guest-runtime;
        });

      apps = forAllSystems (system:
        let
          guestRuntime = self.packages.${system}.guest-runtime;
        in
        {
          guest-runtime = {
            type = "app";
            program = "${guestRuntime}/bin/guest-runtime";
          };
          default = self.apps.${system}.guest-runtime;
        });

      devShells = forAllSystems (system:
        let
          pkgs = pkgsFor system;
          firefoxExecutable = "${pkgs.firefox}/bin/firefox";
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              bun
              cargo
              clippy
              docker-client
              firefox
              imagemagick
              nodejs_22
              python311
              rust-analyzer
              rustc
              rustfmt
              xdotool
              xorg-server
              xprop
            ];

            shellHook = ''
              export FIREFOX_EXECUTABLE="${firefoxExecutable}"
              export ACU_BROWSER_COMMAND="firefox"
            '';
          };
        });
    };
}
