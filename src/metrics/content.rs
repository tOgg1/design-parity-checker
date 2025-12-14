use crate::types::{ContentMetric, NormalizedView};
use crate::Result;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy)]
pub struct ContentSimilarity {
    pub match_threshold: f32,
    pub extra_penalty_weight: f32,
}

impl Default for ContentSimilarity {
    fn default() -> Self {
        Self {
            match_threshold: 0.7,
            extra_penalty_weight: 0.2,
        }
    }
}

impl ContentSimilarity {
    pub fn compute_metric(
        &self,
        reference: &NormalizedView,
        implementation: &NormalizedView,
    ) -> Result<ContentMetric> {
        let ref_texts = extract_texts(reference);
        let impl_texts = extract_texts(implementation);

        if ref_texts.is_empty() && impl_texts.is_empty() {
            return Ok(ContentMetric {
                score: 1.0,
                missing_text: vec![],
                extra_text: vec![],
            });
        }

        let normalized_ref: Vec<(String, String)> = ref_texts
            .into_iter()
            .filter_map(|original| normalize_text(&original).map(|norm| (original, norm)))
            .collect();
        let normalized_impl: Vec<(String, String)> = impl_texts
            .into_iter()
            .filter_map(|original| normalize_text(&original).map(|norm| (original, norm)))
            .collect();

        if normalized_ref.is_empty() && normalized_impl.is_empty() {
            return Ok(ContentMetric {
                score: 1.0,
                missing_text: vec![],
                extra_text: vec![],
            });
        }

        let mut matched_impl = vec![false; normalized_impl.len()];
        let mut matched_count = 0usize;
        let mut missing_text = Vec::new();

        for (ref_orig, ref_norm) in &normalized_ref {
            let mut best_score = 0.0f32;
            let mut best_idx = None;

            for (idx, (_impl_orig, impl_norm)) in normalized_impl.iter().enumerate() {
                let score = token_similarity(ref_norm, impl_norm);
                if score > best_score {
                    best_score = score;
                    best_idx = Some(idx);
                }
            }

            if best_score >= self.match_threshold {
                matched_count += 1;
                if let Some(idx) = best_idx {
                    matched_impl[idx] = true;
                }
            } else {
                missing_text.push(ref_orig.clone());
            }
        }

        let extra_text: Vec<String> = normalized_impl
            .iter()
            .enumerate()
            .filter_map(|(idx, (orig, _))| {
                if matched_impl[idx] {
                    None
                } else {
                    Some(orig.clone())
                }
            })
            .collect();

        let ref_len = normalized_ref.len() as f32;
        let base_score = if ref_len == 0.0 {
            1.0
        } else {
            matched_count as f32 / ref_len
        };

        let ref_chars: usize = normalized_ref.iter().map(|(orig, _)| orig.len()).sum();
        let extra_chars: usize = extra_text.iter().map(|s| s.len()).sum();
        let penalty = if ref_chars == 0 {
            0.0
        } else {
            let frac = extra_chars as f32 / ref_chars as f32;
            (frac * self.extra_penalty_weight).min(0.5)
        };

        let score = (base_score - penalty).clamp(0.0, 1.0);

        Ok(ContentMetric {
            score,
            missing_text,
            extra_text,
        })
    }
}

fn extract_texts(view: &NormalizedView) -> Vec<String> {
    let mut texts = Vec::new();

    if let Some(dom) = &view.dom {
        for node in &dom.nodes {
            if let Some(text) = &node.text {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    texts.push(trimmed.to_string());
                }
            }
        }
    }

    if let Some(figma) = &view.figma_tree {
        for node in &figma.nodes {
            if let Some(text) = &node.text {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    texts.push(trimmed.to_string());
                }
            }
        }
    }

    if let Some(blocks) = &view.ocr_blocks {
        for block in blocks {
            let trimmed = block.text.trim();
            if !trimmed.is_empty() {
                texts.push(trimmed.to_string());
            }
        }
    }

    texts
}

fn normalize_text(input: &str) -> Option<String> {
    let lower = input.to_lowercase();
    let mut cleaned = String::new();

    for ch in lower.chars() {
        if ch.is_alphanumeric() {
            cleaned.push(ch);
        } else if ch.is_whitespace() {
            cleaned.push(' ');
        }
    }

    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn token_similarity(a: &str, b: &str) -> f32 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }

    let intersection = set_a.intersection(&set_b).count() as f32;
    let denom = (set_a.len() + set_b.len()) as f32;

    if denom == 0.0 {
        0.0
    } else {
        (2.0 * intersection) / denom
    }
}
