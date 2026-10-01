//! Bounded streaming segment means; preserve topic shifts without dropping input.
use super::{labels::Terms, normalize};
use crate::AppError;

#[derive(Clone)]
pub(super) struct Segment {
    pub(super) vector: Vec<f64>,
    pub(super) words: Terms,
    pub(super) messages: usize,
}

struct Pending {
    sum: Vec<f64>,
    words: Terms,
    messages: usize,
}
impl Pending {
    fn finish(self, dimensions: usize) -> Result<Segment, AppError> {
        Ok(Segment {
            vector: normalize(&self.sum, dimensions)?,
            words: self.words,
            messages: self.messages,
        })
    }
}

pub(super) struct Builder {
    dimensions: usize,
    maximum: usize,
    similarity: f64,
    pending: Option<Pending>,
}
impl Builder {
    pub(super) fn new(
        dimensions: usize,
        maximum: usize,
        similarity: f64,
    ) -> Result<Self, AppError> {
        if !(1..=2048).contains(&dimensions)
            || !(1..=128).contains(&maximum)
            || !similarity.is_finite()
            || !(0.0..=1.0).contains(&similarity)
        {
            return Err(AppError::usage("invalid topic segment settings"));
        }
        Ok(Self {
            dimensions,
            maximum,
            similarity,
            pending: None,
        })
    }

    /// Emit a completed segment on capacity or a semantic shift. Invalid input
    /// leaves the current segment intact. Each valid message contributes once.
    pub(super) fn push(
        &mut self,
        vector: &[f64],
        words: &Terms,
    ) -> Result<Option<Segment>, AppError> {
        let vector = normalize(vector, self.dimensions)?;
        let split = match &self.pending {
            Some(pending) => {
                let center = normalize(&pending.sum, self.dimensions)?;
                pending.messages >= self.maximum
                    || center.iter().zip(&vector).map(|(a, b)| a * b).sum::<f64>() < self.similarity
            }
            None => false,
        };
        let complete = if split {
            self.pending
                .take()
                .map(|p| p.finish(self.dimensions))
                .transpose()?
        } else {
            None
        };
        let pending = self.pending.get_or_insert_with(|| Pending {
            sum: vec![0.0; self.dimensions],
            words: Terms::new(),
            messages: 0,
        });
        for (sum, value) in pending.sum.iter_mut().zip(vector) {
            *sum += value;
        }
        for word in words.keys() {
            *pending.words.entry(word.clone()).or_default() += 1;
        }
        pending.messages += 1;
        Ok(complete)
    }

    pub(super) fn finish(self) -> Result<Option<Segment>, AppError> {
        self.pending.map(|p| p.finish(self.dimensions)).transpose()
    }
}

#[test]
fn streaming_segments_preserve_all_messages_and_separate_topic_words() {
    let mut builder = Builder::new(2, 2, 0.8).unwrap();
    let database = Terms::from([("database".into(), 7)]);
    let frontend = Terms::from([("frontend".into(), 1)]);
    assert!(builder.push(&[1.0, 0.0], &database).unwrap().is_none());
    assert!(builder.push(&[2.0, 0.0], &database).unwrap().is_none());
    assert!(builder.push(&[f64::NAN, 0.0], &database).is_err());
    let first = builder.push(&[1.0, 0.0], &database).unwrap().unwrap();
    assert_eq!(first.messages, 2);
    assert_eq!(first.vector, [1.0, 0.0]);
    assert_eq!(first.words["database"], 2);
    let second = builder.push(&[0.0, 1.0], &frontend).unwrap().unwrap();
    assert_eq!(second.messages, 1);
    assert!(!second.words.contains_key("frontend"));
    let third = builder.finish().unwrap().unwrap();
    assert_eq!(third.messages, 1);
    assert_eq!(third.vector, [0.0, 1.0]);
    assert!(!third.words.contains_key("database"));
    assert!(Builder::new(2, 0, 0.8).is_err());
    assert!(Builder::new(2, 2, f64::NAN).is_err());
    assert!(Builder::new(0, 2, 0.8).is_err());
}
