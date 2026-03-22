// ABOUTME: Response formatter applying variable substitution to command templates
// ABOUTME: Simple {variable} replacement without template engine dependency
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;
use std::hash::BuildHasher;

/// Format a command response template by substituting `{variable}` placeholders.
///
/// Unresolved placeholders are left as-is (e.g., `{unknown}` stays `{unknown}`).
///
/// # Example
///
/// ```
/// use dravr_canot::commands::formatter::format_response;
/// use std::collections::HashMap;
///
/// let template = "Hello {name}! You have {count} activities.";
/// let mut bindings = HashMap::new();
/// bindings.insert("name".to_owned(), "Alice".to_owned());
/// bindings.insert("count".to_owned(), "42".to_owned());
///
/// let result = format_response(template, &bindings);
/// assert_eq!(result, "Hello Alice! You have 42 activities.");
/// ```
#[must_use]
pub fn format_response<S: BuildHasher>(
    template: &str,
    bindings: &HashMap<String, String, S>,
) -> String {
    let mut result = template.to_owned();
    for (key, value) in bindings {
        result = result.replace(&format!("{{{key}}}"), value);
    }
    result
}
