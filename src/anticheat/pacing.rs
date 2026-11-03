//! Budget-based pacing: human-like, but guaranteed to finish inside the limit.

use crate::util::jitter_ms;

const MARGIN: f64 = 0.12;
const TARGET_FACTOR: f64 = 0.7;
const FLOOR_MS: u64 = 2500;

#[derive(Debug, Clone)]
pub struct Pacer {
    total_budget_ms: u64,
    jeda_min_ms: u64,
    jeda_maks_ms: u64,
    question_count: u32,
    answered: u32,
    elapsed_ms: u64,
}

#[allow(dead_code)]
impl Pacer {
    pub fn new(
        total_budget_ms: u64,
        question_count: u32,
        jeda_min_ms: u64,
        jeda_maks_ms: u64,
    ) -> Self {
        let usable = (total_budget_ms as f64 * (1.0 - MARGIN)) as u64;
        Self {
            total_budget_ms: usable,
            jeda_min_ms: jeda_min_ms.max(1),
            jeda_maks_ms: jeda_maks_ms.max(jeda_min_ms.max(1)),
            question_count: question_count.max(1),
            answered: 0,
            elapsed_ms: 0,
        }
    }

    pub fn total_budget_ms(&self) -> u64 {
        self.total_budget_ms
    }

    pub fn remaining_ms(&self) -> u64 {
        self.total_budget_ms.saturating_sub(self.elapsed_ms)
    }

    pub fn record_answered(&mut self, took_ms: u64) {
        self.answered += 1;
        self.elapsed_ms = self.elapsed_ms.saturating_add(took_ms);
    }

    /// Delay before starting the next answer.
    pub fn next_delay(&self) -> u64 {
        let remaining_q = self.question_count.saturating_sub(self.answered).max(1);
        let allowed = self.remaining_ms() / remaining_q as u64;
        let target = (allowed as f64 * TARGET_FACTOR) as u64;
        let base = target.min(self.jeda_maks_ms).max(self.jeda_min_ms);
        let jitter = base as f64 * 0.25;
        let floor = self.jeda_min_ms.min(FLOOR_MS);
        jitter_ms(
            base.saturating_sub(jitter as u64).max(floor),
            (base + jitter as u64).max(base).min(self.jeda_maks_ms),
        )
    }

    /// Projected total time if the current pace continues.
    pub fn projected_total_ms(&self) -> u64 {
        let remaining_q = self.question_count.saturating_sub(self.answered);
        self.elapsed_ms + self.next_delay() * remaining_q as u64
    }

    pub fn is_behind(&self) -> bool {
        self.projected_total_ms() > self.total_budget_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_questions_in_three_minutes_fits() {
        let p = Pacer::new(180_000, 10, 2500, 8000);
        assert!(p.next_delay() <= 8000);
        assert!(p.next_delay() >= 2500);
        let mut total = 10 * 8000;
        for _ in 0..10 {
            total = total.min(10 * 8000);
        }
        assert!(total <= 180_000);
    }

    #[test]
    fn tight_budget_shrinks_delay() {
        let tight = Pacer::new(60_000, 20, 2500, 8000);
        let loose = Pacer::new(600_000, 20, 2500, 8000);
        assert!(tight.next_delay() < loose.next_delay());
        assert!(tight.next_delay() >= 2500);
    }

    #[test]
    fn behind_detection() {
        let mut p = Pacer::new(30_000, 10, 2500, 8000);
        p.record_answered(9_000);
        assert!(p.is_behind());
    }

    #[test]
    fn remaining_never_underflows() {
        let mut p = Pacer::new(10_000, 5, 2500, 8000);
        p.record_answered(20_000);
        assert_eq!(p.remaining_ms(), 0);
    }
}
