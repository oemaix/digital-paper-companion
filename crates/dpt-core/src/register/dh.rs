//! Diffie-Hellman exchange, RFC 3526 MODP group 14 (protocol §4.4).

use num_bigint::BigUint;
use rand::RngCore;

/// RFC 3526 group 14 prime `p` (2048-bit), hex from protocol Appendix A.
const PRIME_HEX: &str = "\
FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E08\
8A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B\
302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9\
A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE6\
49286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8\
FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D\
670C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39E772C\
180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF695581718\
3995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFF\
FFFFFFFF";

/// Byte length of encoded public keys / shared secret (2048 bits).
pub const MODULUS_BYTES: usize = 256;

fn prime() -> BigUint {
    BigUint::parse_bytes(PRIME_HEX.as_bytes(), 16).expect("valid RFC 3526 prime")
}

/// A client Diffie-Hellman keypair for one registration attempt.
pub struct DhKeypair {
    private: BigUint,
    public: BigUint,
}

impl DhKeypair {
    /// Generates a keypair with a fresh 256-bit private exponent.
    pub fn generate() -> Self {
        let mut buf = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut buf);
        Self::from_private_bytes(&buf)
    }

    /// Constructs a keypair from explicit private-key bytes (for tests /
    /// reproducibility). `A = g^a mod p`, `g = 2`.
    pub fn from_private_bytes(bytes: &[u8]) -> Self {
        let private = BigUint::from_bytes_be(bytes);
        let public = BigUint::from(2u32).modpow(&private, &prime());
        Self { private, public }
    }

    /// Encodes the client public key `ya` for the wire: a single `0x00`
    /// byte followed by `A` as a 256-byte big-endian integer (257 bytes
    /// total), mimicking Java's `BigInteger.toByteArray()` sign byte
    /// (protocol §4.4).
    pub fn encode_public(&self) -> Vec<u8> {
        let mut out = vec![0u8; MODULUS_BYTES + 1];
        let be = self.public.to_bytes_be();
        // Right-align into out[1..]; leaves the leading 0x00 sign byte and
        // any additional leading zeros.
        let start = out.len() - be.len();
        out[start..].copy_from_slice(&be);
        out
    }

    /// Computes the shared secret `ZZ = yb_int^a mod p`, encoded as a
    /// 256-byte big-endian integer (protocol §4.4). `yb_raw` is the device
    /// public key exactly as received (256 or 257 bytes); only its integer
    /// value is used here.
    pub fn shared_secret(&self, yb_raw: &[u8]) -> Result<Vec<u8>, crate::Error> {
        let p = prime();
        let yb = BigUint::from_bytes_be(yb_raw);
        // Basic sanity per NIST SP 800-56 (2 <= yb <= p-2).
        let two = BigUint::from(2u32);
        if yb < two || yb > &p - &two {
            return Err(crate::Error::Crypto(
                "device DH public key out of range".into(),
            ));
        }
        let zz = yb.modpow(&self.private, &p);
        let mut be = zz.to_bytes_be();
        if be.len() > MODULUS_BYTES {
            return Err(crate::Error::Crypto("shared secret too large".into()));
        }
        // Left-pad to exactly 256 bytes.
        if be.len() < MODULUS_BYTES {
            let mut padded = vec![0u8; MODULUS_BYTES - be.len()];
            padded.append(&mut be);
            be = padded;
        }
        Ok(be)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_public_is_257_bytes_with_sign_byte() {
        let kp = DhKeypair::from_private_bytes(&[0x42; 32]);
        let ya = kp.encode_public();
        assert_eq!(ya.len(), MODULUS_BYTES + 1);
        assert_eq!(ya[0], 0x00);
    }

    #[test]
    fn shared_secret_matches_between_parties() {
        // Two parties compute the same secret; also checks 256-byte output.
        let a = DhKeypair::from_private_bytes(&[0x11; 32]);
        let b = DhKeypair::from_private_bytes(&[0x22; 32]);
        // Each side's raw public key (minimal big-endian) feeds the other.
        let a_pub = a.public.to_bytes_be();
        let b_pub = b.public.to_bytes_be();
        let za = a.shared_secret(&b_pub).unwrap();
        let zb = b.shared_secret(&a_pub).unwrap();
        assert_eq!(za, zb);
        assert_eq!(za.len(), MODULUS_BYTES);
    }
}
