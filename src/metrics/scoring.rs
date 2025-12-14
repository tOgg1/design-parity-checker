use crate::types::MetricScores;

#[derive(Debug, Clone, Copy)]
pub struct ScoreWeights {
    pub pixel: f32,
    pub layout: f32,
    pub typography: f32,
    pub color: f32,
    pub content: f32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            pixel: 0.35,
            layout: 0.25,
            typography: 0.15,
            color: 0.15,
            content: 0.10,
        }
    }
}

impl ScoreWeights {
    pub fn sum(&self) -> f32 {
        self.pixel + self.layout + self.typography + self.color + self.content
    }
}

pub fn calculate_combined_score(scores: &MetricScores, weights: &ScoreWeights) -> f32 {
    let mut total_weight = 0.0f32;
    let mut weighted_sum = 0.0f32;

    if let Some(ref m) = scores.pixel {
        weighted_sum += weights.pixel * m.score;
        total_weight += weights.pixel;
    }

    if let Some(ref m) = scores.layout {
        weighted_sum += weights.layout * m.score;
        total_weight += weights.layout;
    }

    if let Some(ref m) = scores.typography {
        weighted_sum += weights.typography * m.score;
        total_weight += weights.typography;
    }

    if let Some(ref m) = scores.color {
        weighted_sum += weights.color * m.score;
        total_weight += weights.color;
    }

    if let Some(ref m) = scores.content {
        weighted_sum += weights.content * m.score;
        total_weight += weights.content;
    }

    if total_weight > 0.0 {
        weighted_sum / total_weight
    } else {
        0.0
    }
}
