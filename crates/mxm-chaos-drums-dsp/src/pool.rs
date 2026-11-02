//! The one supply every held note draws from.

use crate::kit::CELLS;

/// Within this of its capacity the pool is full, exactly.
const FULL_FLOOR: f32 = 1e-6;
/// The capacity at Pool 100%, in cells' worth of charge.
const CAPACITY_FULL: f32 = 4.0;

pub(crate) struct Pool {
    level: f32,
    capacity: f32,
    /// The share of the gap refilled each sample.
    recharge: f32,
}

impl Pool {
    pub fn new(pool: f32, recharge_ms: f32, sample_rate: f32) -> Self {
        let mut it = Self {
            level: 0.0,
            capacity: 0.0,
            recharge: 0.0,
        };
        it.set(pool, recharge_ms, sample_rate);
        it.level = it.capacity;
        it
    }

    pub fn set(&mut self, pool: f32, recharge_ms: f32, sample_rate: f32) {
        self.capacity = CAPACITY_FULL * pool.clamp(0.0, 1.0);
        self.level = self.level.min(self.capacity);
        self.recharge = (1000.0 / (recharge_ms.max(1.0) * sample_rate)).min(1.0);
    }

    pub fn fill(&mut self) {
        self.level = self.capacity;
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_full(&self) -> bool {
        self.level == self.capacity
    }

    /// Divide the pool among every demand at once: each gets the same share of what it asked
    /// for, so no cell's index decides who goes short.
    pub fn grant(&mut self, demands: [f32; CELLS]) -> [f32; CELLS] {
        let total: f32 = demands.iter().sum();
        if total <= 0.0 {
            return [0.0; CELLS];
        }
        let share = if total <= self.level {
            1.0
        } else {
            self.level / total
        };
        self.level = (self.level - total * share).max(0.0);
        demands.map(|demand| demand * share)
    }

    /// Refill toward the capacity. Once a step is too small to move the level at all — near a
    /// capacity of 4 that happens with the gap still in the thousandths — it is full, exactly, or
    /// the pool would stall short of full and the kit would never rest.
    pub fn recharge(&mut self) {
        let next = self.level + (self.capacity - self.level) * self.recharge;
        self.level = if next == self.level || self.capacity - next < FULL_FLOOR {
            self.capacity
        } else {
            next
        };
    }
}
