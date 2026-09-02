// ABOUTME: bot_command_list must publish single-token aliases, not just command names
// ABOUTME: Without them a vocabulary whose group half is multi-word offers a group no entries

// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! Telegram accepts one token per menu entry — `BotCommand.command` is
//! "1-32 characters. Can contain only lowercase English letters, digits and
//! underscores" — so `/group status` can never be a menu entry. Every
//! group-administration command in the catalogue is multi-word, so before
//! aliases were published the `/` menu in a group listed exactly zero group
//! commands while still looking populated with personal ones.
//!
//! These tests assert the published set by value. A presence-only assertion
//! cannot catch the regression that matters here: silently losing entries
//! still leaves a non-empty, plausible-looking menu.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dravr_canot::commands::{CommandDefinition, CommandRegistry, CommandRole};

fn definition(name: &str, command: &str, aliases: &[&str]) -> CommandDefinition {
    CommandDefinition {
        name: name.to_owned(),
        command: command.to_owned(),
        aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
        description: format!("{name} description"),
        domain: "group".to_owned(),
        required_role: CommandRole::Any,
        requires_group: false,
    }
}

/// Mirrors the shape of the real catalogue: a single-word personal command,
/// a multi-word group command reachable only by its alias, and a parent that
/// carries both its own name and an alias.
fn registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    assert!(registry.register(definition("help", "/help", &[])));
    assert!(registry.register(definition("group", "/group", &["/groups"])));
    assert!(registry.register(definition("group-status", "/group status", &["/gs"])));
    assert!(registry.register(definition("group-members", "/group members", &["/gm"])));
    assert!(registry.register(definition("group-invite", "/group invite", &["/gi"])));
    registry
}

fn published(registry: &CommandRegistry) -> Vec<String> {
    registry
        .bot_command_list()
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn published_set_is_exact_and_sorted() {
    // By value, not by presence: adding or losing an entry must fail loudly.
    assert_eq!(
        published(&registry()),
        vec!["gi", "gm", "group", "gs", "help"]
    );
}

#[test]
fn a_redundant_alias_does_not_pad_the_menu() {
    // /groups aliases /group, which Telegram already accepts, so the alias is
    // a second row carrying an identical description. Only an alias standing
    // in for an unpublishable multi-word command earns a row.
    let names = published(&registry());
    assert!(
        !names.contains(&"groups".to_owned()),
        "/groups duplicates /group and must not pad the menu: {names:?}"
    );
    assert!(
        names.contains(&"gs".to_owned()),
        "/gs is the only shape /group status can take and must stay: {names:?}"
    );
}

#[test]
fn a_described_list_keeps_every_command_and_only_changes_the_text() {
    // Nothing is withheld: a scope marks commands, it does not hide them, so
    // an athlete in a shared room can still discover the whole vocabulary.
    let registry = registry();
    let marked = registry.bot_command_list_described(|d| {
        if d.name == "group-status" {
            format!("PERSONAL {}", d.description)
        } else {
            d.description.clone()
        }
    });

    let names: Vec<String> = marked.iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(
        names,
        vec!["gi", "gm", "group", "gs", "help"],
        "a described list must publish the same set as the plain one"
    );
}

#[test]
fn an_alias_carries_its_parents_marker() {
    // The trap: marking the command but not its aliases would leave `/gs`
    // reading as shared while `/group status` reads as personal — the same
    // command described two different ways in one menu.
    let registry = registry();
    let marked = registry.bot_command_list_described(|d| {
        if d.name == "group-status" {
            format!("PERSONAL {}", d.description)
        } else {
            d.description.clone()
        }
    });

    let gs = marked.iter().find(|(n, _)| n == "gs").map_or_else(
        || panic!("alias /gs absent: {marked:?}"),
        |(_, d)| d.clone(),
    );
    assert!(
        gs.starts_with("PERSONAL "),
        "an alias must inherit its parent's marker, got {gs:?}"
    );
}

#[test]
fn the_plain_list_is_the_described_one_using_the_catalogue_text() {
    // Two builders would drift; this pins them as one.
    assert_eq!(
        registry().bot_command_list(),
        registry().bot_command_list_described(|d| d.description.clone())
    );
}

#[test]
fn multi_word_commands_reach_the_menu_through_their_aliases() {
    let names = published(&registry());
    for alias in ["gs", "gm", "gi"] {
        assert!(
            names.contains(&alias.to_owned()),
            "group command alias /{alias} is missing from the published menu: {names:?}"
        );
    }
}

#[test]
fn multi_word_triggers_are_never_published_verbatim() {
    // Telegram rejects the ENTIRE setMyCommands call on one malformed entry,
    // so a spaced trigger would cost the whole menu, not just its own row.
    for (name, _) in registry().bot_command_list() {
        assert!(
            !name.contains(' '),
            "published a multi-token entry Telegram would reject: {name:?}"
        );
    }
}

#[test]
fn every_published_entry_satisfies_telegrams_grammar() {
    for (name, description) in registry().bot_command_list() {
        assert!(
            !name.is_empty() && name.len() <= 32,
            "entry {name:?} violates the 1-32 character rule"
        );
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "entry {name:?} contains a character Telegram rejects"
        );
        assert!(
            !description.trim().is_empty(),
            "entry {name:?} has no description"
        );
    }
}

#[test]
fn an_alias_inherits_its_parents_description() {
    let list = registry().bot_command_list();
    let alias_description = list.iter().find(|(name, _)| name == "gs").map_or_else(
        || panic!("alias /gs absent: {list:?}"),
        |(_, description)| description.clone(),
    );
    assert_eq!(alias_description, "group-status description");
}

#[test]
fn a_command_name_beats_a_colliding_alias() {
    // If another command aliases itself to an existing command's name, the
    // real command must keep the menu row — otherwise the description shown
    // belongs to a different command than the one the tap will run.
    let mut registry = registry();
    assert!(registry.register(definition("decoy", "/decoy", &["/help"])));

    let list = registry.bot_command_list();
    let help_description = list.iter().find(|(name, _)| name == "help").map_or_else(
        || panic!("/help absent: {list:?}"),
        |(_, description)| description.clone(),
    );
    assert_eq!(help_description, "help description");

    // ...and the collision must not leave two rows named `help`.
    assert_eq!(
        list.iter().filter(|(name, _)| name == "help").count(),
        1,
        "duplicate menu rows for /help: {list:?}"
    );
}
