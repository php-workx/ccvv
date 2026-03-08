//! Fixture-based tests using real clipboard captures.
//!
//! Each fixture is a raw/cleaned pair from actual clipboard usage.
//! These tests document **current pipeline behavior** — when a fixture
//! test fails after a code change, review the new output and update
//! the cleaned_*.txt file if the new behavior is an improvement.
//!
//! To add a new fixture:
//! 1. Save raw clipboard text to `tests/fixtures/raw_NN_description.txt`
//! 2. Run the pipeline and save output to `tests/fixtures/cleaned_NN_description.txt`
//! 3. Add a test entry to the `fixtures!` macro below

use ccvv_lib::pipeline::Pipeline;
use ccvv_lib::transforms::agent::AgentTransform;
use ccvv_lib::transforms::autowrap::AutowrapTransform;
use ccvv_lib::transforms::normalize::NormalizeTransform;
use ccvv_lib::transforms::structural::StructuralTransform;
use ccvv_lib::transforms::url::UrlTransform;
use ccvv_lib::transforms::whitespace::WhitespaceTransform;
use ccvv_lib::transforms::Transform;

/// Build the full default pipeline matching the app's stage order.
fn full_pipeline() -> Pipeline {
    let stages: Vec<Box<dyn Transform>> = vec![
        Box::new(NormalizeTransform::new()),
        Box::new(WhitespaceTransform::new()),
        Box::new(AgentTransform::new()),
        Box::new(StructuralTransform::new()),
        Box::new(UrlTransform::new()),
        Box::new(AutowrapTransform::new()),
    ];
    Pipeline::new(stages)
        .with_max_input_bytes(1_048_576)
        .with_sensitive_filter(true)
}

macro_rules! fixture_test {
    ($name:ident, $raw_file:expr, $cleaned_file:expr) => {
        #[test]
        fn $name() {
            let raw = include_str!(concat!("fixtures/", $raw_file));
            let expected = include_str!(concat!("fixtures/", $cleaned_file));
            let pipeline = full_pipeline();
            let (actual, _ctx) = pipeline.run(raw);
            // Trim trailing newline from expected — fixture files may have one from export
            let expected = expected.trim_end_matches('\n');
            assert_eq!(
                actual, expected,
                "Fixture {} produced unexpected output.\n\
                 --- EXPECTED (cleaned file) ---\n{}\n\
                 --- ACTUAL (pipeline output) ---\n{}",
                $raw_file, expected, actual
            );
        }
    };
}

fixture_test!(
    fixture_01_prose_with_technical_tokens,
    "raw_01_prose_with_technical_tokens.txt",
    "cleaned_01_prose_with_technical_tokens.txt"
);

fixture_test!(
    fixture_02_mixed_prose_numbered_list,
    "raw_02_mixed_prose_numbered_list.txt",
    "cleaned_02_mixed_prose_numbered_list.txt"
);

fixture_test!(
    fixture_03_bullet_list_with_indent,
    "raw_03_bullet_list_with_indent.txt",
    "cleaned_03_bullet_list_with_indent.txt"
);

fixture_test!(
    fixture_04_single_line_em_dash,
    "raw_04_single_line_em_dash.txt",
    "cleaned_04_single_line_em_dash.txt"
);

// Table fixtures (#05, #06, #08, #09) are excluded — the app routes table
// content through the interactive TableCellPicker, not the pipeline.

fixture_test!(
    fixture_10_multi_paragraph_prompt,
    "raw_10_multi_paragraph_prompt.txt",
    "cleaned_10_multi_paragraph_prompt.txt"
);

fixture_test!(
    fixture_11_git_commit_message,
    "raw_11_git_commit_message.txt",
    "cleaned_11_git_commit_message.txt"
);

fixture_test!(
    fixture_12_rust_explanation,
    "raw_12_rust_explanation.txt",
    "cleaned_12_rust_explanation.txt"
);

fixture_test!(
    fixture_13_aws_accounts_list,
    "raw_13_aws_accounts_list.txt",
    "cleaned_13_aws_accounts_list.txt"
);

fixture_test!(
    fixture_14_security_finding,
    "raw_14_security_finding.txt",
    "cleaned_14_security_finding.txt"
);

fixture_test!(
    fixture_17_list_with_continuations,
    "raw_17_list_with_continuations.txt",
    "cleaned_17_list_with_continuations.txt"
);

fixture_test!(
    fixture_18_instructions_with_shell_commands,
    "raw_18_instructions_with_shell_commands.txt",
    "cleaned_18_instructions_with_shell_commands.txt"
);

fixture_test!(
    fixture_19_multiline_shell_command,
    "raw_19_multiline_shell_command.txt",
    "cleaned_19_multiline_shell_command.txt"
);

fixture_test!(
    fixture_20_pure_shell_block,
    "raw_20_pure_shell_block.txt",
    "cleaned_20_pure_shell_block.txt"
);

fixture_test!(
    fixture_21_pure_list,
    "raw_21_pure_list.txt",
    "cleaned_21_pure_list.txt"
);

// ===== Idempotency: every fixture must be stable under double-application =====

macro_rules! fixture_idempotency_test {
    ($name:ident, $raw_file:expr) => {
        #[test]
        fn $name() {
            let raw = include_str!(concat!("fixtures/", $raw_file));
            let pipeline = full_pipeline();
            let (first, _) = pipeline.run(raw);
            let (second, _) = pipeline.run(&first);
            assert_eq!(
                first, second,
                "Fixture {} is not idempotent.\n\
                 --- FIRST PASS ---\n{}\n\
                 --- SECOND PASS ---\n{}",
                $raw_file, first, second
            );
        }
    };
}

fixture_idempotency_test!(
    idempotent_01_prose_with_technical_tokens,
    "raw_01_prose_with_technical_tokens.txt"
);
fixture_idempotency_test!(
    idempotent_02_mixed_prose_numbered_list,
    "raw_02_mixed_prose_numbered_list.txt"
);
fixture_idempotency_test!(
    idempotent_03_bullet_list_with_indent,
    "raw_03_bullet_list_with_indent.txt"
);
fixture_idempotency_test!(
    idempotent_04_single_line_em_dash,
    "raw_04_single_line_em_dash.txt"
);
fixture_idempotency_test!(
    idempotent_10_multi_paragraph_prompt,
    "raw_10_multi_paragraph_prompt.txt"
);
fixture_idempotency_test!(
    idempotent_11_git_commit_message,
    "raw_11_git_commit_message.txt"
);
fixture_idempotency_test!(
    idempotent_12_rust_explanation,
    "raw_12_rust_explanation.txt"
);
fixture_idempotency_test!(
    idempotent_13_aws_accounts_list,
    "raw_13_aws_accounts_list.txt"
);
fixture_idempotency_test!(
    idempotent_14_security_finding,
    "raw_14_security_finding.txt"
);
fixture_idempotency_test!(
    idempotent_17_list_with_continuations,
    "raw_17_list_with_continuations.txt"
);
fixture_idempotency_test!(
    idempotent_18_instructions_with_shell_commands,
    "raw_18_instructions_with_shell_commands.txt"
);
fixture_idempotency_test!(
    idempotent_19_multiline_shell_command,
    "raw_19_multiline_shell_command.txt"
);

fixture_idempotency_test!(
    idempotent_20_pure_shell_block,
    "raw_20_pure_shell_block.txt"
);

fixture_idempotency_test!(
    idempotent_21_pure_list,
    "raw_21_pure_list.txt"
);

fixture_test!(
    fixture_22_heading_with_wrapped_prose,
    "raw_22_heading_with_wrapped_prose.txt",
    "cleaned_22_heading_with_wrapped_prose.txt"
);

// ===== Idempotency for fixture 22 =====

fixture_idempotency_test!(
    idempotent_22_heading_with_wrapped_prose,
    "raw_22_heading_with_wrapped_prose.txt"
);
