use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::Span;
use vue_sfc_parser::ast::{Attribute, Element, Node};

use crate::{
    rule::Rule,
    utils::{directive_key_span, is_custom_component, walk_elements},
    vue_template::{VueTemplateContext, VueTemplateRule},
};

/// eslint-plugin-vue `use-v-on-exact`'s `SYSTEM_MODIFIERS`.
const SYSTEM_MODIFIERS: &[&str] = &["ctrl", "shift", "alt", "meta"];

/// eslint-plugin-vue `use-v-on-exact`'s `GLOBAL_MODIFIERS`.
const GLOBAL_MODIFIERS: &[&str] =
    &["stop", "prevent", "capture", "self", "once", "passive", "native"];

fn consider_exact_diagnostic(span: Span) -> OxcDiagnostic {
    OxcDiagnostic::warn("Consider to use '.exact' modifier.")
        .with_help(
            "Add `.exact` so this listener does not also fire for the same event with extra system modifiers held down.",
        )
        .with_label(span)
}

#[derive(Debug, Default, Clone)]
pub struct UseVOnExact;

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Requires the `.exact` modifier when multiple `v-on` listeners for the
    /// same event on the same element differ only by system modifiers
    /// (`ctrl`/`shift`/`alt`/`meta`), in Vue `<template>` blocks.
    ///
    /// ### Why is this bad?
    ///
    /// Vue's modifier system is additive, not exclusive: `@click.ctrl`
    /// fires *in addition to* a plain `@click` on the same element, not
    /// instead of it. Without `.exact` on the less-specific listener, both
    /// handlers run for a ctrl+click, which is rarely the intent.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule:
    /// ```vue
    /// <template>
    ///   <button @click="onClick" @click.ctrl="onCtrlClick" />
    /// </template>
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```vue
    /// <template>
    ///   <button @click.exact="onClick" @click.ctrl="onCtrlClick" />
    /// </template>
    /// ```
    UseVOnExact,
    vue,
    correctness,
    version = "1.77.0",
    short_description = "Enforce usage of `exact` modifier on `v-on`.",
);

impl Rule for UseVOnExact {}

impl VueTemplateRule for UseVOnExact {
    fn run_on_template<'a>(&self, nodes: &[Node<'a>], ctx: &mut VueTemplateContext<'a>) {
        walk_elements(nodes, &mut |element| check_element(element, ctx));
    }
}

/// One `v-on` listener on an element: `name` is its (possibly dynamic, possibly
/// empty for a spread `v-on="obj"`) argument text; mirrors eslint-plugin-vue's
/// `EventDirective`.
struct EventDirective<'e, 'a> {
    name: &'a str,
    attribute: &'e Attribute<'a>,
    modifiers: &'e [&'a str],
}

/// eslint-plugin-vue `use-v-on-exact`'s `VStartTag` handler.
fn check_element<'a>(element: &Element<'a>, ctx: &mut VueTemplateContext<'a>) {
    if element.attributes.is_empty() {
        return;
    }

    let mut events: Vec<EventDirective<'_, 'a>> = element
        .attributes
        .iter()
        .filter_map(|attribute| {
            let directive = attribute.directive.as_ref()?;
            if directive.name != "on" {
                return None;
            }
            Some(EventDirective {
                name: directive.argument.as_ref().map_or("", |argument| argument.text),
                attribute,
                modifiers: directive.modifiers.as_slice(),
            })
        })
        .collect();

    // On a custom component, `v-on` listens for custom (component-emitted)
    // events, which don't carry native system modifiers; only a `.native`
    // listener (Vue 2's escape hatch to a real DOM event) is relevant here.
    if is_custom_component(element) {
        events.retain(|event| event.modifiers.contains(&"native"));
    }
    if events.is_empty() {
        return;
    }

    // Group by event name, preserving first-seen order (matches iterating
    // `Object.keys` of an object built by insertion).
    let mut names: Vec<&str> = Vec::new();
    for event in &events {
        if !names.contains(&event.name) {
            names.push(event.name);
        }
    }

    for name in names {
        let group: Vec<usize> = events
            .iter()
            .enumerate()
            .filter(|(_, event)| event.name == name)
            .map(|(i, _)| i)
            .collect();
        if !group.iter().any(|&i| has_system_modifier(events[i].modifiers)) {
            continue;
        }

        for conflicted in find_conflicted_events(&events, &group) {
            ctx.diagnostic(consider_exact_diagnostic(directive_key_span(
                events[conflicted].attribute,
            )));
        }
    }
}

fn has_system_modifier(modifiers: &[&str]) -> bool {
    modifiers.iter().any(|modifier| SYSTEM_MODIFIERS.contains(modifier))
}

fn is_key_modifier(modifier: &str) -> bool {
    !GLOBAL_MODIFIERS.contains(&modifier) && !SYSTEM_MODIFIERS.contains(&modifier)
}

/// eslint-plugin-vue's `getSystemModifiersString`/`getKeyModifiersString`:
/// the matching modifiers, alphabetically sorted and comma-joined.
fn modifiers_string(modifiers: &[&str], keep: impl Fn(&str) -> bool) -> String {
    let mut kept: Vec<&str> = modifiers.iter().copied().filter(|modifier| keep(modifier)).collect();
    kept.sort_unstable();
    kept.join(",")
}

/// eslint-plugin-vue's `hasConflictedModifiers`: whether `event` (the
/// candidate) is dominated by `base` — `base` has at least one modifier, and
/// `base`'s system-modifier set is a (textual, comma-joined) superset of
/// `event`'s, so any event `base` fires for, `event` would fire for too.
fn has_conflicted_modifiers(
    base: &EventDirective,
    event: &EventDirective,
    same_node: bool,
) -> bool {
    if same_node || event.modifiers.contains(&"exact") {
        return false;
    }

    let event_key_modifiers = modifiers_string(event.modifiers, is_key_modifier);
    let base_key_modifiers = modifiers_string(base.modifiers, is_key_modifier);
    if !event_key_modifiers.is_empty()
        && !base_key_modifiers.is_empty()
        && event_key_modifiers != base_key_modifiers
    {
        return false;
    }

    let event_system_modifiers =
        modifiers_string(event.modifiers, |m| SYSTEM_MODIFIERS.contains(&m));
    let base_system_modifiers = modifiers_string(base.modifiers, |m| SYSTEM_MODIFIERS.contains(&m));
    !base.modifiers.is_empty()
        && base_system_modifiers != event_system_modifiers
        && base_system_modifiers.contains(&event_system_modifiers)
}

/// eslint-plugin-vue's `findConflictedEvents`: every event in `group`
/// (indices into `events`) that some other event in `group` dominates, in
/// insertion order and without duplicates.
fn find_conflicted_events(events: &[EventDirective], group: &[usize]) -> Vec<usize> {
    let mut conflicted: Vec<usize> = Vec::new();
    for &base_index in group {
        let newly: Vec<usize> = group
            .iter()
            .copied()
            .filter(|candidate_index| !conflicted.contains(candidate_index))
            .filter(|&candidate_index| {
                has_conflicted_modifiers(
                    &events[base_index],
                    &events[candidate_index],
                    base_index == candidate_index,
                )
            })
            .collect();
        conflicted.extend(newly);
    }
    conflicted
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::UseVOnExact;
    use crate::{rule::RuleMeta, tester::Tester};

    #[test]
    fn test() {
        let pass = vec![
            (
                r#"<template><button @click="a" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `.exact` on the plain listener excludes it from conflict.
            (
                r#"<template><button @click.exact="a" @click.ctrl="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Disjoint system modifiers: neither is a superset of the other.
            (
                r#"<template><button @click.ctrl="a" @click.shift="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component without `.native`: not checked.
            (
                r#"<template><MyComp @click="a" @click.ctrl="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Different key modifiers: the early key-modifier-mismatch bail.
            (
                r#"<template><button @keyup.enter="a" @keyup.tab.ctrl="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            (r"<template><div /></template>", None, None, Some(PathBuf::from("test.vue"))),
        ];

        let fail = vec![
            // Plain click also catches ctrl+click: report on the plain one.
            (
                r#"<template><button @click="a" @click.ctrl="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // `.ctrl` is a subset of `.ctrl.shift`: report on the subset.
            (
                r#"<template><button @click.ctrl="a" @click.ctrl.shift="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Same key modifier, one has an extra system modifier.
            (
                r#"<template><button @keyup.enter="a" @keyup.enter.ctrl="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
            // Custom component *with* `.native`: checked like a native element.
            (
                r#"<template><MyComp @click.native="a" @click.native.ctrl="b" /></template>"#,
                None,
                None,
                Some(PathBuf::from("test.vue")),
            ),
        ];

        Tester::new(UseVOnExact::NAME, UseVOnExact::PLUGIN, pass, fail).test_and_snapshot();
    }
}
