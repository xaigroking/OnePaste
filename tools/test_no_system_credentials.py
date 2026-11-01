#!/usr/bin/env python3
"""Regression cases for package aliases, target declarations and direct platform calls."""
import unittest
from check_no_system_credentials import violations
class Guard(unittest.TestCase):
    def test_blocks_dependencies_on_any_target_and_renamed_packages(self):
        for text in ['[target.future.dependencies]\nkeyring="3"', '[dependencies]\nrenamed={package="secret-service",version="1"}']:
            self.assertTrue(violations("Cargo.toml",text))
        self.assertTrue(violations("Cargo.lock",'[[package]]\nname="security-framework"\nversion="1"'))
    def test_blocks_actual_calls_and_new_key_providers(self):
        for text in ['keyring::Entry::new("service","account")','SecItemCopyMatching(query,out)','CredReadW(target,kind,0,result)','navigator.credentials.get(options)','use windows::Win32::Security::Credentials::CredReadW as read','features=["Win32_Security_Credentials"]','impl KeyStore for SystemProvider {}']:
            self.assertTrue(violations("entry.rs",text))
    def test_keeps_application_crypto_and_device_authentication(self):
        self.assertEqual(violations("entry.rs",'let key = Key::random()?; authenticate_device_token(token); impl KeyStore for EncryptedKeyStore {}'),[])
if __name__=="__main__":unittest.main()
