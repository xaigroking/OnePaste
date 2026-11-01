#!/usr/bin/env python3
"""Keep application key access independent of operating-system credential providers across every target."""
from pathlib import Path
import re
import subprocess
import sys
import tomllib

FORBIDDEN_PACKAGES = {"keyring", "secret-service", "libsecret", "keychain-services", "security-framework", "security-framework-sys", "windows-credentials", "android-keystore", "kwallet", "huks", "rustls-native-certs"}
CALLS = re.compile(r"keyring\s*::|secure_store\s*::|SecItem(?:CopyMatching|Add|Update|Delete)|SecKeychain\w*|Cred(?:Read|Write|Delete|Enumerate)[AW]?\b|Win32_Security_Credentials|Crypt(?:Protect|Unprotect)Data|NCrypt(?:OpenStorageProvider|CreatePersistedKey|OpenKey)|find-generic-password|add-generic-password|delete-generic-password|find-internet-password|PasswordVault|AndroidKeyStore|android\.security\.keystore|Hks_(?:Get|Import|Export|Generate|Delete)|navigator\s*\.\s*credentials|security\s+(?:create-keychain|import|unlock-keychain)")
PROVIDERS = re.compile(r"impl\s+(?:\w+::)?KeyStore\s+for\s+(\w+)")
EXCLUDE = {"tools/check_no_system_credentials.py", "tools/test_no_system_credentials.py"}

def violations(path, text):
    bad = []
    if path.endswith(("Cargo.toml", "Cargo.lock")):
        data = tomllib.loads(text)
        if path.endswith("Cargo.lock"):
            for package in data.get("package", []):
                if package["name"] in FORBIDDEN_PACKAGES: bad.append("forbidden dependency: " + package["name"])
        else:
            def visit(value):
                if isinstance(value, dict):
                    for key, nested in value.items():
                        if key in FORBIDDEN_PACKAGES or (key == "package" and isinstance(nested, str) and nested in FORBIDDEN_PACKAGES): bad.append("forbidden dependency: " + key)
                        visit(nested)
                elif isinstance(value, list):
                    for nested in value: visit(nested)
            visit(data)
    for match in CALLS.finditer(text): bad.append("forbidden credential API at line " + str(text.count("\n",0,match.start())+1))
    if path.endswith(".rs"):
        for match in PROVIDERS.finditer(text):
            if match[1] not in {"EncryptedKeyStore", "MemoryKeyStore"}: bad.append("unapproved application key provider: " + match[1])
    return bad

def main():
    failed = False
    names = subprocess.check_output(["git","ls-files","--cached","--others","--exclude-standard","-z"]).decode().split("\0")
    for name in names:
        p=Path(name)
        if not name or name in EXCLUDE or not p.is_file() or p.suffix not in {".rs",".toml",".lock",".ts",".tsx",".js",".mjs",".py",".yml",".yaml"}:continue
        try: text=p.read_text()
        except UnicodeError:continue
        for problem in violations(name,text): print(f"FAIL {name}: {problem}",file=sys.stderr);failed=True
    if failed:return 1
    print("system credential guard: OK");return 0
if __name__=="__main__":sys.exit(main())
