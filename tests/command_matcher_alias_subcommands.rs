// ABOUTME: A parent command's alias must reach every subcommand registered under the parent
// ABOUTME: Guards the greedy matcher against swallowing a subcommand as an argument to the alias
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `/coach` is aliased as `/coaches`; `/coach invite`, `/coach select` and
//! `/coach assign` are separate commands registered under the canonical
//! spelling only. Longest-match over the raw lookup keys would resolve
//! `/coaches invite` to `/coaches` — the list handler — and hand it `invite`
//! as an argument. These tests pin the alias-canonicalising resolution.

use dravr_canot::commands::{CommandDefinition, CommandMatcher, CommandRegistry, CommandRole};

fn definition(name: &str, command: &str, aliases: &[&str]) -> CommandDefinition {
    CommandDefinition {
        name: name.to_owned(),
        command: command.to_owned(),
        aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
        description: format!("{name} description"),
        domain: "coach".to_owned(),
        required_role: CommandRole::Any,
        requires_group: false,
    }
}

fn coach_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    assert!(registry.register(definition("coach", "/coach", &["/coaches"])));
    assert!(registry.register(definition("coach-invite", "/coach invite", &[])));
    assert!(registry.register(definition("coach-select", "/coach select", &[])));
    assert!(registry.register(definition("coach-assign", "/coach assign", &[])));
    assert!(registry.register(definition("group", "/group", &["/g"])));
    assert!(registry.register(definition("group-invite", "/group invite", &["/gi"])));
    registry
}

#[test]
fn alias_followed_by_subcommand_dispatches_to_the_subcommand() {
    let registry = coach_registry();
    let matcher = CommandMatcher::from_registry(&registry);

    for (text, expected_name, expected_args) in [
        ("/coaches invite", "coach-invite", vec![]),
        ("/coaches select abc-123", "coach-select", vec!["abc-123"]),
        (
            "/coaches assign coach-1 group-2",
            "coach-assign",
            vec!["coach-1", "group-2"],
        ),
    ] {
        let parsed = matcher
            .try_match(text, &registry)
            .unwrap_or_else(|| panic!("{text} must match"));
        assert_eq!(parsed.name, expected_name, "{text}");
        assert_eq!(parsed.args, expected_args, "{text}");
        assert_eq!(parsed.raw_text, text, "raw text keeps what was typed");
    }
}

#[test]
fn canonical_subcommands_still_match_directly() {
    let registry = coach_registry();
    let matcher = CommandMatcher::from_registry(&registry);

    let parsed = matcher.try_match("/coach invite", &registry).unwrap();
    assert_eq!(parsed.name, "coach-invite");
    assert!(parsed.args.is_empty());

    let parsed = matcher.try_match("/Coach Select X", &registry).unwrap();
    assert_eq!(parsed.name, "coach-select");
    assert_eq!(parsed.args, vec!["X"], "argument casing is preserved");
}

#[test]
fn bare_alias_and_alias_with_plain_arguments_still_reach_the_parent() {
    let registry = coach_registry();
    let matcher = CommandMatcher::from_registry(&registry);

    let parsed = matcher.try_match("/coaches", &registry).unwrap();
    assert_eq!(parsed.name, "coach");
    assert!(parsed.args.is_empty());

    // An argument that is not a subcommand stays an argument to the parent.
    let parsed = matcher.try_match("/coaches running", &registry).unwrap();
    assert_eq!(parsed.name, "coach");
    assert_eq!(parsed.args, vec!["running"]);

    // A word-boundary miss is not a match: `/coachesx` is nothing.
    assert!(matcher.try_match("/coachesx invite", &registry).is_none());
}

#[test]
fn a_subcommand_alias_keeps_its_own_arguments() {
    let registry = coach_registry();
    let matcher = CommandMatcher::from_registry(&registry);

    let parsed = matcher.try_match("/gi coach", &registry).unwrap();
    assert_eq!(parsed.name, "group-invite");
    assert_eq!(parsed.args, vec!["coach"]);

    let parsed = matcher.try_match("/g invite", &registry).unwrap();
    assert_eq!(
        parsed.name, "group-invite",
        "parent alias reaches the subcommand"
    );
    assert!(parsed.args.is_empty());
}
