use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Performance {
    pub preset: String,
    pub upload_concurrency: usize,
    pub model_processing_concurrency: usize,
}
impl Default for Performance {
    fn default() -> Self {
        Self {
            preset: "medium".into(),
            upload_concurrency: 4,
            model_processing_concurrency: 2,
        }
    }
}
impl Performance {
    pub fn resolved(&self) -> Result<(usize, usize), crate::error::CloudError> {
        if !(0..=32).contains(&self.upload_concurrency)
            || !(0..=16).contains(&self.model_processing_concurrency)
        {
            return Err(crate::error::CloudError::invalid_metadata(
                "并发范围：上传 0–32，扫描与解码 0–16；0 表示不限制",
            ));
        }
        match self.preset.as_str() {
            "low" => Ok((2, 1)),
            "medium" => Ok((4, 2)),
            "high" => Ok((8, 4)),
            "custom" => Ok((self.upload_concurrency, self.model_processing_concurrency)),
            _ => Err(crate::error::CloudError::invalid_metadata("无效的性能预设")),
        }
    }
}

/// Existing jobs retain their permits when the limit is reduced. New jobs wait
/// until the active count falls below the new limit; no permit pool is replaced.
pub struct Limiter {
    counts: Mutex<(usize, usize, usize)>,
    notify: Notify,
}
impl Limiter {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            counts: Mutex::new((0, limit, 0)),
            notify: Notify::new(),
        })
    }
    pub fn set_limit(&self, limit: usize) {
        self.counts.lock().unwrap().1 = limit;
        self.notify.notify_waiters();
    }
    pub fn pause(self: &Arc<Self>) -> Pause {
        self.counts.lock().unwrap().2 += 1;
        Pause(self.clone())
    }
    pub async fn idle(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.counts.lock().unwrap().0 == 0 {
                return;
            }
            notified.await;
        }
    }
    pub async fn acquire(self: &Arc<Self>) -> Permit {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut counts = self.counts.lock().unwrap();
                if counts.2 == 0 && (counts.1 == 0 || counts.0 < counts.1) {
                    counts.0 += 1;
                    return Permit(self.clone());
                }
            }
            notified.await;
        }
    }
}
pub struct Pause(Arc<Limiter>);
impl Drop for Pause {
    fn drop(&mut self) {
        self.0.counts.lock().unwrap().2 -= 1;
        self.0.notify.notify_waiters();
    }
}
pub struct Permit(Arc<Limiter>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.counts.lock().unwrap().0 -= 1;
        self.0.notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn maintenance_pause_drains_jobs_and_resumes_admission() {
        let limiter = Limiter::new(0);
        let job = limiter.acquire().await;
        let pause = limiter.pause();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), limiter.idle())
                .await
                .is_err()
        );
        drop(job);
        limiter.idle().await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), limiter.acquire())
                .await
                .is_err()
        );
        drop(pause);
        let _job = limiter.acquire().await;
    }
    #[tokio::test]
    async fn zero_means_unlimited_and_can_be_reduced_without_losing_jobs() {
        let settings = Performance {
            preset: "custom".into(),
            upload_concurrency: 0,
            model_processing_concurrency: 0,
        };
        assert_eq!(settings.resolved().unwrap(), (0, 0));
        assert_eq!(Performance::default().resolved().unwrap(), (4, 2));
        let limiter = Limiter::new(0);
        let mut jobs = Vec::new();
        for _ in 0..100 {
            jobs.push(limiter.acquire().await);
        }
        limiter.set_limit(1);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), limiter.acquire())
                .await
                .is_err()
        );
        jobs.clear();
        let _permit = limiter.acquire().await;
    }
    #[tokio::test]
    async fn reduction_waits_for_existing_jobs_and_increase_wakes_waiters() {
        let limiter = Limiter::new(2);
        let first = limiter.acquire().await;
        let second = limiter.acquire().await;
        limiter.set_limit(1);
        let pending = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.acquire().await }
        });
        drop(first);
        tokio::task::yield_now().await;
        assert!(!pending.is_finished());
        limiter.set_limit(3);
        let third = tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap();
        drop((second, third));
        assert_eq!(limiter.counts.lock().unwrap().0, 0);
    }
}
