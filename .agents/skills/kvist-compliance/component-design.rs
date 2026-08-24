//! Component design skill.
//!
//! This skill defines a component's context, public contract, and constraints
//! from a product vision without accessing the full source tree. It produces
//! a feasibility report that identifies unresolved decisions, contradictions,
//! bounds, and failure paths.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A component context defined from a product vision.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ComponentContext {
    /// Unique identifier for this component.
    pub id: String,
    /// Human-readable name of the component.
    pub name: String,
    /// The product vision or goal this component addresses.
    pub vision: String,
    /// The public contract (interface) of the component.
    pub contract: String,
    /// Constraints and boundaries for this component.
    pub constraints: Vec<String>,
    /// List of unresolved decisions (open questions).
    pub unresolved: Vec<UnresolvedDecision>,
    /// Contradictions identified during analysis.
    pub contradictions: Vec<Contradiction>,
    /// Bounds and assumptions.
    pub bounds: Vec<String>,
    /// Potential failure paths.
    pub failure_paths: Vec<FailurePath>,
    /// Timestamp when the context was created.
    pub created_at: String,
}

/// An unresolved decision requiring human resolution.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnresolvedDecision {
    /// Title of the decision.
    pub title: String,
    /// Description of the decision.
    pub description: String,
    /// Options for this decision.
    pub options: Vec<String>,
}

/// A contradiction found in the specification.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Contradiction {
    /// Title of the contradiction.
    pub title: String,
    /// Description of the contradiction.
    pub description: String,
    /// The conflicting requirements or statements.
    pub conflicting_items: Vec<String>,
}

/// A potential failure path.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FailurePath {
    /// Title of the failure path.
    pub title: String,
    /// Description of how the failure can occur.
    pub description: String,
    /// Mitigation strategy.
    pub mitigation: String,
}

/// Feasibility report produced by the component design skill.
#[derive(Debug, Serialize, Deserialize)]
pub struct FeasibilityReport {
    /// The component context.
    pub context: ComponentContext,
    /// Overall feasibility assessment.
    pub is_feasible: bool,
    /// Summary of findings.
    pub summary: String,
    /// Detailed findings.
    pub findings: Vec<FeasibilityFinding>,
    /// Recommendations.
    pub recommendations: Vec<String>,
}

/// A single feasibility finding.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

impl ComponentDesignSkill {
    /// Creates a new component design skill.
    pub fn new() -> Self {
        Self {
            working_directory: PathBuf::from(".kvist"),
        }
    }

    /// Runs a component design session.
    ///
    /// This method orchestrates the component design process by:
    /// 1. Collecting input from the product vision.
    /// 2. Identifying constraints and boundaries.
    /// 3. Detecting contradictions.
    /// 4. Identifying unresolved decisions.
    /// 5. Enumerating failure paths.
    /// 6. Producing a feasibility report.
    pub fn design(&self, vision: &str, contract: &str, constraints: &[String]) -> Result<FeasibilityReport> {
        // Step 1: Parse the vision and contract
        let vision_lower = vision.to_lowercase();
        let contract_lower = contract.to_lowercase();

        // Step 2: Identify unresolved decisions
        let unresolved = self.identify_unresolved_decisions(&vision_lower, &contract_lower);

        // Step 3: Identify contradictions
        let contradictions = self.identify_contradictions(&vision_lower, &contract_lower);

        // Step 4: Identify bounds and assumptions
        let bounds = self.identify_bounds(&vision_lower, &contract_lower);

        // Step 5: Identify failure paths
        let failure_paths = self.identify_failure_paths(&vision_lower, &contract_lower);

        // Step 6: Determine feasibility
        let is_feasible = !contradictions.is_empty();

        // Step 7: Generate summary and recommendations
        let summary = if is_feasible {
            "The component is feasible with minor caveats. The following items require human resolution before proceeding.".to_string()
        } else {
            "The component has contradictions that must be resolved before proceeding.".to_string()
        };

        let recommendations = self.generate_recommendations(&contradictions, &unresolved);

        Ok(FeasibilityReport {
            context: ComponentContext {
                id: format!("comp-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()),
                name: "component-design-session".to_string(),
                vision: vision.to_string(),
                contract: contract.to_string(),
                constraints: constraints.to_vec(),
                unresolved,
                contradictions,
                bounds,
                failure_paths,
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    .to_string(),
            },
            is_feasible,
            summary,
            findings: self.generate_findings(&contradictions, &unresolved, &failure_paths),
            recommendations,
        })
    }

    /// Identifies unresolved decisions in the vision and contract.
    fn identify_unresolved_decisions(&self, vision: &str, contract: &str) -> Vec<UnresolvedDecision> {
        let mut decisions = Vec::new();

        // Check for ambiguous language
        let ambiguous_terms = [
            ("highly", "highly available"),
            ("fast", "low latency"),
            ("secure", "security-hardened"),
            ("cheap", "cost-efficient"),
            ("simple", "easy to maintain"),
        ];

        for (word, expanded) in ambiguous_terms.iter() {
            if vision.contains(word) || contract.contains(word) {
                decisions.push(UnresolvedDecision {
                    title: format!("Ambiguous term: '{}'", expanded),
                    description: format!(
                        "The term '{}' appears in the vision/contract and is ambiguous.\n\
                         Please specify the precise requirement."
                    ),
                    options: vec![
                        format!("Add a precise definition for '{}'", expanded),
                        "Remove the term and replace with a precise alternative".to_string(),
                    ],
                });
            }
        }

        // Check for missing scope boundaries
        if !vision.contains("scope") && !vision.contains("bound") {
            decisions.push(UnresolvedDecision {
                title: "Missing scope boundary".to_string(),
                description: "No explicit scope boundary is defined. The component may grow\n\
                             without bounds, leading to scope creep.".to_string(),
                options: vec![
                    "Define an explicit scope boundary.".to_string(),
                    "Define a 'out of scope' list.".to_string(),
                ],
            });
        }

        // Check for missing interface definition
        if !contract.contains("interface") && !contract.contains("api") {
            decisions.push(UnresolvedDecision {
                title: "Missing interface definition".to_string(),
                description: "The contract does not define an explicit interface.\n\
                             This makes integration with other components uncertain.".to_string(),
                options: vec![
                    "Define a REST API.".to_string(),
                    "Define a gRPC service.".to_string(),
                    "Define a message protocol.".to_string(),
                ],
            });
        }

        decisions
    }

    /// Identifies contradictions between vision and contract.
    fn identify_contradictions(&self, vision: &str, contract: &str) -> Vec<Contradiction> {
        let mut contradictions = Vec::new();

        // Contradiction: "simple" vs "secure"
        if vision.contains("simple") || vision.contains("simplest") {
            if contract.contains("secure") || contract.contains("encryption") || contract.contains("authentication") {
                contradictions.push(Contradiction {
                    title: "Simplicity vs. Security".to_string(),
                    description: "The vision emphasizes simplicity while the contract requires security.\n\
                                 These are often at odds. Simplicity can be achieved through\n\
                                 abstraction and encapsulation, not by removing security\n\
                                 mechanisms.".to_string(),
                    conflicting_items: vec![
                        if vision.contains("simple") { "vision: simplicity".to_string() } else { "vision: simplest".to_string() },
                        "contract: security requirements".to_string(),
                    ],
                });
            }
        }

        // Contradiction: "fast" vs "thorough"
        if vision.contains("fast") || vision.contains("quick") {
            if contract.contains("thorough") || contract.contains("comprehensive") {
                contradictions.push(Contradiction {
                    title: "Speed vs. Thoroughness".to_string(),
                    description: "The vision emphasizes speed while the contract requires thoroughness.\n\
                                 These are often at odds. A thorough analysis may take more\n\
                                 time than a superficial one.".to_string(),
                    conflicting_items: vec![
                        if vision.contains("fast") { "vision: fast".to_string() } else { "vision: quick".to_string() },
                        "contract: thorough analysis".to_string(),
                    ],
                });
            }
        }

        // Contradiction: "cheap" vs "robust"
        if vision.contains("cheap") || vision.contains("low-cost") {
            if contract.contains("robust") || contract.contains("resilient") {
                contradictions.push(Contradiction {
                    title: "Cost vs. Robustness".to_string(),
                    description: "The vision emphasizes low cost while the contract requires robustness.\n\
                                 Robust systems often require redundancy, redundancy requires\n\
                                 cost, and cost-cutting can undermine robustness.".to_string(),
                    conflicting_items: vec![
                        if vision.contains("cheap") { "vision: cheap".to_string() } else { "vision: low-cost".to_string() },
                        "contract: robustness requirements".to_string(),
                    ],
                });
            }
        }

        contradictions
    }

    /// Identifies bounds and assumptions.
    fn identify_bounds(&self, vision: &str, contract: &str) -> Vec<String> {
        let mut bounds = Vec::new();

        // Check for implicit bounds
        if vision.contains("web") || vision.contains("browser") {
            bounds.push("The component is bound to web-based interaction (browser, REST, etc.).".to_string());
        }

        if vision.contains("mobile") || vision.contains("app") {
            bounds.push("The component targets mobile platforms (iOS, Android, etc.).".to_string());
        }

        if vision.contains("cloud") || vision.contains("serverless") {
            bounds.push("The component assumes cloud infrastructure availability.".to_string());
        }

        // Check for implicit assumptions
        if contract.contains("user") || contract.contains("user input") {
            bounds.push("The component assumes a human user exists and can provide input.".to_string());
        }

        if contract.contains("database") || contract.contains("storage") {
            bounds.push("The component assumes persistent storage is available.".to_string());
        }

        if contract.contains("network") || contract.contains("http") {
            bounds.push("The component assumes network connectivity is available.".to_string());
        }

        bounds
    }

    /// Identifies failure paths.
    fn identify_failure_paths(&self, vision: &str, contract: &str) -> Vec<FailurePath> {
        let mut failure_paths = Vec::new();

        // Check for availability failure paths
        if contract.contains("available") || contract.contains("uptime") {
            failure_paths.push(FailurePath {
                title: "Availability Failure".to_string(),
                description: "If the required infrastructure (databases, network, etc.) becomes\n\
                             unavailable, the component cannot fulfill its contract.\n\
                             Mitigation: add circuit breakers, fallbacks, and graceful\n\
                             degradation paths.".to_string(),
                mitigation: "Implement circuit breakers, fallbacks, and graceful degradation.".to_string(),
            });
        }

        // Check for data loss failure paths
        if contract.contains("persistent") || contract.contains("store") {
            failure_paths.push(FailurePath {
                title: "Data Loss Failure".to_string(),
                description: "If storage is lost or corrupted, the component cannot fulfill\n\
                             its contract. Mitigation: implement replication, backups,\n\
                             and checksums.".to_string(),
                mitigation: "Implement replication, backups, and data integrity checks.".to_string(),
            });
        }

        // Check for security failure paths
        if contract.contains("secure") || contract.contains("encrypt") {
            failure_paths.push(FailurePath {
                title: "Security Failure".to_string(),
                description: "If an authentication or authorization mechanism is bypassed,\n\
                             the component may expose sensitive data or functionality.\n\
                             Mitigation: defense in depth, least privilege, and defense\n\
                             in depth.".to_string(),
                mitigation: "Implement defense in depth, least privilege, and regular security audits.".to_string(),
            });
        }

        failure_paths
    }

    /// Generates feasibility findings.
    fn generate_findings(
        &self,
        contradictions: &[Contradiction],
        unresolved: &[UnresolvedDecision],
        failure_paths: &[FailurePath],
    ) -> Vec<FeasibilityFinding> {
        let mut findings = Vec::new();

        for contradiction in contradictions {
            findings.push(FeasibilityFinding {
                severity: Severity::Critical,
                category: "Contradiction".to_string(),
                title: contradiction.title.clone(),
                description: contradiction.description.clone(),
            });
        }

        for unresolved in unresolved {
            findings.push(FeasibilityFinding {
                severity: Severity::Medium,
                category: "Unresolved Decision".to_string(),
                title: unresolved.title.clone(),
                description: format!(
                    "Decision: {}\nOptions: {}",
                    unresolved.title,
                    unresolved.options.iter().map(|o| o.as_str()).collect::<Vec<_>>().join(", ")
                ),
            });
        }

        for failure_path in failure_paths {
            findings.push(FeasibilityFinding {
                severity: Severity::Low,
                category: "Failure Path".to_string(),
                title: failure_path.title.clone(),
                description: format!(
                    "Failure: {}\nMitigation: {}",
                    failure_path.description, failure_path.mitigation
                ),
            });
        }

        findings
    }

    /// Generates recommendations.
    fn generate_recommendations(
        &self,
        contradictions: &[Contradiction],
        unresolved: &[UnresolvedDecision],
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        for contradiction in contradictions {
            recommendations.push(format!(
                "RESOLVE: {} — {}",
                contradiction.title, contradiction.description
            ));
        }

        for unresolved in unresolved {
            recommendations.push(format!(
                "DECIDE: {} — {}",
                unresolved.title,
                unresolved.options.iter().map(|o| o.as_str()).collect::<Vec<_>>().join("; ")
            ));
        }

        recommendations
    }
}

use std::time::{SystemTime, UNIX_EPOCH};

/// A skill that designs components from a product vision.
pub struct ComponentDesignSkill {
    working_directory: PathBuf,
}

impl ComponentDesignSkill {
    pub fn new() -> Self {
        Self {
            working_directory: PathBuf::from(".kvist"),
        }
    }

    /// Runs a component design session.
    pub fn run(&self, vision: &str, contract: &str, constraints: &[String]) -> Result<String> {
        let report = self.design(vision, contract, constraints)?;

        // Write the feasibility report to a file
        let report_path = self.working_directory.join("FEASIBILITY.md");
        let report_content = toml::to_string_pretty(&report)
            .unwrap_or_else(|e| panic!("failed to serialize report: {e}"));

        std::fs::write(&report_path, &report_content)?;

        Ok(format!(
            "Feasibility report written to: {}\n{}",
            report_path.display(),
            report_content
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_unresolved_decisions() {
        let skill = ComponentDesignSkill::new();
        let decisions = skill.identify_unresolved_decisions(
            "We want a highly available web service that is simple to deploy",
            "The service must be highly available and use encryption for all data.",
        );

        assert!(decisions.iter().any(|d| d.title.contains("highly available")));
    }

    #[test]
    fn test_identify_contradictions() {
        let skill = ComponentDesignSkill::new();
        let contradictions = skill.identify_contradictions(
            "We want a simple, cheap solution",
            "The solution must be secure, robust, and enterprise-grade.",
        );

        assert!(contradictions.iter().any(|c| c.title.contains("Simplicity")));
    }

    #[test]
    fn test_generate_findings() {
        let skill = ComponentDesignSkill::new();
        let findings = skill.generate_findings(
            &[],
            &[UnresolvedDecision {
                title: "Missing scope boundary".to_string(),
                description: "no scope defined".to_string(),
                options: vec!["define scope".to_string()],
            }],
            &[],
        );

        assert_eq!(findings.len(), 1);
    }
}
