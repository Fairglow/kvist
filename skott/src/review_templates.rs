//! Generated templates for advisory review evidence.
//!
//! These templates are versioned and produce deterministic, VCS-inspectable
//! evidence files. They contain no user-specific values, credentials, or
//! license terms.

/// Current template version for review evidence.
pub const TEMPLATE_VERSION: u32 = 1;

/// Template for a review receipt.
///
/// Placeholders:
/// - `{id}`: Stable review identifier
/// - `{timestamp}`: ISO 8601 timestamp
/// - `{targets}`: JSON array of target objects
/// - `{scope}`: Review scope description
/// - `{reviewer}`: Reviewer identity (optional)
/// - `{author}`: Authoring context identity (optional)
/// - `{tool}`: Tool identity (optional)
/// - `{kvist_version}`: Kvist version
/// - `{report_path}`: Path to report (optional)
/// - `{report_digest}`: SHA-256 digest of report (optional)
pub const REVIEW_RECEIPT_TEMPLATE: &str = r#"{"schema_version":1,"id":"{id}","timestamp":"{timestamp}","targets":[{targets}],"scope":"{scope}","reviewer":{reviewer},"author":{author},"tool":{tool},"kvist_version":"{kvist_version}","report_path":{report_path},"report_digest":{report_digest},"acknowledged":null,"exception":null}"#;

/// Template for a review report.
///
/// Placeholders:
/// - `{receipt_id}`: Reference to the receipt
/// - `{timestamp}`: ISO 8601 timestamp
/// - `{scope}`: Review scope description
/// - `{findings}`: Bounded review findings (redacted)
/// - `{requires_acknowledgement}`: Boolean
pub const REVIEW_REPORT_TEMPLATE: &str = r#"{"schema_version":1,"receipt_id":"{receipt_id}","timestamp":"{timestamp}","scope":"{scope}","findings":"{findings}","requires_acknowledgement":{requires_acknowledgement}}"#;

/// Template for a review acknowledgement.
///
/// Placeholders:
/// - `{receipt_id}`: Reference to the receipt
/// - `{timestamp}`: ISO 8601 timestamp
/// - `{actor}`: Actor who acknowledged
/// - `{notes}`: Optional notes
pub const REVIEW_ACKNOWLEDGEMENT_TEMPLATE: &str = r#"{"schema_version":1,"receipt_id":"{receipt_id}","timestamp":"{timestamp}","actor":"{actor}","notes":{notes}}"#;

/// Template for a review exception.
///
/// Placeholders:
/// - `{timestamp}`: ISO 8601 timestamp
/// - `{targets}`: JSON array of target objects
/// - `{actor}`: Actor who raised the exception
/// - `{reason}`: Reason for the exception
pub const REVIEW_EXCEPTION_TEMPLATE: &str = r#"{"schema_version":1,"timestamp":"{timestamp}","targets":[{targets}],"actor":"{actor}","reason":"{reason}"}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_template_has_required_fields() {
        assert!(REVIEW_RECEIPT_TEMPLATE.contains("\"schema_version\":1"));
        assert!(REVIEW_RECEIPT_TEMPLATE.contains("\"id\":\"{id}\""));
        assert!(REVIEW_RECEIPT_TEMPLATE.contains("\"timestamp\":\"{timestamp}\""));
        assert!(REVIEW_RECEIPT_TEMPLATE.contains("\"targets\":["));
        assert!(REVIEW_RECEIPT_TEMPLATE.contains("\"scope\":\"{scope}\""));
        assert!(REVIEW_RECEIPT_TEMPLATE.contains("\"kvist_version\":\"{kvist_version}\""));
    }

    #[test]
    fn report_template_has_required_fields() {
        assert!(REVIEW_REPORT_TEMPLATE.contains("\"schema_version\":1"));
        assert!(REVIEW_REPORT_TEMPLATE.contains("\"receipt_id\":\"{receipt_id}\""));
        assert!(REVIEW_REPORT_TEMPLATE.contains("\"findings\":\"{findings}\""));
    }

    #[test]
    fn acknowledgement_template_has_required_fields() {
        assert!(REVIEW_ACKNOWLEDGEMENT_TEMPLATE.contains("\"schema_version\":1"));
        assert!(REVIEW_ACKNOWLEDGEMENT_TEMPLATE.contains("\"receipt_id\":\"{receipt_id}\""));
        assert!(REVIEW_ACKNOWLEDGEMENT_TEMPLATE.contains("\"actor\":\"{actor}\""));
    }

    #[test]
    fn exception_template_has_required_fields() {
        assert!(REVIEW_EXCEPTION_TEMPLATE.contains("\"schema_version\":1"));
        assert!(REVIEW_EXCEPTION_TEMPLATE.contains("\"actor\":\"{actor}\""));
        assert!(REVIEW_EXCEPTION_TEMPLATE.contains("\"reason\":\"{reason}\""));
    }
}
