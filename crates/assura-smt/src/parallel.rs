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
