//! Shared helpers for the per-object discovery/description metadata that the
//! `vgi-lint` strict profile expects on every function:
//! `vgi.title`, `vgi.doc_llm`, `vgi.doc_md`, and `vgi.keywords`.

use vgi::FunctionExample;

/// Minimal JSON string escaping (quotes + backslashes) for tag payloads.
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build the described `vgi.example_queries` JSON tag from a list of
/// `(description, sql)` pairs (VGI515: every example must carry a non-empty
/// description).
pub fn example_queries_json(examples: &[(&str, &str)]) -> String {
    let items: Vec<String> = examples
        .iter()
        .map(|(description, sql)| {
            format!(
                "{{\"description\":\"{}\",\"sql\":\"{}\"}}",
                json_escape(description),
                json_escape(sql)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Build the native `FunctionMetadata.examples` vector from the SAME
/// `(description, sql)` pairs used for [`example_queries_json`]. The native
/// `duckdb_functions().examples` carrier drops descriptions, so keeping the two
/// byte-identical lets the linter dedupe them against the described tag entries
/// (VGI515) rather than seeing a description-less native example.
pub fn function_examples(examples: &[(&str, &str)]) -> Vec<FunctionExample> {
    examples
        .iter()
        .map(|(description, sql)| FunctionExample {
            sql: (*sql).into(),
            description: (*description).into(),
            expected_output: None,
        })
        .collect()
}

/// Encode comma-separated keywords as the JSON array of strings `vgi.keywords`
/// requires (VGI138).
pub fn keywords_json(keywords: &str) -> String {
    let items: Vec<String> = keywords
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| {
            let escaped = k.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Build the `vgi.agent_test_tasks` JSON value: a fixed suite of analyst tasks
/// that `vgi-lint simulate` (the `--ai` agent-check) runs. Each `(name, prompt)`
/// pair becomes a task object; only the `prompt` is published to the simulated
/// analyst. The private graders (`reference_sql`, keyed by task name) live in
/// `vgi-agent-tests.yaml` at the repo root so the agent never sees them (VGI416).
pub fn agent_test_tasks_json(tasks: &[(&str, &str)]) -> String {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }
    let items: Vec<String> = tasks
        .iter()
        .map(|(name, prompt)| {
            format!(
                "{{\"name\":\"{}\",\"prompt\":\"{}\"}}",
                esc(name),
                esc(prompt)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Build the four standard per-object discovery tags.
pub fn object_tags(
    title: &str,
    description_llm: &str,
    description_md: &str,
    keywords: &str,
) -> Vec<(String, String)> {
    vec![
        ("vgi.title".to_string(), title.to_string()),
        ("vgi.doc_llm".to_string(), description_llm.to_string()),
        ("vgi.doc_md".to_string(), description_md.to_string()),
        ("vgi.keywords".to_string(), keywords_json(keywords)),
    ]
}
