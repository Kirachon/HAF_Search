use std::borrow::Cow;

pub const VECTOR_SIZE: usize = 512;
const NGRAM_LEN: usize = 3;

#[derive(Default, Clone)]
pub struct Vectorizer;

impl Vectorizer {
    pub fn new() -> Self {
        Self
    }

    pub fn encode(&self, text: &str) -> Vec<f32> {
        let normalized = normalize(text);
        if normalized.is_empty() {
            return vec![0.0; VECTOR_SIZE];
        }

        let mut vector = vec![0.0f32; VECTOR_SIZE];
        let bytes = normalized.as_bytes();

        if bytes.len() < NGRAM_LEN {
            let idx = hash_bytes(bytes) % VECTOR_SIZE as u32;
            vector[idx as usize] += 1.0;
        } else {
            for window in bytes.windows(NGRAM_LEN) {
                let idx = hash_bytes(window) % VECTOR_SIZE as u32;
                vector[idx as usize] += 1.0;
            }
        }

        normalize_vector(&mut vector);
        vector
    }
}

fn normalize(input: &str) -> Cow<'_, str> {
    Cow::Owned(input.trim().to_lowercase())
}

fn hash_bytes(bytes: &[u8]) -> u32 {
    let mut hash = 0u32;
    for &b in bytes {
        hash = hash.wrapping_mul(31).wrapping_add(b as u32);
    }
    hash
}

fn normalize_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in vector {
            *v /= norm;
        }
    }
}
