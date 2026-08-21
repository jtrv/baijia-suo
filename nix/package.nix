{
  lib,
  rustPlatform,
  installShellFiles,
  scdoc,
  pam,
  libxkbcommon,
}:

rustPlatform.buildRustPackage {
  pname = "baijia-suo";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    installShellFiles
    scdoc
  ];
  buildInputs = [
    pam
    libxkbcommon
  ];

  postBuild = ''
    scdoc <docs/baijia-suo.1.scd >baijia-suo.1
  '';

  # build.rs emits target/completions/ during the cargo build.
  postInstall = ''
    installManPage baijia-suo.1
    installShellCompletion \
      --bash target/completions/baijia-suo.bash \
      --zsh target/completions/_baijia-suo \
      --fish target/completions/baijia-suo.fish
    # The binary embeds Liberation Sans; OFL-1.1 requires its license ship too.
    install -Dm644 fonts/LICENSE $out/share/licenses/baijia-suo/LICENSE.font
  '';

  meta = {
    description = "Secure Wayland screen locker with xlockmore animation support";
    homepage = "https://github.com/jtrv/baijia-suo";
    license = with lib.licenses; [
      mit
      ofl
    ];
    platforms = lib.platforms.linux;
    mainProgram = "baijia-suo";
  };
}
