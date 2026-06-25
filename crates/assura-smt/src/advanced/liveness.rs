// ===========================================================================
// T094: CORE.8 Liveness contracts
// ===========================================================================

/// Liveness property kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum LivenessKind {
    Eventually,
    LeadsTo,
    EventuallyWithin(u64),
}

/// A liveness obligation.
#[derive(Debug, Clone)]
pub struct LivenessObligation {
    pub name: String,
    pub kind: LivenessKind,
    pub premise: String,
    pub conclusion: String,
    pub verified: bool,
}

/// Manages liveness contracts for verification.
#[derive(Debug, Clone)]
pub struct LivenessChecker {
    obligations: Vec<LivenessObligation>,
    fairness_assumptions: Vec<String>,
}

impl LivenessChecker {
    pub fn new() -> Self {
        Self {
            obligations: Vec::new(),
            fairness_assumptions: Vec::new(),
        }
    }

    pub fn add_obligation(
        &mut self,
        name: String,
        kind: LivenessKind,
        premise: String,
        conclusion: String,
    ) {
        self.obligations.push(LivenessObligation {
            name,
            kind,
            premise,
            conclusion,
            verified: false,
        });
    }

    pub fn add_fairness(&mut self, assumption: String) {
        self.fairness_assumptions.push(assumption);
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(o) = self.obligations.iter_mut().find(|o| o.name == name) {
            o.verified = true;
        }
    }

    /// Check for unverified liveness obligations.
    pub fn check_unverified(&self) -> Vec<String> {
        self.obligations
            .iter()
            .filter(|o| !o.verified)
            .map(|o| {
                format!(
                    "liveness obligation `{}` ({:?}) not verified",
                    o.name, o.kind
                )
            })
            .collect()
    }

    /// Check that eventually_within obligations have reasonable bounds.
    pub fn check_bounded(&self) -> Vec<String> {
        self.obligations
            .iter()
            .filter(|o| matches!(o.kind, LivenessKind::EventuallyWithin(t) if t == 0))
            .map(|o| format!("liveness obligation `{}` has zero time bound", o.name))
            .collect()
    }

    /// Check that leads_to obligations have fairness assumptions.
    pub fn check_fairness(&self) -> Vec<String> {
        if self.fairness_assumptions.is_empty() {
            let leads_to: Vec<_> = self
                .obligations
                .iter()
                .filter(|o| o.kind == LivenessKind::LeadsTo)
                .collect();
            if !leads_to.is_empty() {
                return vec![
                    "leads_to obligations present but no fairness assumptions declared".into(),
                ];
            }
        }
        vec![]
    }

    pub fn obligation_count(&self) -> usize {
        self.obligations.len()
    }
}

impl Default for LivenessChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liveness_new_is_empty() {
        let lc = LivenessChecker::new();
        assert_eq!(lc.obligation_count(), 0);
    }

    #[test]
    fn liveness_default_is_empty() {
        let lc = LivenessChecker::default();
        assert_eq!(lc.obligation_count(), 0);
    }

    #[test]
    fn liveness_add_obligation_increases_count() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "progress".into(),
            LivenessKind::Eventually,
            "true".into(),
            "done".into(),
        );
        assert_eq!(lc.obligation_count(), 1);
    }

    #[test]
    fn liveness_unverified_reported() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "termination".into(),
            LivenessKind::Eventually,
            "started".into(),
            "finished".into(),
        );
        let unverified = lc.check_unverified();
        assert_eq!(unverified.len(), 1);
        assert!(unverified[0].contains("termination"));
    }

    #[test]
    fn liveness_mark_verified_clears() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "term".into(),
            LivenessKind::Eventually,
            "a".into(),
            "b".into(),
        );
        lc.mark_verified("term");
        assert!(lc.check_unverified().is_empty());
    }

    #[test]
    fn liveness_mark_verified_unknown_is_noop() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "real".into(),
            LivenessKind::Eventually,
            "a".into(),
            "b".into(),
        );
        lc.mark_verified("fake");
        assert_eq!(lc.check_unverified().len(), 1);
    }

    #[test]
    fn liveness_zero_bound_detected() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "instant".into(),
            LivenessKind::EventuallyWithin(0),
            "a".into(),
            "b".into(),
        );
        let warnings = lc.check_bounded();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("zero time bound"));
    }

    #[test]
    fn liveness_nonzero_bound_ok() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "bounded".into(),
            LivenessKind::EventuallyWithin(100),
            "a".into(),
            "b".into(),
        );
        assert!(lc.check_bounded().is_empty());
    }

    #[test]
    fn liveness_leads_to_without_fairness_warns() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "resp".into(),
            LivenessKind::LeadsTo,
            "request".into(),
            "response".into(),
        );
        let warnings = lc.check_fairness();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("fairness"));
    }

    #[test]
    fn liveness_leads_to_with_fairness_ok() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "resp".into(),
            LivenessKind::LeadsTo,
            "req".into(),
            "res".into(),
        );
        lc.add_fairness("scheduler is fair".into());
        assert!(lc.check_fairness().is_empty());
    }

    #[test]
    fn liveness_eventually_no_fairness_needed() {
        let mut lc = LivenessChecker::new();
        lc.add_obligation(
            "term".into(),
            LivenessKind::Eventually,
            "a".into(),
            "b".into(),
        );
        assert!(lc.check_fairness().is_empty());
    }
}
