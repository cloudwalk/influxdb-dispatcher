//! Utilities for aggregating metrics.

/// The average of a stream of values.
/// This is a compact representation, and will not store all elements. This comes at a
/// small precision cost, which should be negligible for metrics.
#[derive(Debug, Default)]
pub struct RunningAverage {
    /// Current average.
    average: f64,
    /// Current ammount of processed elements.
    count: u32,
}

impl RunningAverage {
    /// Accept a new value from the input stream.
    pub fn accept(&mut self, value: f64) {
        // M_n = (V_1 + ... + V_n) / n
        // M_(n+1) = (V_1 + ... + V_n + V_(n+1)) / (n+1)
        // M_(n+1) = (M_n * n + V_(n+1)) / (n+1)
        // M_(n+1) = ((M_n * n) / (n+1)) + (V_(n+1) / (n+1))
        // M_(n+1) = M_n * (n / (n+1)) + (V_(n+1) / (n+1))
        let count = self.count as f64;
        // Saturating should create a minor imprecision in the result.
        self.count = self.count.saturating_add(1);
        self.average = self.average * (count / (count + 1.0)) + (value / (count + 1.0));
    }

    /// Get the current average value.
    pub fn get(&self) -> f64 {
        self.average
    }

    /// Get the count of recorded values.
    pub fn count(&self) -> u32 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_average() {
        let mut running_avg = RunningAverage::default();
        for i in 1..=100 {
            running_avg.accept(i as f64);

            let range = 1..=i;
            let size = range.len() as f64;
            let avg = range.sum::<u16>() as f64 / size;

            assert_eq!(running_avg.get(), avg);
        }
    }
}
