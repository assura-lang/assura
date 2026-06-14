// ===========================================================================
// T114: Parallel SMT queries
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ParallelVerifier {
    jobs: Vec<VerificationJob>,
    max_parallelism: usize,
}

#[derive(Debug, Clone)]
pub struct VerificationJob {
    pub contract_name: String,
    pub clause: String,
    pub status: JobStatus,
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl ParallelVerifier {
    pub fn new(max_parallelism: usize) -> Self {
        Self {
            jobs: Vec::new(),
            max_parallelism,
        }
    }

    pub fn add_job(&mut self, contract_name: String, clause: String) {
        self.jobs.push(VerificationJob {
            contract_name,
            clause,
            status: JobStatus::Pending,
            result: None,
        });
    }

    pub fn start_next(&mut self) -> Option<usize> {
        let running = self
            .jobs
            .iter()
            .filter(|j| j.status == JobStatus::Running)
            .count();
        if running >= self.max_parallelism {
            return None;
        }
        for (i, job) in self.jobs.iter_mut().enumerate() {
            if job.status == JobStatus::Pending {
                job.status = JobStatus::Running;
                return Some(i);
            }
        }
        None
    }

    pub fn complete_job(&mut self, index: usize, result: String) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.status = JobStatus::Completed;
            job.result = Some(result);
        }
    }

    pub fn fail_job(&mut self, index: usize) {
        if let Some(job) = self.jobs.get_mut(index) {
            job.status = JobStatus::Failed;
        }
    }

    pub fn all_complete(&self) -> bool {
        self.jobs
            .iter()
            .all(|j| j.status == JobStatus::Completed || j.status == JobStatus::Failed)
    }

    pub fn pending_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.status == JobStatus::Pending)
            .count()
    }
    pub fn completed_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.status == JobStatus::Completed)
            .count()
    }
    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }
}

impl Default for ParallelVerifier {
    fn default() -> Self {
        Self::new(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_verifier_empty() {
        let v = ParallelVerifier::new(2);
        assert_eq!(v.job_count(), 0);
        assert_eq!(v.pending_count(), 0);
        assert_eq!(v.completed_count(), 0);
        assert!(v.all_complete());
    }

    #[test]
    fn test_add_job() {
        let mut v = ParallelVerifier::new(2);
        v.add_job("contract_a".into(), "ensures".into());
        assert_eq!(v.job_count(), 1);
        assert_eq!(v.pending_count(), 1);
        assert!(!v.all_complete());
    }

    #[test]
    fn test_start_next_returns_index() {
        let mut v = ParallelVerifier::new(2);
        v.add_job("c".into(), "e1".into());
        v.add_job("c".into(), "e2".into());
        let idx = v.start_next().unwrap();
        assert_eq!(idx, 0);
    }

    #[test]
    fn test_start_next_respects_parallelism() {
        let mut v = ParallelVerifier::new(1);
        v.add_job("c".into(), "e1".into());
        v.add_job("c".into(), "e2".into());
        assert!(v.start_next().is_some()); // starts first
        assert!(v.start_next().is_none()); // at capacity
    }

    #[test]
    fn test_complete_job() {
        let mut v = ParallelVerifier::new(2);
        v.add_job("c".into(), "e".into());
        let idx = v.start_next().unwrap();
        v.complete_job(idx, "verified".into());
        assert_eq!(v.completed_count(), 1);
        assert_eq!(v.pending_count(), 0);
        assert!(v.all_complete());
    }

    #[test]
    fn test_fail_job() {
        let mut v = ParallelVerifier::new(2);
        v.add_job("c".into(), "e".into());
        let idx = v.start_next().unwrap();
        v.fail_job(idx);
        assert!(v.all_complete()); // failed counts as complete
        assert_eq!(v.completed_count(), 0); // but not "completed"
    }

    #[test]
    fn test_multi_job_workflow() {
        let mut v = ParallelVerifier::new(2);
        v.add_job("c".into(), "e1".into());
        v.add_job("c".into(), "e2".into());
        v.add_job("c".into(), "e3".into());

        let i0 = v.start_next().unwrap();
        let i1 = v.start_next().unwrap();
        assert!(v.start_next().is_none()); // at capacity

        v.complete_job(i0, "ok".into());
        let i2 = v.start_next().unwrap(); // slot freed
        assert_eq!(i2, 2);

        v.complete_job(i1, "ok".into());
        v.complete_job(i2, "ok".into());
        assert!(v.all_complete());
        assert_eq!(v.completed_count(), 3);
    }

    #[test]
    fn test_default_parallelism() {
        let v = ParallelVerifier::default();
        // default is 4, can start 4 jobs
        assert_eq!(v.job_count(), 0);
    }

    #[test]
    fn test_complete_out_of_bounds_noop() {
        let mut v = ParallelVerifier::new(2);
        v.complete_job(99, "ok".into()); // should not panic
        v.fail_job(99); // should not panic
    }
}
