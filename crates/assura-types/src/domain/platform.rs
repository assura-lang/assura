//! Platform-related domain checkers.
//!
//! PlatformAbstractionChecker, FeatureFlagChecker, ResourceLimitChecker.

use crate::TypeError;

// ===========================================================================
// T097: PLAT.1 Platform abstraction
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct PlatformAbstractionChecker {
    platforms: Vec<String>,
    abstractions: std::collections::HashMap<String, Vec<String>>,
}

impl PlatformAbstractionChecker {
    pub fn new() -> Self {
        Self {
            platforms: Vec::new(),
            abstractions: std::collections::HashMap::new(),
        }
    }

    pub fn known_platforms(&self) -> &[String] {
        &self.platforms
    }

    pub fn add_platform(&mut self, name: String) {
        if !self.platforms.contains(&name) {
            self.platforms.push(name);
        }
    }

    pub fn declare_abstraction(&mut self, name: String, supported: Vec<String>) {
        self.abstractions.insert(name, supported);
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for platform in &self.platforms {
                if !supported.contains(platform) {
                    errors.push(TypeError {
                        code: "A44001".into(),
                        message: format!(
                            "abstraction `{name}` missing impl for platform `{platform}`"
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_direct_platform_use(&self, used_platform: &str) -> Option<TypeError> {
        if self.platforms.contains(&used_platform.to_string()) {
            Some(TypeError {
                code: "A44002".into(),
                message: format!("direct platform reference `{used_platform}` without abstraction"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_unknown_platforms(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for p in supported {
                if !self.platforms.contains(p) {
                    errors.push(TypeError {
                        code: "A44003".into(),
                        message: format!("abstraction `{name}` references unknown platform `{p}`"),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for PlatformAbstractionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T098: PLAT.2 Feature flags
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct FeatureFlagChecker {
    flags: std::collections::HashMap<String, FeatureFlagInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct FeatureFlagInfo {
    pub default_enabled: bool,
    pub used: bool,
    pub conflicts_with: Vec<String>,
}

impl FeatureFlagChecker {
    pub fn new() -> Self {
        Self {
            flags: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, default_enabled: bool, conflicts_with: Vec<String>) {
        self.flags.insert(
            name,
            FeatureFlagInfo {
                default_enabled,
                used: false,
                conflicts_with,
            },
        );
    }

    pub fn mark_used(&mut self, name: &str) {
        if let Some(f) = self.flags.get_mut(name) {
            f.used = true;
        }
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        self.flags
            .iter()
            .filter(|(_, i)| !i.used)
            .map(|(n, _)| TypeError {
                code: "A45001".into(),
                message: format!("feature flag `{n}` is declared but never used"),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn check_conflicts(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, info) in &self.flags {
            if info.default_enabled {
                for conflict in &info.conflicts_with {
                    if let Some(other) = self.flags.get(conflict)
                        && other.default_enabled
                    {
                        errors.push(TypeError {
                            code: "A45002".into(),
                            message: format!(
                                "conflicting flags: `{name}` and `{conflict}` both enabled"
                            ),
                            span: 0..1,
                            secondary: None,
                        });
                    }
                }
            }
        }
        errors
    }

    pub fn check_undeclared(&self, flag_name: &str) -> Option<TypeError> {
        if !self.flags.contains_key(flag_name) {
            Some(TypeError {
                code: "A45003".into(),
                message: format!("reference to undeclared feature flag `{flag_name}`"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }
}

impl Default for FeatureFlagChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T099: PLAT.3 Resource limits
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct ResourceLimitChecker {
    limits: std::collections::HashMap<String, ResourceLimit>,
    usage: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResourceLimit {
    pub max_value: u64,
    pub unit: String,
}

impl ResourceLimitChecker {
    pub fn new() -> Self {
        Self {
            limits: std::collections::HashMap::new(),
            usage: std::collections::HashMap::new(),
        }
    }

    pub fn declare_limit(&mut self, name: String, max_value: u64, unit: String) {
        self.limits
            .insert(name.clone(), ResourceLimit { max_value, unit });
        self.usage.insert(name, 0);
    }

    pub fn record_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u += amount;
        }
    }

    pub fn release_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u = u.saturating_sub(amount);
        }
    }

    pub fn check_limits(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && current > limit.max_value
            {
                errors.push(TypeError {
                    code: "A46001".into(),
                    message: format!(
                        "resource `{name}` usage {current} exceeds limit {} {}",
                        limit.max_value, limit.unit
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_unbounded(&self, name: &str) -> Option<TypeError> {
        if !self.limits.contains_key(name) {
            Some(TypeError {
                code: "A46002".into(),
                message: format!("resource `{name}` used without declared limit"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_near_limit(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && limit.max_value > 0
                && current > limit.max_value / 10 * 9
            {
                errors.push(TypeError {
                    code: "A46003".into(),
                    message: format!(
                        "resource `{name}` at {}% of limit",
                        current / (limit.max_value / 100).max(1)
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }
}

impl Default for ResourceLimitChecker {
    fn default() -> Self {
        Self::new()
    }
}
