use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse_manifest(path: &PathBuf) -> Vec<(String, String, String)> {
    let content = fs::read_to_string(path).expect("manifest should be readable");
    let mut entries = Vec::new();
    let mut name = None;
    let mut color = None;
    let mut description = None;
    for line in content.lines() {
        let line = line.trim_end();
        if let Some(value) = line.strip_prefix("- name: \"") {
            if let Some((n, c, d)) = name
                .take()
                .zip(color.take())
                .zip(description.take())
                .map(|((n, c), d)| (n, c, d))
            {
                entries.push((n, c, d));
            }
            name = Some(value.trim_end_matches('"').to_string());
            color = None;
            description = None;
        } else if let Some(value) = line.strip_prefix("  color: \"") {
            color = Some(value.trim_end_matches('"').to_string());
        } else if let Some(value) = line.strip_prefix("  description: \"") {
            description = Some(value.trim_end_matches('"').to_string());
        }
    }
    if let Some((n, c, d)) = name
        .take()
        .zip(color.take())
        .zip(description.take())
        .map(|((n, c), d)| (n, c, d))
    {
        entries.push((n, c, d));
    }
    entries
}

#[test]
fn factory_execution_label_manifest_matches_catalog() {
    let root = repo_root();
    let manifest_path = root.join(".github/labels.factory-execution.yml");
    let doc_path = root.join("docs/factory-execution-label-standard.md");
    let manifest = fs::read_to_string(&manifest_path).expect("manifest should exist");
    let doc = fs::read_to_string(&doc_path).expect("doc should exist");

    let entries = parse_manifest(&manifest_path);
    let expected = vec![
        (
            "ai/queued",
            "CFD3D7",
            "Accepted work awaiting execution, including scheduled retries.",
        ),
        ("ai/processing", "1D76DB", "Active execution."),
        (
            "ai/waiting",
            "FBCA04",
            "Waiting for an external condition without human intervention.",
        ),
        (
            "ai/blocked",
            "B60205",
            "An unmet prerequisite prevents progress.",
        ),
        (
            "ai/needs-input",
            "D93F0B",
            "Human information or a decision is required.",
        ),
        (
            "ai/failed",
            "B60205",
            "Unsuccessful terminal outcome with no automatic retry pending.",
        ),
        ("ai/cancelled", "6E7781", "Confirmed cancellation."),
        (
            "ai/completed",
            "0E8A16",
            "Successful automated work, independent of merge or issue closure.",
        ),
    ];
    assert_eq!(
        entries.len(),
        expected.len(),
        "catalog must contain exactly eight labels"
    );
    for (entry, exp) in entries.iter().zip(expected.iter()) {
        assert_eq!(entry.0, exp.0);
        assert_eq!(entry.1, exp.1);
        assert_eq!(entry.2, exp.2);
        assert_eq!(entry.1.len(), 6);
        assert!(entry.1.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(entry.2.len() <= 100);
        assert!(
            manifest.contains(&format!("color: \"{}\"", entry.1)),
            "colors must be quoted in manifest"
        );
    }

    let names: Vec<_> = entries.iter().map(|e| e.0.as_str()).collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(names.len(), unique.len(), "label names must be unique");

    for (name, color, desc) in &expected {
        assert!(doc.contains(name), "doc missing label {name}");
        assert!(doc.contains(color), "doc missing color {color}");
        assert!(doc.contains(desc), "doc missing description for {name}");
    }

    let normalized_doc = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized_doc_lower = normalized_doc.to_lowercase();
    for required in [
        "observational projections, not commands and not authoritative execution state",
        "manual label changes do not authorize execution transitions",
        "at most one catalog state applies to an issue",
        "no published automation state",
        "initial contract applies to github issues, not linked pull requests",
        "unrelated labels under the same prefix namespace are not reserved and are not managed",
        "case-insensitive collisions",
        "level-1 extensions",
        "can also be used independently by external repositories",
        "versioning",
        "non-destructive",
        "manifest and this document must agree",
        "version: `1.1.0`",
        "labels.prefix",
        "omitting `labels.prefix` resolves the existing `ai/*` catalog",
        "single lowercase ascii namespace followed by `/`",
        "reserved families of applicable repository standards cannot be selected",
        "state identifiers",
        "independent of label names",
        "ownership is limited to the eight resolved names",
        "inconsistent effective configuration between components is detectable before label mutation",
        "invalid configuration is rejected without replacing the last valid active configuration",
        "explicit migration is required",
    ] {
        assert!(
            normalized_doc_lower.contains(required),
            "doc missing required text: {required}"
        );
    }
    assert!(
        manifest.contains("Version: 1.1.0"),
        "manifest must declare version"
    );
}
