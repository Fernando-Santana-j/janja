#!/bin/sh
set -eu
if [ "${1-}" != '--update' ]; then
    if ! command -v python3 >/dev/null || ! command -v openssl >/dev/null || ! command -v zenity >/dev/null; then
        printf '%s\n' 'Instalando os componentes do atualizador: Python 3, OpenSSL e Zenity.'
        if command -v apt-get >/dev/null; then
            sudo apt-get update
            sudo apt-get install -y python3 openssl zenity
        elif command -v dnf >/dev/null; then
            sudo dnf install -y python3 openssl zenity
        elif command -v pacman >/dev/null; then
            sudo pacman -S --needed python openssl zenity
        else
            printf '%s\n' 'Instale python3, openssl e zenity e execute este instalador novamente.' >&2
            exit 1
        fi
    fi
fi
case "$(uname -m)" in x86_64) ;; *) printf '%s\n' 'Este pacote exige Linux x86_64.' >&2; exit 1 ;; esac
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
payload_line=$(awk '/^__JANJA_PAYLOAD__$/ { print NR + 1; exit }' "$0")
tail -n +"$payload_line" "$0" | tar -xz -C "$temporary"
root="$HOME/.local/share/janja"
version=$(cat "$temporary/version.txt")
chmod +x "$temporary/janja-linux"
actual=$("$temporary/janja-linux" --version)
case "$actual" in *"$version"*) ;; *) printf '%s\n' 'O aplicativo nao passou na verificacao de versao.' >&2; exit 1 ;; esac
mkdir -p "$root"
for name in janja-linux update.py launch version.txt update-config.json update-public.pem; do
    cp "$temporary/$name" "$root/$name.pending"
done
for name in janja-linux update.py launch version.txt update-config.json update-public.pem; do
    mv -f "$root/$name.pending" "$root/$name"
done
chmod +x "$root/launch" "$root/janja-linux"
# The portable launcher registers the installed launch path without starting a session.
JANJA_INSTALLED_LAUNCHER="$root/launch" "$root/janja-linux" --version >/dev/null
if [ "${1-}" != '--update' ]; then
    printf '%s\n' "Instalado em $root. Inicie com: $root/launch"
    "$root/launch" --install-desktop
fi
exit 0
__JANJA_PAYLOAD__
