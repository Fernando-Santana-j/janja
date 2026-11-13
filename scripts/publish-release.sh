#!/usr/bin/env bash
# Run from the owner's terminal: validates, commits and publishes the current BUILD.
set -euo pipefail
cd "$(dirname "$0")/.."
for program in gh git python3 openssl cargo ssh scp ssh-add; do
    command -v "$program" >/dev/null || { echo "Comando necessario: $program" >&2; exit 1; }
done
if ! gh auth status --hostname github.com >/dev/null 2>&1; then
    gh auth login --hostname github.com --git-protocol https --scopes workflow --web
fi
mkdir -p dist
exec > >(tee -a dist/publicacao.log) 2>&1
repo=Fernando-Santana-j/janja
phase='verificar repositorio'
trap 'echo "Publicacao interrompida na etapa: $phase. Consulte dist/publicacao.log." >&2' ERR
git_auth() { git -c credential.helper= -c 'credential.helper=!gh auth git-credential' "$@"; }
origin=$(git remote get-url origin)
if [[ "$origin" != "https://github.com/$repo.git" && "$origin" != "https://github.com/$repo" ]]; then
    echo 'O origin nao corresponde ao repositorio de publicacao.' >&2; exit 1
fi
[[ "$(git branch --show-current)" == main ]] || { echo 'Execute na branch main.' >&2; exit 1; }
[[ "$(gh repo view "$repo" --json visibility --jq .visibility)" == PUBLIC ]] || {
    echo 'Os downloads automaticos exigem uma Release em repositorio publico.' >&2; exit 1;
}
version=$(python3 -c 'import pathlib,re; print(re.search(r"pub const BUILD: &str = \"([^\"]+)\";",pathlib.Path("src/lib.rs").read_text()).group(1))')
[[ "$version" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-r[0-9]+-[a-z0-9-]+$ ]] || exit 1
git_auth fetch origin main --tags
git merge-base --is-ancestor origin/main HEAD || {
    echo 'A branch remota tem alteracoes novas. Integre-as e valide antes de publicar.' >&2; exit 1;
}
if git rev-parse --verify "refs/tags/$version" >/dev/null 2>&1; then
    echo "A tag $version ja existe. Para uma nova atualizacao, incremente BUILD; nao reutilize a tag." >&2
    exit 1
fi
git diff --cached --quiet || { echo 'Existem arquivos ja preparados no Git; revise-os antes de executar este comando.' >&2; exit 1; }
test -f .release-keys/update-private.pem
git check-ignore -q .release-keys/update-private.pem
phase='validar a chave original'
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
printf '%s' 'Janja release signing check' > "$temporary/check"
openssl dgst -sha256 -sign .release-keys/update-private.pem -out "$temporary/signature" "$temporary/check"
openssl dgst -sha256 -verify assets/update-public.pem -signature "$temporary/signature" "$temporary/check"
phase='validar atualizador e cliente Windows'
python3 scripts/test-updates.py
cargo fmt --all -- --check
bash scripts/test-windows-from-linux.sh --distribution-only
phase='preparar commit revisavel'
git add -- Cargo.toml Cargo.lock build.rs src scripts assets examples .github README.md .gitignore docs/atualizacoes.md docs/plano-r12.md docs/medicoes-20261113.json
if git grep --cached -I -l -E '^-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----'; then
    echo 'Chave privada encontrada nos arquivos preparados; publicacao cancelada.' >&2; exit 1
else
    status=$?
    [[ "$status" -eq 1 ]] || exit "$status"
fi
phase='configurar assinatura no GitHub'
gh secret set JANJA_UPDATE_PRIVATE_KEY --repo "$repo" < .release-keys/update-private.pem
if ! git diff --cached --quiet; then
    git diff --cached --stat
    git commit -m "Publica Janja $version com instaladores e atualizacoes assinadas"
fi
phase='enviar codigo e tag'
commit=$(git rev-parse HEAD)
git_auth push origin main
git tag "$version"
git_auth push origin "refs/tags/$version"
phase='aguardar compilacao e testes no GitHub'
run_id=''
for attempt in {1..24}; do
    if ! run_id=$(gh run list --repo "$repo" --workflow release.yml --commit "$commit" --event push --limit 10 --json databaseId,headBranch --jq "map(select(.headBranch == \"$version\")) | first | .databaseId // empty"); then run_id=''; fi
    if [[ -n "$run_id" ]]; then break; fi
    sleep 5
done
[[ -n "$run_id" ]] || { echo 'A tag foi enviada; a execucao do workflow ainda nao apareceu.' >&2; exit 1; }
printf 'Workflow: https://github.com/%s/actions/runs/%s\n' "$repo" "$run_id"
gh run watch "$run_id" --repo "$repo" --exit-status
phase='conferir arquivos publicados'
published="dist/publicado-$version"
mkdir -p "$published"
gh release download "$version" --repo "$repo" --dir "$published" --clobber --pattern 'janja-update.json*' --pattern 'janja-setup-windows.exe' --pattern 'janja-setup-linux.sh'
python3 scripts/verify-release.py --directory "$published"
gh release view "$version" --repo "$repo" --json tagName,url,isDraft,isPrerelease,assets > dist/publicacao.json
python3 - <<'PY'
import json
from pathlib import Path
release=json.loads(Path('dist/publicacao.json').read_text())
expected={'janja-linux','janja-windows.exe','janja-setup-linux.sh','janja-setup-windows.exe','janja-update.json','janja-update.json.sig'}
assert not release['isDraft'] and not release['isPrerelease']
assert {asset['name'] for asset in release['assets']} == expected, 'Arquivos da Release incompletos ou inesperados'
print('Publicacao completa e manifesto conferido:',release['url'])
PY
