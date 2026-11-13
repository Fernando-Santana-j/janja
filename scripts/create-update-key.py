#!/usr/bin/env python3
"""Create a release signing key once; never print or distribute the private key."""
import base64
from pathlib import Path
import subprocess


def main():
    root = Path(__file__).resolve().parent.parent
    directory = root / '.release-keys'
    directory.mkdir(mode=0o700, exist_ok=True)
    private = directory / 'update-private.pem'
    if private.exists() or (root / 'assets/update-public.pem').exists():
        raise SystemExit('A chave ja existe. Preserve a chave privada original para as proximas versoes.')
    # Create exclusively with restricted permissions before OpenSSL writes it.
    private.touch(mode=0o600, exist_ok=False)
    try:
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'RSA', '-pkeyopt', 'rsa_keygen_bits:3072',
                        '-pkeyopt', 'rsa_keygen_pubexp:65537', '-out', str(private)],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        public = root / 'assets/update-public.pem'
        subprocess.run(['openssl', 'pkey', '-in', str(private), '-pubout', '-out', str(public)],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        modulus = subprocess.check_output(['openssl', 'rsa', '-pubin', '-in', str(public),
                                           '-noout', '-modulus'], text=True).strip().split('=', 1)[1]
        encoded = base64.b64encode(bytes.fromhex(modulus)).decode('ascii')
        (root / 'assets/update-public.xml').write_text(
            f'<RSAKeyValue><Modulus>{encoded}</Modulus><Exponent>AQAB</Exponent></RSAKeyValue>\n',
            encoding='ascii')
    except BaseException:
        private.unlink(missing_ok=True)
        raise
    print('Chave criada em .release-keys/update-private.pem. Guarde um backup seguro.')
    print('Somente assets/update-public.pem e assets/update-public.xml entram na distribuicao.')


if __name__ == '__main__':
    main()
