//! §λ-L-E Fase 13.g — LSP-facing analysis primitives for typed channels.
//!
//! Pure functions over the AST that surface the information `axon-lsp`
//! needs to implement editor features (autocomplete, hover, go-to-def,
//! find-references) over the Fase 13 typed-channel surface.
//!
//! The LSP itself lives in the sibling `axon-lsp` repo; this module is
//! its data-extraction layer.  Keeping the analysis here (in
//! `axon-frontend`) means the LSP picks it up by adding a path
//! dependency — no logic duplication, byte-identical with what
//! `axon check` and the type checker see.
//!
//! Contract Fase 12.c stays intact: this module uses only the AST
//! types from `crate::ast` and `std`; no runtime deps, no I/O.

use crate::ast::{
    ChannelDefinition, ConditionalNode, Declaration, DiscoverStatement, EmitStatement,
    EpistemicBlock, FlowStep, ForInStatement, ListenStep, Loc, Program,
    PublishStatement,
};
use crate::tokens::Trivia;

/// Extract outer doc-comment lines (`///` and `/** */`) from a slice of
/// leading trivia, returning each line of body text trimmed.
///
/// Inner doc comments (`//!`, `/*! */`) document the enclosing module
/// rather than the declaration they precede, so they are intentionally
/// excluded — see Fase 14.f hover contract on `channel_hover_markdown`.
fn doc_comment_lines(trivia: &[Trivia]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for t in trivia {
        if !t.is_doc() || t.is_inner_doc() {
            continue;
        }
        for raw in t.stripped_text().lines() {
            // Strip the optional leading space conventionally written
            // after `///` / `/**` (e.g. "/// Hello" yields " Hello").
            let trimmed = raw.strip_prefix(' ').unwrap_or(raw);
            // Drop block-style line-leading `*` decoration if present.
            let trimmed = trimmed.strip_prefix("* ").unwrap_or(trimmed);
            lines.push(trimmed.trim_end().to_string());
        }
    }
    // Drop any leading/trailing blank lines so the rendered hover does
    // not start or end with a blank paragraph.
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

// ─────────────────────────────────────────────────────────────────────
//  Reference kinds — what the LSP must distinguish for find-references.
// ─────────────────────────────────────────────────────────────────────

/// Where a channel is referenced.  Distinguishing kinds lets the LSP
/// render different glyphs in the references panel and lets editors
/// implement "find all consumers" or "find all producers" as filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelRefKind {
    /// `emit Name(value)` — π-calc output prefix.
    Emit,
    /// `emit Outer(InnerChannel)` — channel-as-value mobility (paper §3.2).
    EmitMobility,
    /// `listen Name as alias { … }` — typed canonical input (D4).
    Listen,
    /// `publish Name within Shield` — capability extrusion (D8).
    Publish,
    /// `discover Name as alias` — dual of publish.
    Discover,
}

/// One occurrence of a channel reference in the source.
#[derive(Debug, Clone)]
pub struct ChannelReference {
    pub channel_name: String,
    pub kind: ChannelRefKind,
    pub loc: Loc,
}

// ─────────────────────────────────────────────────────────────────────
//  Channel listing — for outline / document-symbols / completion.
// ─────────────────────────────────────────────────────────────────────

/// List every channel declaration in the program, in source order.
///
/// Drives:
///   - `textDocument/documentSymbol` (outline view)
///   - `textDocument/completion` after `listen `, `emit `, `publish `,
///     `discover ` (the LSP filters by prefix)
///   - `workspace/symbol` aggregation across files
pub fn list_channels(program: &Program) -> Vec<&ChannelDefinition> {
    let mut out = Vec::new();
    collect_channels_in(&program.declarations, &mut out);
    out
}

fn collect_channels_in<'a>(decls: &'a [Declaration], out: &mut Vec<&'a ChannelDefinition>) {
    for decl in decls {
        match decl {
            Declaration::Channel(c) => out.push(c),
            Declaration::Epistemic(eb) => {
                let EpistemicBlock { body, .. } = eb;
                collect_channels_in(body, out);
            }
            _ => {}
        }
    }
}

/// Find a channel by name — drives `textDocument/definition`.
///
/// Returns the AST node so the LSP can pluck `loc` for the jump and
/// any other field it wants to surface in the same hover.
pub fn find_channel_definition<'a>(
    program: &'a Program,
    name: &str,
) -> Option<&'a ChannelDefinition> {
    list_channels(program).into_iter().find(|c| c.name == name)
}

// ─────────────────────────────────────────────────────────────────────
//  Reference walk — for find-all-references and rename refactoring.
// ─────────────────────────────────────────────────────────────────────

/// Find every occurrence of `name` across emit, listen, publish, discover.
///
/// Walks daemons (their listen blocks) and flow bodies (and nested
/// conditional/for-in bodies).  The list is in source order so
/// editors can render a deterministic references panel.
pub fn find_channel_references(program: &Program, name: &str) -> Vec<ChannelReference> {
    let mut refs = Vec::new();
    visit_decls(&program.declarations, name, &mut refs);
    refs
}

fn visit_decls(decls: &[Declaration], target: &str, refs: &mut Vec<ChannelReference>) {
    for decl in decls {
        match decl {
            Declaration::Daemon(d) => {
                for listener in &d.listeners {
                    visit_listen(listener, target, refs);
                }
            }
            Declaration::Flow(f) => visit_flow_body(&f.body, target, refs),
            Declaration::Epistemic(eb) => visit_decls(&eb.body, target, refs),
            _ => {}
        }
    }
}

fn visit_listen(listener: &ListenStep, target: &str, refs: &mut Vec<ChannelReference>) {
    // Only typed-channel listeners count as channel references; legacy
    // string topics are deprecated and resolve at runtime, so they are
    // intentionally excluded from rename / find-references results.
    if listener.channel_is_ref && listener.channel == target {
        refs.push(ChannelReference {
            channel_name: target.to_string(),
            kind: ChannelRefKind::Listen,
            loc: listener.loc.clone(),
        });
    }
}

fn visit_flow_body(steps: &[FlowStep], target: &str, refs: &mut Vec<ChannelReference>) {
    for step in steps {
        visit_flow_step(step, target, refs);
    }
}

fn visit_flow_step(step: &FlowStep, target: &str, refs: &mut Vec<ChannelReference>) {
    match step {
        FlowStep::Emit(EmitStatement {
            channel_ref,
            value_ref,
            loc,
        }) => {
            if channel_ref == target {
                refs.push(ChannelReference {
                    channel_name: target.to_string(),
                    kind: ChannelRefKind::Emit,
                    loc: loc.clone(),
                });
            }
            // The mobility case: `emit Outer(InnerChannel)` references
            // InnerChannel as well.  We surface this so the LSP can
            // show all sites where a handle leaves a flow as a value.
            if value_ref == target && channel_ref != target {
                refs.push(ChannelReference {
                    channel_name: target.to_string(),
                    kind: ChannelRefKind::EmitMobility,
                    loc: loc.clone(),
                });
            }
        }
        FlowStep::Publish(PublishStatement {
            channel_ref, loc, ..
        }) => {
            if channel_ref == target {
                refs.push(ChannelReference {
                    channel_name: target.to_string(),
                    kind: ChannelRefKind::Publish,
                    loc: loc.clone(),
                });
            }
        }
        FlowStep::Discover(DiscoverStatement {
            capability_ref,
            loc,
            ..
        }) => {
            if capability_ref == target {
                refs.push(ChannelReference {
                    channel_name: target.to_string(),
                    kind: ChannelRefKind::Discover,
                    loc: loc.clone(),
                });
            }
        }
        FlowStep::Listen(l) => visit_listen(l, target, refs),
        FlowStep::If(ConditionalNode {
            then_body,
            else_body,
            ..
        }) => {
            visit_flow_body(then_body, target, refs);
            visit_flow_body(else_body, target, refs);
        }
        FlowStep::ForIn(ForInStatement { body, .. }) => {
            visit_flow_body(body, target, refs);
        }
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Hover — markdown content for `textDocument/hover`.
// ─────────────────────────────────────────────────────────────────────

/// Render a channel's metadata as Markdown, suitable as `MarkupContent`
/// in an LSP `Hover` response.
///
/// Output sections, in order:
///   1. **Doc comment** (Fase 14.f) — outer doc trivia (`///` /
///      `/** */`) attached to the channel declaration is rendered as
///      the leading paragraph. Inner doc comments (`//!` / `/*! */`)
///      document the enclosing module rather than this declaration
///      and are deliberately omitted from per-channel hover.
///   2. **Signature block** — fenced `axon` code listing the channel's
///      message type, QoS, lifetime, persistence, and shield.
///   3. **Friendly notes** — second-order channel detection, shield
///      gate explanation. Kept in one place so wording is consistent
///      across IDEs.
///
/// The output is intentionally compact so editors with limited hover
/// real-estate render it well. Editors can extend with additional
/// sections (references count, paper links) outside this function.
pub fn channel_hover_markdown(channel: &ChannelDefinition) -> String {
    let mut buf = String::new();

    // ── §1 Doc comment paragraph (Fase 14.f) ──
    let doc_lines = doc_comment_lines(&channel.leading_trivia);
    if !doc_lines.is_empty() {
        buf.push_str(&doc_lines.join("\n"));
        buf.push_str("\n\n");
    }

    buf.push_str("```axon\n");
    buf.push_str(&format!("channel {} {{\n", channel.name));
    buf.push_str(&format!("  message: {}\n", channel.message));
    buf.push_str(&format!("  qos: {}\n", channel.qos));
    buf.push_str(&format!("  lifetime: {}\n", channel.lifetime));
    buf.push_str(&format!("  persistence: {}\n", channel.persistence));
    if !channel.shield_ref.is_empty() {
        buf.push_str(&format!("  shield: {}\n", channel.shield_ref));
    }
    buf.push_str("}\n");
    buf.push_str("```\n\n");

    // Friendly explanation lines — kept in one place so wording is
    // consistent across IDEs.
    if channel.message.starts_with("Channel<") {
        buf.push_str(
            "**Second-order channel** — carries another channel handle as its \
             message (π-calculus mobility).\n\n",
        );
    }
    if channel.shield_ref.is_empty() {
        buf.push_str(
            "_No shield declared._ This channel cannot be `publish`ed; \
             declare `shield: <ShieldName>` to enable capability extrusion (D8).\n",
        );
    } else {
        buf.push_str(&format!(
            "Capability-gated by **`{}`** — `publish {} within {}` is the \
             only legal extrusion path.\n",
            channel.shield_ref, channel.name, channel.shield_ref,
        ));
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────
//  Completion — names suitable after `listen `, `emit `, `publish `,
//  `discover `.  Returns owned strings so the LSP can build CompletionItems.
// ─────────────────────────────────────────────────────────────────────

/// Channel names available in scope, sorted alphabetically.
///
/// Suitable for any of the four prefix triggers (`listen `, `emit `,
/// `publish `, `discover `).  The LSP filters further as the user
/// types.  For `discover`, the LSP should additionally filter to
/// publishable channels via `is_publishable_channel` below.
pub fn channel_names_in_scope(program: &Program) -> Vec<String> {
    let mut names: Vec<String> = list_channels(program)
        .iter()
        .map(|c| c.name.clone())
        .collect();
    names.sort();
    names
}

/// Names of channels that can be `discover`ed — those declaring `shield_ref`.
///
/// Mirrors the runtime invariant enforced by `_check_discover` (D8).
pub fn publishable_channel_names(program: &Program) -> Vec<String> {
    let mut names: Vec<String> = list_channels(program)
        .iter()
        .filter(|c| !c.shield_ref.is_empty())
        .map(|c| c.name.clone())
        .collect();
    names.sort();
    names
}

/// Detail string for a CompletionItem (one-line summary used by editors
/// next to the name in the popup).
pub fn channel_completion_detail(channel: &ChannelDefinition) -> String {
    let publishable = if channel.shield_ref.is_empty() {
        ""
    } else {
        " · publishable"
    };
    format!(
        "channel<{}, {}, {}>{}",
        channel.message, channel.qos, channel.lifetime, publishable,
    )
}

// ─────────────────────────────────────────────────────────────────────
//  Diagnostics extras — duplicate-channel detection.
//
//  Most Fase-13 diagnostics already flow through the type checker;
//  the only extra the LSP wants is duplicate-name detection that the
//  type checker currently surfaces as a generic SymbolTable error.
//  Exposing it here lets the LSP attach a richer related-information
//  list (each duplicate site).
// ─────────────────────────────────────────────────────────────────────

/// Locate duplicate channel declarations.  Each returned tuple is
/// `(name, definitions)` where the definitions list has length ≥ 2.
pub fn duplicate_channels(program: &Program) -> Vec<(String, Vec<&ChannelDefinition>)> {
    use std::collections::BTreeMap;
    let mut by_name: BTreeMap<&str, Vec<&ChannelDefinition>> = BTreeMap::new();
    for c in list_channels(program) {
        by_name.entry(&c.name).or_default().push(c);
    }
    by_name
        .into_iter()
        .filter(|(_, defs)| defs.len() > 1)
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────
//  Trigger detection — what completion list to offer at a position.
//
//  The LSP normally drives this from textDocument/completion's trigger
//  characters, but exposing it here lets a unit test cover the
//  decision logic without spinning up the LSP server.
// ─────────────────────────────────────────────────────────────────────

/// Which completion list the LSP should offer when the user has typed
/// up to the cursor.  Returns `None` if no channel-related completion
/// is appropriate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelCompletionTrigger {
    /// After `listen ` — typed channel ref preferred over string.
    Listen,
    /// After `emit ` — names of channels to emit on.
    Emit,
    /// After `publish ` — only publishable channels.
    Publish,
    /// After `discover ` — only publishable channels.
    Discover,
}

/// Inspect the line text up to the cursor and decide whether a
/// channel completion list applies.  Returns the trigger flavour so
/// the LSP can choose between `channel_names_in_scope` and
/// `publishable_channel_names`.
pub fn detect_channel_trigger(line_prefix: &str) -> Option<ChannelCompletionTrigger> {
    let trimmed = line_prefix.trim_start();
    // Match the most recent token before the cursor.  We accept either
    // the keyword followed by a space or the keyword as the entire
    // tail (cursor right after, i.e. `listen|`).
    for (kw, kind) in [
        ("listen", ChannelCompletionTrigger::Listen),
        ("emit", ChannelCompletionTrigger::Emit),
        ("publish", ChannelCompletionTrigger::Publish),
        ("discover", ChannelCompletionTrigger::Discover),
    ] {
        if trimmed == kw || trimmed.starts_with(&format!("{kw} ")) {
            return Some(kind);
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src, "<test>").tokenize().expect("lex");
        Parser::new(tokens).parse().expect("parse")
    }

    // ── list_channels ──────────────────────────────────────────────

    #[test]
    fn list_channels_returns_in_source_order() {
        let p = parse(
            r#"
            channel Beta { message: T }
            channel Alpha { message: T }
            channel Gamma { message: T }
        "#,
        );
        let names: Vec<&str> = list_channels(&p).iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Beta", "Alpha", "Gamma"]);
    }

    #[test]
    fn list_channels_descends_into_epistemic_blocks() {
        let p = parse(
            r#"
            know { channel Inside { message: T } }
            channel Outside { message: T }
        "#,
        );
        let names: Vec<String> = list_channels(&p).iter().map(|c| c.name.clone()).collect();
        assert!(names.contains(&"Inside".to_string()));
        assert!(names.contains(&"Outside".to_string()));
    }

    // ── find_channel_definition ────────────────────────────────────

    #[test]
    fn find_channel_definition_returns_node_with_loc() {
        let p = parse("channel C { message: Order }");
        let c = find_channel_definition(&p, "C").expect("found");
        assert_eq!(c.name, "C");
        assert!(c.loc.line > 0);
    }

    #[test]
    fn find_channel_definition_unknown_returns_none() {
        let p = parse("channel C { message: T }");
        assert!(find_channel_definition(&p, "Bogus").is_none());
    }

    // ── find_channel_references ────────────────────────────────────

    #[test]
    fn find_references_emit_publish_discover_listen() {
        let p = parse(
            r#"
            channel C { message: Order }
            daemon D() {
              goal: "x"
              listen C as ev { }
            }
            flow f() -> O {
              emit C(payload)
              publish C within Gate
              discover C as ch
            }
        "#,
        );
        let refs = find_channel_references(&p, "C");
        let kinds: Vec<ChannelRefKind> = refs.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ChannelRefKind::Listen));
        assert!(kinds.contains(&ChannelRefKind::Emit));
        assert!(kinds.contains(&ChannelRefKind::Publish));
        assert!(kinds.contains(&ChannelRefKind::Discover));
    }

    #[test]
    fn find_references_distinguishes_mobility_from_emit() {
        let p = parse(
            r#"
            channel Inner { message: Order }
            channel Outer { message: Channel<Order> }
            flow f() -> O { emit Outer(Inner) }
        "#,
        );
        let refs_inner = find_channel_references(&p, "Inner");
        // Inner appears once as the value of an emit — that's mobility.
        assert_eq!(refs_inner.len(), 1);
        assert_eq!(refs_inner[0].kind, ChannelRefKind::EmitMobility);
        let refs_outer = find_channel_references(&p, "Outer");
        assert_eq!(refs_outer.len(), 1);
        assert_eq!(refs_outer[0].kind, ChannelRefKind::Emit);
    }

    #[test]
    fn find_references_skips_legacy_string_topics() {
        let p = parse(
            r#"
            channel C { message: Order }
            daemon D() {
              goal: "x"
              listen "C" as ev { }
            }
        "#,
        );
        // The string topic "C" shadows the channel name but is a
        // legacy literal, NOT a typed reference — should not appear.
        let refs = find_channel_references(&p, "C");
        assert!(
            refs.is_empty(),
            "string topics must not appear in channel references: {:?}",
            refs
        );
    }

    #[test]
    fn find_references_descends_into_conditionals_and_for_loops() {
        let p = parse(
            r#"
            channel C { message: T }
            flow f() -> O {
              if x == 1 {
                emit C(p)
              } else {
                publish C within Gate
              }
              for item in items {
                discover C as ch
              }
            }
        "#,
        );
        let refs = find_channel_references(&p, "C");
        let kinds: Vec<ChannelRefKind> = refs.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ChannelRefKind::Emit));
        assert!(kinds.contains(&ChannelRefKind::Publish));
        assert!(kinds.contains(&ChannelRefKind::Discover));
    }

    // ── channel_hover_markdown ─────────────────────────────────────

    #[test]
    fn hover_includes_signature_block() {
        let p = parse("channel C { message: Order qos: exactly_once shield: G }");
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(md.contains("```axon"));
        assert!(md.contains("channel C"));
        assert!(md.contains("message: Order"));
        assert!(md.contains("qos: exactly_once"));
        assert!(md.contains("shield: G"));
    }

    #[test]
    fn hover_flags_second_order_channel() {
        let p = parse("channel C { message: Channel<Order> }");
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(md.contains("Second-order channel"));
        assert!(md.contains("π-calculus mobility"));
    }

    #[test]
    fn hover_explains_publish_gate_when_present() {
        let p = parse("channel C { message: T shield: Gate }");
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(md.contains("Capability-gated by"));
        assert!(md.contains("Gate"));
    }

    #[test]
    fn hover_warns_when_shield_missing() {
        let p = parse("channel C { message: T }");
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(md.contains("No shield declared"));
        assert!(md.contains("D8"));
    }

    // ── completions ────────────────────────────────────────────────

    #[test]
    fn channel_names_in_scope_are_sorted() {
        let p = parse(
            r#"
            channel Zulu { message: T }
            channel Alpha { message: T }
            channel Mike { message: T }
        "#,
        );
        assert_eq!(
            channel_names_in_scope(&p),
            vec!["Alpha".to_string(), "Mike".to_string(), "Zulu".to_string()],
        );
    }

    #[test]
    fn publishable_filter_excludes_shieldless_channels() {
        let p = parse(
            r#"
            channel Public { message: T shield: Gate }
            channel Private { message: T }
        "#,
        );
        assert_eq!(publishable_channel_names(&p), vec!["Public".to_string()]);
    }

    #[test]
    fn completion_detail_marks_publishable_channels() {
        let p = parse("channel C { message: Order qos: exactly_once shield: Gate }");
        let c = find_channel_definition(&p, "C").unwrap();
        let detail = channel_completion_detail(c);
        assert!(detail.contains("Order"));
        assert!(detail.contains("exactly_once"));
        assert!(detail.contains("publishable"));
    }

    #[test]
    fn completion_detail_omits_publishable_when_no_shield() {
        let p = parse("channel C { message: T }");
        let c = find_channel_definition(&p, "C").unwrap();
        let detail = channel_completion_detail(c);
        assert!(!detail.contains("publishable"));
    }

    // ── duplicate detection ────────────────────────────────────────

    #[test]
    fn duplicate_channels_detected() {
        let p = parse(
            r#"
            channel C { message: T }
            channel C { message: U }
        "#,
        );
        let dups = duplicate_channels(&p);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].0, "C");
        assert_eq!(dups[0].1.len(), 2);
    }

    // ── §Fase 14.f — hover enrichment with doc comments ─────────────

    #[test]
    fn hover_prepends_outer_doc_line_comment() {
        let p = parse(
            "/// Inbound order events from the broker.\nchannel C { message: Order shield: Gate }",
        );
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        // Doc paragraph appears before the signature block.
        let doc_pos = md.find("Inbound order events").expect("doc text missing");
        let sig_pos = md.find("```axon").expect("signature missing");
        assert!(doc_pos < sig_pos, "doc must come before signature: {md}");
    }

    #[test]
    fn hover_renders_multiple_outer_doc_lines_as_paragraph() {
        let src = "/// Line one.\n/// Line two.\nchannel C { message: T shield: Gate }";
        let p = parse(src);
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(md.contains("Line one."));
        assert!(md.contains("Line two."));
    }

    #[test]
    fn hover_renders_outer_doc_block_comment() {
        let src = "/** Block-form docs for C. */\nchannel C { message: T shield: Gate }";
        let p = parse(src);
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(md.contains("Block-form docs for C."));
    }

    #[test]
    fn hover_omits_inner_doc_comment() {
        // Inner doc comments document the enclosing module, not the
        // declaration that follows. They must NOT appear in per-channel
        // hover.
        let src = "//! file-level docs that should not leak\nchannel C { message: T shield: Gate }";
        let p = parse(src);
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(
            !md.contains("file-level docs that should not leak"),
            "inner doc must not appear in channel hover: {md}",
        );
    }

    #[test]
    fn hover_omits_regular_line_comment() {
        // Regular `//` comments are not documentation. They must not be
        // surfaced as the channel's doc paragraph.
        let src = "// internal note that should not surface\nchannel C { message: T shield: Gate }";
        let p = parse(src);
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(
            !md.contains("internal note that should not surface"),
            "regular comment leaked into hover: {md}",
        );
    }

    #[test]
    fn hover_with_no_doc_comment_unchanged() {
        // Backward-compat: when no doc trivia is present the hover
        // output starts with the signature block exactly like pre-14.f.
        let p = parse("channel C { message: Order shield: Gate }");
        let c = find_channel_definition(&p, "C").unwrap();
        let md = channel_hover_markdown(c);
        assert!(md.starts_with("```axon\n"));
    }

    #[test]
    fn duplicate_channels_empty_when_unique() {
        let p = parse(
            r#"
            channel A { message: T }
            channel B { message: T }
        "#,
        );
        assert!(duplicate_channels(&p).is_empty());
    }

    // ── trigger detection ──────────────────────────────────────────

    #[test]
    fn detect_trigger_recognizes_each_keyword() {
        for (line, expected) in [
            ("  listen ", Some(ChannelCompletionTrigger::Listen)),
            ("emit ", Some(ChannelCompletionTrigger::Emit)),
            ("    publish ", Some(ChannelCompletionTrigger::Publish)),
            ("discover ", Some(ChannelCompletionTrigger::Discover)),
            ("listen", Some(ChannelCompletionTrigger::Listen)),
        ] {
            assert_eq!(detect_channel_trigger(line), expected, "line {line:?}");
        }
    }

    #[test]
    fn detect_trigger_returns_none_outside_keyword() {
        assert_eq!(detect_channel_trigger("step S {"), None);
        assert_eq!(detect_channel_trigger("flow f() ->"), None);
        assert_eq!(detect_channel_trigger(""), None);
    }
}
