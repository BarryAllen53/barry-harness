use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn generate() -> Pkce {
    let verifier = random_token(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Pkce {
        verifier,
        challenge,
    }
}

pub fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::rng().fill(buffer.as_mut_slice());
    URL_SAFE_NO_PAD.encode(&buffer)
}
