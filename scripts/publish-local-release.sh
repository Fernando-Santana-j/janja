#!/usr/bin/env bash
# Publish already validated packages when GitHub Actions is unavailable.
set -euo pipefail
cd "$(dirname "$0")/.."
for program in gh git python3 openssl; do
    command -v "$program" >/dev/null || { echo "Comando necessario: $program" >&2; exit 1; }
done
gh auth status --hostname github.com >/dev/null 2>&1 || { echo 'Entre com gh auth login e execute novamente.' >&2; exit 1; }
mkdir -p dist
if [[ "${JANJA_PUBLICATION_LOGGED-}" != 1 ]]; then exec > >(tee -a dist/publicacao.log) 2>&1; fi
phase='conferir a tag e os pacotes locais'
trap 'echo "Publicacao local interrompida na etapa: $phase. Consulte dist/publicacao.log." >&2' ERR
repo=Fernando-Santana-j/janja
version=$(python3 -c 'import pathlib,re; print(re.search(r"pub const BUILD: &str = \"([^\"]+)\";",pathlib.Path("src/lib.rs").read_text()).group(1))')
[[ "$version" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-r[0-9]+-[a-z0-9-]+$ ]] || exit 1
[[ "$(gh repo view "$repo" --json visibility --jq .visibility)" == PUBLIC ]] || exit 1
git -c credential.helper= -c 'credential.helper=!gh auth git-credential' fetch "https://github.com/$repo.git" "refs/tags/$version"
[[ "$(git rev-parse 'FETCH_HEAD^{commit}')" == "$(git rev-parse HEAD)" ]] || {
    echo 'A tag remota deve apontar para o commit atual. Nenhuma tag foi alterada.' >&2; exit 1;
}
git diff --quiet HEAD -- src assets Cargo.toml Cargo.lock build.rs scripts/update-linux.py scripts/update-windows.ps1 scripts/installer-linux.sh scripts/installer-windows.nsi scripts/package-native.py scripts/package-linux-installer.py || {
    echo 'Existem mudancas de aplicativo ainda fora do commit validado.' >&2; exit 1;
}
git check-ignore -q .release-keys/update-private.pem
python3 scripts/test-updates.py
python3 scripts/release-manifest.py --private-key .release-keys/update-private.pem
python3 scripts/verify-release.py --validated-builds dist
files=(janja-linux janja-windows.exe janja-setup-linux.sh janja-setup-windows.exe janja-update.json janja-update.json.sig)
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
phase='preparar rascunho da Release'
if gh release view "$version" --repo "$repo" --json tagName,isDraft,assets > "$temporary/release.json"; then
    python3 - "$temporary/release.json" "$version" <<'PY'
import json,sys
release=json.load(open(sys.argv[1]))
expected={'janja-linux','janja-windows.exe','janja-setup-linux.sh','janja-setup-windows.exe','janja-update.json','janja-update.json.sig'}
if release['tagName'] != sys.argv[2] or not release['isDraft'] or not {a['name'] for a in release['assets']} <= expected:
    raise SystemExit('A Release ja esta publicada ou possui arquivos inesperados; nada sera sobrescrito.')
PY
else
    cat > "$temporary/notes.md" <<EOF
Transmissao de tela e audio para Windows e Linux.

- Tela cheia preserva a imagem inteira por padrao.
- Renderizacao mais leve e cliente sem instrumentacao de diagnostico dispensavel.
- Ajustes na selecao do encoder AMD e testes de transmissao, audio e carga sintetica.
- Instaladores com atualizacoes assinadas pelo GitHub ao abrir o aplicativo.
- Instalacao, atualizacao, rollback e desinstalacao verificados; Defender sem deteccoes nos arquivos Windows publicados.

Instale uma vez com janja-setup-windows.exe ou sh janja-setup-linux.sh. Use os atalhos instalados para receber as proximas atualizacoes. Os portateis continuam com atualizacao manual.

Linux: pacote local gerado em Arch Linux com $(getconf GNU_LIBC_VERSION). Algumas bibliotecas exigem essa versao; distribuicoes com glibc anterior precisam de compilacao propria. Esta publicacao usa os pacotes locais validados porque o GitHub Actions esta indisponivel na conta.

Sem certificado Authenticode: avisos de reputacao do Windows ou navegador ainda podem ocorrer. Nenhuma desativacao do antivirus e necessaria.
EOF
    gh release create "$version" --repo "$repo" --draft --verify-tag --title "Janja $version" --notes-file "$temporary/notes.md"
fi
phase='enviar e conferir os seis arquivos'
gh release upload "$version" --repo "$repo" --clobber "${files[@]/#/dist/}"
published="dist/publicado-$version"
mkdir -p "$published"
gh release download "$version" --repo "$repo" --dir "$published" --clobber
python3 scripts/verify-release.py --directory "$published" --validated-builds dist
gh release view "$version" --repo "$repo" --json isDraft,assets > "$temporary/release.json"
python3 - "$temporary/release.json" <<'PY'
import json,sys
release=json.load(open(sys.argv[1]))
expected={'janja-linux','janja-windows.exe','janja-setup-linux.sh','janja-setup-windows.exe','janja-update.json','janja-update.json.sig'}
if not release['isDraft'] or {a['name'] for a in release['assets']} != expected:
    raise SystemExit('O rascunho nao contem exatamente os seis arquivos esperados.')
PY
phase='tornar a Release publica'
gh release edit "$version" --repo "$repo" --draft=false --latest
gh release view "$version" --repo "$repo" --json tagName,url,isDraft,isPrerelease,assets > dist/publicacao.json
python3 - <<'PY'
import json
from pathlib import Path
release=json.loads(Path('dist/publicacao.json').read_text())
if release['isDraft'] or release['isPrerelease']:
    raise SystemExit('A Release ainda nao esta publica como versao estavel.')
print('Publicacao local concluida:',release['url'])
PY
