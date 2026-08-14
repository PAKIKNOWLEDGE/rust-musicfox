# Binary-base packaging (from the skill-nix-binary-base-packing-creator
# pattern): Nix packages a locally built artifact, it does not compile.
#
#   cargo build --release
#   nix-build ./packaging/nix-binary.nix && nix profile install ./result
#
# For reproducible source builds use the flake instead:
#   nix build .# && nix profile install ./result

{ pkgs ? import <nixpkgs> { } }:
pkgs.stdenv.mkDerivation {
  pname = "rust-musicfox";
  version = "0.1.0";

  src = null;
  unpackPhase = "true";
  buildPhase = "true";

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin
    cp ${./../target/release/musicfox} $out/bin/musicfox
    runHook postInstall
  '';
}
