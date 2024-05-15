//! This is a tiny worker to hold secret key
//! We use it to avoid sending it back and forth through async pipes
//! so that we can be sure that zeroizing at least tries to do its thing
//!
//! Keep in mind, that leaking secrets in a system like Kalatori is a serious threat
//! with delayed attacks taken into account. Of course, some secret rotation scheme must
//! be implemented, but it seems likely that it would be neglected occasionally.
//! So we go to trouble of running this separate process.
//!
//! Also this abstraction could be used to implement off-system signer

use crate::{
    definitions::Entropy,
    error::{Error, ErrorSigner},
    TaskTracker,
};

use std::env;

use mnemonic_external::{regular::InternalWordList, WordSet};
use substrate_crypto_light::{
    common::{AccountId32, AsBase58, DeriveJunction, FullDerivation},
    sr25519::{Pair, Signature},
};
use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroize;

const SEED: &str = "KALATORI_SEED";

/// Signer handle
pub struct Signer {
    tx: mpsc::Sender<SignerRequest>,
}

impl Signer {
    /// Run once to initialize; this should do **all** secret management
    pub fn init(recipient: AccountId32, task_tracker: TaskTracker) -> Result<Self, Error> {
        let (tx, mut rx) = mpsc::channel(16);
        task_tracker.spawn("Signer", async move {
            let mut seed_entropy = parse_seeds()?; // TODO: shutdown on failure
            while let Some(request) = rx.recv().await {
                match request {
                    SignerRequest::PublicKey(request) => {
                        let _unused = request.res.send(
                            match Pair::from_entropy_and_full_derivation(
                                &seed_entropy,
                                // api spec says use "2" for communication, let's use it here too
                                derivations(&recipient.to_base58_string(2), &request.id),
                            ) {
                                Ok(a) => Ok(a.public().to_base58_string(request.ss58)),
                                Err(e) => Err(e.into()),
                            },
                        );
                    }
                    SignerRequest::Sign(request) => {
                        let _unused = request.res.send(
                            match Pair::from_entropy_and_full_derivation(
                                &seed_entropy,
                                // api spec says use "2" for communication, let's use it here too
                                derivations(&recipient.to_base58_string(2), &request.id),
                            ) {
                                Ok(a) => Ok(a.sign(&request.signable)),
                                Err(e) => Err(e.into()),
                            },
                        );
                    }
                    SignerRequest::Shutdown(res) => {
                        seed_entropy.zeroize();
                        let _ = res.send(());
                        break;
                    }
                }
            }
            Ok("Signer module cleared and is shutting down!".into())
        });

        Ok(Self { tx })
    }

    pub async fn public(&self, id: String, ss58: u16) -> Result<String, ErrorSigner> {
        let (res, rx) = oneshot::channel();
        self.tx
            .send(SignerRequest::PublicKey(PublicKeyRequest { id, ss58, res }))
            .await
            .map_err(|_| ErrorSigner::SignerDown)?;
        rx.await.map_err(|_| ErrorSigner::SignerDown)?
    }

    pub async fn sign(&self, id: String, signable: Vec<u8>) -> Result<Signature, ErrorSigner> {
        let (res, rx) = oneshot::channel();
        self.tx
            .send(SignerRequest::Sign(Sign { id, signable, res }))
            .await
            .map_err(|_| ErrorSigner::SignerDown)?;
        rx.await.map_err(|_| ErrorSigner::SignerDown)?
    }

    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        let _unused = self.tx.send(SignerRequest::Shutdown(tx)).await;
        let _ = rx.await;
    }

    /// Clone wrapper in case we need to make it more complex later
    pub fn interface(&self) -> Self {
        Signer {
            tx: self.tx.clone(),
        }
    }
}

/// Messages sent to signer; signer never initiates anything on its own.
enum SignerRequest {
    /// Generate public key for order
    PublicKey(PublicKeyRequest),

    /// Sign a transaction
    Sign(Sign),

    /// Safe termination
    Shutdown(oneshot::Sender<()>),
}

/// Information required to generate public invoice address, with callback
struct PublicKeyRequest {
    id: String,
    ss58: u16,
    res: oneshot::Sender<Result<String, ErrorSigner>>,
}

/// Bytes to sign, with callback
struct Sign {
    id: String,
    signable: Vec<u8>,
    res: oneshot::Sender<Result<Signature, ErrorSigner>>,
}

/// Read seeds from env
///
/// TODO: read also old seeds and do something about them
fn parse_seeds() -> Result<Entropy, ErrorSigner> {
    entropy_from_phrase(&env::var(SEED).map_err(|_| ErrorSigner::Env(SEED.to_string()))?)
}

/// Convert seed phrase to entropy
pub fn entropy_from_phrase(seed: &str) -> Result<Entropy, ErrorSigner> {
    let mut word_set = WordSet::new();
    for word in seed.split(' ') {
        word_set.add_word(&word, &InternalWordList)?;
    }
    Ok(word_set.to_entropy()?)
}

/// Standartized derivation protocol
pub fn derivations<'a>(recipient: &'a str, order: &'a str) -> FullDerivation<'a> {
    FullDerivation {
        junctions: vec![DeriveJunction::hard(recipient), DeriveJunction::hard(order)],
        password: None,
    }
}