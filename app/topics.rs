//! Experimental topic discovery. CF compression nodes are not semantic parents.
mod birch;
#[cfg(test)]
mod evaluation;
mod grouping;
mod hierarchy;
mod labels;
mod segments;
#[cfg(test)]
mod tests;

use crate::AppError;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct TopicId(u64);

#[derive(Clone, Copy)]
struct Settings {
    dimensions: usize,
    radius: f64,
    branching: usize,
    max_topics: usize,
    routing: birch::Routing,
}
impl Settings {
    fn validate(self) -> Result<(), AppError> {
        if !(1..=2048).contains(&self.dimensions)
            || !self.radius.is_finite()
            || !(0.0..1.0).contains(&self.radius)
            || !(2..=128).contains(&self.branching)
            || !(1..=8192).contains(&self.max_topics)
        {
            return Err(AppError::usage("invalid experimental topic settings"));
        }
        Ok(())
    }
}

fn normalize(vector: &[f64], dimensions: usize) -> Result<Vec<f64>, AppError> {
    if vector.len() != dimensions || vector.iter().any(|x| !x.is_finite()) {
        return Err(AppError::model(
            "invalid topic embedding dimensions or values",
        ));
    }
    let scale = vector.iter().map(|x| x.abs()).fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return Err(AppError::model("zero topic embedding"));
    }
    let norm = vector
        .iter()
        .map(|x| (x / scale).powi(2))
        .sum::<f64>()
        .sqrt();
    Ok(vector.iter().map(|x| (x / scale) / norm).collect())
}

#[derive(Serialize)]
struct Assignment {
    topic: TopicId,
    cosine: f64,
}

struct AssignmentModel {
    dimensions: usize,
    centroids: Vec<(TopicId, Vec<f64>)>,
}
impl AssignmentModel {
    fn new(dimensions: usize, centroids: Vec<(TopicId, Vec<f64>)>) -> Result<Self, AppError> {
        if !(1..=2048).contains(&dimensions) {
            return Err(AppError::model("invalid topic dimensions"));
        }
        let centroids = centroids
            .into_iter()
            .map(|(id, vector)| normalize(&vector, dimensions).map(|vector| (id, vector)))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            dimensions,
            centroids,
        })
    }
    fn assign(
        &self,
        vector: &[f64],
        minimum: f64,
        relative: f64,
    ) -> Result<Vec<Assignment>, AppError> {
        if !minimum.is_finite()
            || !(0.0..=1.0).contains(&minimum)
            || !relative.is_finite()
            || !(0.0..=1.0).contains(&relative)
        {
            return Err(AppError::usage("invalid topic assignment thresholds"));
        }
        let vector = normalize(vector, self.dimensions)?;
        let mut matches = Vec::with_capacity(self.centroids.len());
        for (topic, centroid) in &self.centroids {
            let cosine = vector
                .iter()
                .zip(centroid)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                .clamp(-1.0, 1.0);
            matches.push(Assignment {
                topic: *topic,
                cosine,
            });
        }
        let best = matches.iter().map(|a| a.cosine).fold(-1.0_f64, f64::max);
        matches.retain(|a| a.cosine >= minimum && a.cosine >= best * relative);
        matches.sort_by(|a, b| b.cosine.total_cmp(&a.cosine).then(a.topic.cmp(&b.topic)));
        Ok(matches)
    }
}
