#!/usr/bin/env bash
# Construit le binaire optimisé puis les paquets RPM et DEB dans dist/.
#   ./scripts/paquets.sh
# Prérequis : cargo install cargo-deb cargo-generate-rpm
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo-deb >/dev/null || { echo "Installe cargo-deb : cargo install cargo-deb"; exit 1; }
command -v cargo-generate-rpm >/dev/null || { echo "Installe cargo-generate-rpm : cargo install cargo-generate-rpm"; exit 1; }

echo "→ Compilation (release)"
cargo build --release --locked
mkdir -p dist
echo "→ Paquet DEB"
cargo deb --no-build --output dist/
echo "→ Paquet RPM"
cargo generate-rpm --output dist/
echo
ls -lh dist/
