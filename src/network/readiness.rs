use core::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessStatus {
    Ready,
    NotReady,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadinessCheck {
    pub name: &'static str,
    pub status: ReadinessStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadinessSuite {
    pub checks: [ReadinessCheck; 3],
}

impl ReadinessSuite {
    pub fn run_with_probes(
        icmp_probe: impl FnOnce() -> bool,
        dns_probe: impl FnOnce() -> bool,
        https_probe: impl FnOnce() -> bool,
    ) -> Self {
        let icmp_status = if icmp_probe() {
            ReadinessStatus::Ready
        } else {
            ReadinessStatus::NotReady
        };
        let dns_status = if dns_probe() {
            ReadinessStatus::Ready
        } else {
            ReadinessStatus::NotReady
        };
        let https_status = if https_probe() {
            ReadinessStatus::Ready
        } else {
            ReadinessStatus::NotReady
        };

        Self {
            checks: [
                ReadinessCheck {
                    name: "icmp",
                    status: icmp_status,
                },
                ReadinessCheck {
                    name: "dns",
                    status: dns_status,
                },
                ReadinessCheck {
                    name: "https",
                    status: https_status,
                },
            ],
        }
    }

    pub fn is_ready(&self) -> bool {
        self.checks
            .iter()
            .all(|check| matches!(check.status, ReadinessStatus::Ready))
    }
}

impl fmt::Display for ReadinessStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadinessStatus::Ready => write!(f, "ready"),
            ReadinessStatus::NotReady => write!(f, "not-ready"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_suite_is_ready_when_all_checks_pass() {
        let suite = ReadinessSuite::run_with_probes(|| true, || true, || true);
        assert!(suite.is_ready());
        assert_eq!(suite.checks[0].status, ReadinessStatus::Ready);
        assert_eq!(suite.checks[1].status, ReadinessStatus::Ready);
        assert_eq!(suite.checks[2].status, ReadinessStatus::Ready);
    }

    #[test]
    fn readiness_suite_is_not_ready_when_any_check_fails() {
        let suite = ReadinessSuite::run_with_probes(|| true, || false, || true);
        assert!(!suite.is_ready());
        assert_eq!(suite.checks[0].status, ReadinessStatus::Ready);
        assert_eq!(suite.checks[1].status, ReadinessStatus::NotReady);
        assert_eq!(suite.checks[2].status, ReadinessStatus::Ready);
    }
}
