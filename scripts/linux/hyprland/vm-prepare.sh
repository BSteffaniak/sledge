#!/usr/bin/env bash
# Installed as sledge-hyprland-vm-prepare; pass the checkout directory explicitly.
# The small host prerequisite must pass before Nix realizes the guest image.
set -euo pipefail
: "${SLEDGE_VM_GATE:?use flake package hyprland-vm-prepare}"
: "${SLEDGE_VM_RUNNER:?use flake package hyprland-vm-prepare}"
: "${SLEDGE_VM_EXECUTABLE:?immutable generated runner required}"
: "${SLEDGE_VM_IDENTITY:?runner admission validator required}"
if [[ $# -lt 2 || $# -gt 3 || ! -f $1/flake.nix || ( $# == 3 && $3 != --kvm ) ]]; then
  echo 'usage: sledge-hyprland-vm-prepare CHECKOUT_DIRECTORY /dev/dri/renderDNUMBER [--kvm]' >&2
  exit 2
fi
root=$(realpath "$1")
echo 'Budget estimate: 2 CPUs, 4 GiB guest RAM, 8 GiB disposable disk, 180s runtime.'
echo 'Additional closure/image estimate: 5-15 GiB; no guest realization before GPU gate.'
bash "$SLEDGE_VM_GATE" "$2"
# All realization remains through nix shell, with no profile or lock mutation.
exec nix shell --no-write-lock-file "path:$root#hyprland-vm" \
  --command bash "$SLEDGE_VM_RUNNER" "${@:2}"
