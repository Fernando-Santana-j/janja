#!/usr/bin/env bash
# Run from the owner's terminal when this agent session cannot open network sockets.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1-}
if [ "$#" -gt 1 ] || { [ -n "$mode" ] && [ "$mode" != '--distribution-only' ] && [ "$mode" != '--inspect' ] && [ "$mode" != '--audio-test' ]; }; then
    echo 'Uso: bash scripts/test-windows-from-linux.sh [--distribution-only|--inspect|--audio-test]' >&2
    exit 2
fi
ssh_key=/home/fernando/.ssh/windows_caixa
remote=caixa@192.168.1.11
started_agent=false
if [ -z "${SSH_AUTH_SOCK:-}" ]; then
    eval "$(ssh-agent -s)"
    started_agent=true
fi
trap 'if "$started_agent"; then ssh-agent -k >/dev/null; fi' EXIT
ssh-add "$ssh_key"
ssh_options=(-F /dev/null -i "$ssh_key" -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30)
if [ "$mode" = '--audio-test' ]; then
    scp "${ssh_options[@]}" scripts/diagnose-audio-windows.ps1 "$remote:"
    ssh "${ssh_options[@]}" "$remote" 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:/Users/Caixa/diagnose-audio-windows.ps1' 2>&1 | tee dist/windows-audio-debug.log
    scp "${ssh_options[@]}" "$remote:Downloads/windows-audio-debug.zip" dist/windows-audio-debug.zip
    exit
fi
if [ "$mode" = '--inspect' ]; then
    scp "${ssh_options[@]}" scripts/inspect-windows-validation.ps1 "$remote:"
    ssh "${ssh_options[@]}" "$remote" 'powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:/Users/Caixa/inspect-windows-validation.ps1' 2>&1 | tee dist/windows-inspection.log
    exit
fi
python3 scripts/package-sources.py
scp "${ssh_options[@]}" dist/janja-nativo-fontes.zip scripts/build-remote-windows.ps1 scripts/validate-remote-windows.ps1 "$remote:"
extra=''
if [ "$mode" = '--distribution-only' ]; then extra=' -DistributionOnly'; fi
ssh "${ssh_options[@]}" "$remote" "powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:/Users/Caixa/build-remote-windows.ps1 -Diagnostics$extra" 2>&1 | tee dist/windows-build-r12.log
ssh "${ssh_options[@]}" "$remote" "powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:/Users/Caixa/validate-remote-windows.ps1$extra" 2>&1 | tee dist/windows-validation-r12.log
for name in janja-windows.exe janja-setup-windows.exe janja-diagnostico-windows.exe windows.build.json windows-self-test.json windows-network-test.json windows-stress-test.json windows-security.json; do
    scp "${ssh_options[@]}" "$remote:Downloads/$name" "dist/$name.pending"
done
python3 - <<'PY'
import hashlib,json,os
import re
from pathlib import Path
root=Path('dist')
stamp=json.loads((root/'windows.build.json.pending').read_text(encoding='utf-8-sig'))
version=re.search(r'pub const BUILD: &str = "([^"]+)";',Path('src/lib.rs').read_text()).group(1)
assert stamp['version']==version, 'Versao Windows incorreta'
for name,digest in stamp['sha256'].items():
    assert name in ('janja-windows.exe','janja-setup-windows.exe')
    with (root/(name+'.pending')).open('rb') as stream:
        assert hashlib.file_digest(stream,'sha256').hexdigest()==digest, name
for name in ('windows-self-test.json','windows-network-test.json'):
    report=json.loads((root/(name+'.pending')).read_text(encoding='utf-8-sig'))
    assert report['received_fps'] >= 24 and report['audio_peak'] > 0.01, name
security=json.loads((root/'windows-security.json.pending').read_text(encoding='utf-8-sig'))
assert security['defender']['status']=='no_detection_in_scan', 'Analise do Defender inconclusiva'
for name in ('janja-windows.exe','janja-setup-windows.exe','janja-diagnostico-windows.exe','windows.build.json','windows-self-test.json','windows-network-test.json','windows-stress-test.json','windows-security.json'):
    os.replace(root/(name+'.pending'),root/name)
print(f'Executaveis Windows {version} e relatorios validados em dist/.')
PY
