{
  description = "pmxcfs-backed node-to-node state announcements for Proxmox";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      version = "0.1.0";
      linuxSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      devSystems = linuxSystems ++ [
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forSystems =
        systems: f:
        nixpkgs.lib.genAttrs systems (
          system:
          let
            pkgs = import nixpkgs { inherit system; };
          in
          f pkgs
        );
    in
    {
      packages = forSystems linuxSystems (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "proxmox-notify";
          inherit version;
          src = self;

          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];

          postInstall = ''
            install -Dm644 config/config.toml "$out/etc/proxmox-notify/config.toml"
            install -Dm644 systemd/proxmox-notify-announce.service "$out/lib/systemd/system/proxmox-notify-announce.service"
            install -Dm644 systemd/proxmox-notify-watch@.path "$out/lib/systemd/system/proxmox-notify-watch@.path"
            install -Dm644 systemd/proxmox-notify-watch@.service "$out/lib/systemd/system/proxmox-notify-watch@.service"
            install -Dm644 systemd/proxmox-notify-reconcile@.timer "$out/lib/systemd/system/proxmox-notify-reconcile@.timer"
            install -Dm644 systemd/proxmox-notify-reconcile@.service "$out/lib/systemd/system/proxmox-notify-reconcile@.service"
            substituteInPlace "$out"/lib/systemd/system/proxmox-notify-*.service \
              --replace-fail /usr/local/bin/proxmox-notify "$out/bin/proxmox-notify"
            wrapProgram "$out/bin/proxmox-notify" \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.systemd ]}
          '';

          meta = {
            description = "pmxcfs-backed state announcement CLI for Proxmox clusters";
            platforms = linuxSystems;
            mainProgram = "proxmox-notify";
          };
        };
      });

      checks = forSystems linuxSystems (pkgs: {
        default = pkgs.stdenvNoCC.mkDerivation {
          name = "proxmox-notify-check";
          src = self;

          nativeBuildInputs = [
            pkgs.bash
            pkgs.python3
          ];

          buildPhase = ''
            runHook preBuild

            PROXMOX_NOTIFY_BIN="${self.packages.${pkgs.system}.default}/bin/proxmox-notify" \
              tests/smoke.sh

            runHook postBuild
          '';

          installPhase = ''
            touch "$out"
          '';
        };
      });

      devShells = forSystems devSystems (pkgs: {
        default = pkgs.mkShell {
          packages =
            [
              pkgs.bash
              pkgs.cargo
              pkgs.clippy
              pkgs.git
              pkgs.gh
              pkgs.gnumake
              pkgs.python3
              pkgs.rustc
              pkgs.rustfmt
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.dpkg
            ];

          shellHook = ''
            echo "proxmox-notify Rust dev shell"
            echo "Run: make ci"
          '';
        };
      });
    };
}
