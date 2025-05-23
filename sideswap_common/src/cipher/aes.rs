use aes_gcm_siv::{aead::Aead, AeadCore, Aes256GcmSiv, KeyInit, Nonce};

use super::Cipher;

pub struct AesCipher(Aes256GcmSiv);

impl AesCipher {
    pub fn new(key: &[u8; 32]) -> Self {
        Self(Aes256GcmSiv::new(key.into()))
    }
}

impl Cipher for AesCipher {
    type Error = aes_gcm_siv::Error;

    fn encrypt(&mut self, data: &[u8]) -> Vec<u8> {
        let nonce = Aes256GcmSiv::generate_nonce(rand::thread_rng());
        let encrypted = self.0.encrypt(&nonce, data).expect("must not fail");

        let mut output = Vec::new();
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&encrypted);
        output
    }

    fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, Self::Error> {
        if data.len() < std::mem::size_of::<aes_gcm_siv::Nonce>() {
            return Err(aes_gcm_siv::aead::Error);
        }
        let (nonce, encrypted_data) = data.split_at(std::mem::size_of::<aes_gcm_siv::Nonce>());
        let nonce = Nonce::from_slice(nonce);
        self.0.decrypt(nonce, encrypted_data.as_ref())
    }
}
