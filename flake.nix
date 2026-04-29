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
      packages = forSystems linuxSystems (
        pkgs:
        let
          runtimePath = pkgs.lib.makeBinPath (
            [
              pkgs.bash
              pkgs.coreutils
              pkgs.python3
              pkgs.util-linux
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.systemd ]
          );
        in
        {
          default = pkgs.stdenvNoCC.mkDerivation {
            pname = "proxmox-notify";
            inherit version;
            src = self;

            nativeBuildInputs = [
              pkgs.gnumake
              pkgs.makeWrapper
            ];

            dontBuild = true;

            installPhase = ''
              runHook preInstall

              make install DESTDIR="$out" PREFIX= SYSCONFDIR=/etc
              patchShebangs "$out/bin" "$out/lib/proxmox-notify/helpers"
              substituteInPlace "$out"/lib/systemd/system/proxmox-notify-*.service \
                --replace-fail /usr/local/bin/proxmox-notify "$out/bin/proxmox-notify"
              wrapProgram "$out/bin/proxmox-notify" \
                --prefix PATH : ${runtimePath}

              runHook postInstall
            '';

            meta = {
              description = "pmxcfs-backed state announcement CLI for Proxmox clusters";
              platforms = linuxSystems;
              mainProgram = "proxmox-notify";
            };
          };

          deb = pkgs.stdenvNoCC.mkDerivation {
            pname = "proxmox-notify-deb";
            inherit version;
            src = self;

            nativeBuildInputs = [
              pkgs.dpkg
              pkgs.gnumake
            ];

            buildPhase = ''
              runHook preBuild

              patchShebangs .
              BUILD_DIR="$TMPDIR/build" VERSION="${version}" scripts/build-deb

              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall

              mkdir -p "$out"
              cp "$TMPDIR/build/proxmox-notify_${version}_all.deb" "$out/"

              runHook postInstall
            '';
          };
        }
      );

      checks = forSystems linuxSystems (pkgs: {
        default = pkgs.stdenvNoCC.mkDerivation {
          name = "proxmox-notify-check";
          src = self;

          nativeBuildInputs = [
            pkgs.gnumake
            pkgs.python3
            pkgs.util-linux
          ];

          buildPhase = ''
            runHook preBuild

            patchShebangs .
            make ci

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
              pkgs.git
              pkgs.gh
              pkgs.gnumake
              pkgs.python3
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.dpkg
              pkgs.util-linux
            ];

          shellHook = ''
            echo "proxmox-notify dev shell"
            echo "Run: make ci"
          '';
        };
      });
    };
}
