use crate::hashing::{self, Algorithm};
use crate::{Result, invalid};
use ed25519_dalek::{Signature as EdSignature, Signer, SigningKey, VerifyingKey};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub schema: String,
    pub kind: String,
    pub algorithm: String,
    pub key: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    pub schema: String,
    pub algorithm: String,
    pub hash_algorithm: Algorithm,
    pub hash: String,
    pub public_key: String,
    pub signature: String,
}

fn write_key(path: &Path, kind: &str, bytes: &[u8]) -> Result<()> {
    let key = Key {
        schema: "devtool-proof.key/1".into(),
        kind: kind.into(),
        algorithm: "ed25519".into(),
        key: hex::encode(bytes),
    };
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if kind == "private" { 0o600 } else { 0o644 });
    }
    let mut file = options.open(path)?;
    let encoded = Zeroizing::new(serde_json::to_vec_pretty(&key)?);
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn generate(private: &Path, public: &Path) -> Result<String> {
    if crate::files::output(private)? == crate::files::output(public)? {
        return Err(invalid(
            "Private and public key outputs require different paths",
        ));
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::fill(&mut *seed).map_err(|error| invalid(error.to_string()))?;
    let signing = SigningKey::from_bytes(&seed);
    write_key(private, "private", signing.as_bytes())?;
    write_key(public, "public", signing.verifying_key().as_bytes())?;
    Ok(hashing::reader(
        signing.verifying_key().as_bytes().as_slice(),
        Algorithm::Sha256,
    )?
    .hash)
}

fn load(path: &Path, kind: &str) -> Result<[u8; 32]> {
    let bytes = Zeroizing::new(fs::read(path)?);
    let key: Key = serde_json::from_slice(&bytes)?;
    if key.schema != "devtool-proof.key/1" || key.algorithm != "ed25519" || key.kind != kind {
        return Err(invalid(format!("Expected an Ed25519 {kind} key")));
    }
    hex::decode(key.key)
        .map_err(|error| invalid(error.to_string()))?
        .try_into()
        .map_err(|_| invalid("Key must be 32 bytes"))
}

fn message(hash: &str) -> Result<Vec<u8>> {
    let mut bytes = b"devtool-proof.signature/1\0sha256\0".to_vec();
    bytes.extend(hex::decode(hash).map_err(|error| invalid(error.to_string()))?);
    Ok(bytes)
}

pub fn sign(file: &Path, key: &Path, output: &Path) -> Result<()> {
    let seed = Zeroizing::new(load(key, "private")?);
    let signing = SigningKey::from_bytes(&seed);
    let digest = hashing::file(file, Algorithm::Sha256)?;
    let signature = signing.sign(&message(&digest.hash)?);
    let envelope = Signature {
        schema: "devtool-proof.signature/1".into(),
        algorithm: "ed25519".into(),
        hash_algorithm: Algorithm::Sha256,
        hash: digest.hash,
        public_key: hex::encode(signing.verifying_key().as_bytes()),
        signature: hex::encode(signature.to_bytes()),
    };
    fs::write(output, serde_json::to_vec_pretty(&envelope)?)?;
    Ok(())
}

pub fn verify(file: &Path, key: &Path, signature: &Path) -> Result<bool> {
    let public = load(key, "public")?;
    let verifying =
        VerifyingKey::from_bytes(&public).map_err(|error| invalid(error.to_string()))?;
    let envelope: Signature = serde_json::from_slice(&fs::read(signature)?)?;
    if envelope.schema != "devtool-proof.signature/1"
        || envelope.algorithm != "ed25519"
        || envelope.hash_algorithm != Algorithm::Sha256
    {
        return Err(invalid("Unsupported signature document"));
    }
    hashing::valid(&envelope.hash, Algorithm::Sha256)?;
    let bytes = hex::decode(&envelope.signature).map_err(|error| invalid(error.to_string()))?;
    let signature = EdSignature::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    let actual = hashing::file(file, Algorithm::Sha256)?;
    Ok(envelope
        .public_key
        .eq_ignore_ascii_case(&hex::encode(public))
        && actual.hash.eq_ignore_ascii_case(&envelope.hash)
        && verifying
            .verify_strict(&message(&envelope.hash)?, &signature)
            .is_ok())
}
