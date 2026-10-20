//! The UI Doctrine lint (D1-D18) over the Project Template's Rust files.
//!
//! Files are lexed with `proc_macro2` (comments dropped; doc comments and
//! string/char literals are literals and never match). An ident's **words**
//! drop `r#`, split at `_` and camel-case boundaries, and are lowercased.
//!
//! An **interactive handler** is a `fn` item whose attributes include one
//! whose path ends in `command`, `user_command`, `message_command` or
//! `handler`; an **event handler** one ending in `event`. A handler's scope is
//! its signature and body. Handlers inside modules and impls are found too.
//!
//! - UD-1, anywhere: an ident with the word `embed`/`embeds`. In an
//!   interactive handler: `LegacyMessage`; in its body, a `.content(` call or a
//!   `content:` struct-literal field.
//! - UD-3, anywhere: the response-type idents (`InteractionResponseType`, …).
//!   In an interactive handler: `.reply(`, `.update(`, `.defer_reply(`,
//!   `.defer_update(`, `.edit_reply(` on any receiver but `ctx` (the handler
//!   context's `ctx.update(msg)` picks the response type itself, FR-61); a
//!   `Responder`/`CommandResponder`/`ComponentResponder` parameter without
//!   `Manual` (the `auto_defer = false` marker) in its generic args, and any
//!   `ModalResponder` parameter.
//! - UD-7, anywhere: an ident with the word `collector(s)` or `listener(s)`;
//!   the await-loop idents (`wait_for`, …); a `.on_<x>(` call or a method
//!   whose words contain `listen`/`listener`, with a top-level `|` or `move`
//!   in its arguments. In an interactive or event handler: `spawn`,
//!   `spawn_local`.

use std::str::FromStr;

use proc_macro2::{Delimiter, Ident, Spacing, TokenStream, TokenTree};

use super::Problems;

const UD1_EMBED: &str = "[UD-1] embeds are not an interactive output; build a View";
const UD1_LEGACY: &str = "[UD-1] LegacyMessage in an interactive handler; return a View";
const UD1_CONTENT: &str = "[UD-1] content as handler output; return a View";
const UD3_TYPE: &str = "[UD-3] manual response-type selection; send the rendered message with ctx.respond/ctx.update and let Noctis pick the response";
const UD3_CALL: &str =
    "[UD-3] manual response through the Responder; use ctx.respond(msg) or ctx.update(msg)";
const UD7_COLLECTOR: &str = "[UD-7] collectors are not allowed; one #[handler] per Route";
const UD7_AWAIT: &str = "[UD-7] await-component loop; state goes in Payloads, Refs or ports";
const UD7_LISTENER: &str = "[UD-7] inline listener; register a #[handler] for a Route";
const UD7_SPAWN: &str = "[UD-7] task spawned per interaction or event; use a Ref TTL or a port";

const RESPONSE_TYPES: &[&str] = &[
    "InteractionResponseType",
    "InteractionResponse",
    "InteractionResponseData",
    "InteractionCallbackType",
    "CallbackType",
    "ResponseType",
];
const AWAIT_LOOPS: &[&str] = &[
    "wait_for",
    "wait_for_component",
    "wait_for_interaction",
    "await_component",
    "await_interaction",
    "next_component",
    "next_interaction",
    "component_stream",
    "interaction_stream",
];
const RESPONDER_CALLS: &[&str] = &[
    "reply",
    "update",
    "defer_reply",
    "defer_update",
    "edit_reply",
];
/// The receiver whose methods pick the response type themselves
/// (`ctx.respond`, `ctx.update`, `ctx.reply_text`).
const CTX_RECEIVER: &str = "ctx";
const RESPONDERS: &[&str] = &[
    "Responder",
    "CommandResponder",
    "ComponentResponder",
    "ModalResponder",
];
const INTERACTIVE_ATTRS: &[&str] = &["command", "user_command", "message_command", "handler"];

/// One finding: 1-based line and message.
type Finding = (usize, String);

/// Lints one file, reporting under `path`.
pub(super) fn check(path: &str, src: &str, problems: &mut Problems) {
    match lint(src) {
        Ok(findings) => {
            for (line, message) in findings {
                problems.add(format!("{path}:{line}"), message);
            }
        }
        Err(err) => problems.add(path, format!("cannot lex: {err}")),
    }
}

/// Lints `src`; `Err` is the lexer's message.
fn lint(src: &str) -> Result<Vec<Finding>, String> {
    let stream = TokenStream::from_str(src).map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    walk(&stream.into_iter().collect::<Vec<_>>(), &mut out);
    out.sort();
    out.dedup();
    Ok(out)
}

/// The words of an ident (see the module docs).
fn words(ident: &str) -> Vec<String> {
    let ident = ident.strip_prefix("r#").unwrap_or(ident);
    let mut out = Vec::new();
    for part in ident.split('_') {
        let chars: Vec<char> = part.chars().collect();
        let mut word = String::new();
        for (i, &c) in chars.iter().enumerate() {
            let boundary = i > 0
                && c.is_uppercase()
                && (chars[i - 1].is_lowercase()
                    || chars[i - 1].is_ascii_digit()
                    || (chars[i - 1].is_uppercase()
                        && chars.get(i + 1).is_some_and(|n| n.is_lowercase())));
            if boundary && !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            word.extend(c.to_lowercase());
        }
        if !word.is_empty() {
            out.push(word);
        }
    }
    out
}

fn name(ident: &Ident) -> String {
    let s = ident.to_string();
    s.strip_prefix("r#").map(str::to_owned).unwrap_or(s)
}

fn line(ident: &Ident) -> usize {
    ident.span().start().line
}

fn tokens(stream: TokenStream) -> Vec<TokenTree> {
    stream.into_iter().collect()
}

fn is_punct(tt: Option<&TokenTree>, ch: char) -> bool {
    matches!(tt, Some(TokenTree::Punct(p)) if p.as_char() == ch)
}

fn paren(tt: Option<&TokenTree>) -> Option<Vec<TokenTree>> {
    match tt {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
            Some(tokens(g.stream()))
        }
        _ => None,
    }
}

/// `.name(args)` at `tts[i]` (the `.`) → (method ident, args).
fn method_call(tts: &[TokenTree], i: usize) -> Option<(&Ident, Vec<TokenTree>)> {
    if !is_punct(tts.get(i), '.') {
        return None;
    }
    let TokenTree::Ident(method) = tts.get(i + 1)? else {
        return None;
    };
    Some((method, paren(tts.get(i + 2))?))
}

/// Calls `f` on `tts` and every nested group's tokens, with the delimiter.
fn each_level(
    tts: &[TokenTree],
    delimiter: Option<Delimiter>,
    f: &mut impl FnMut(&[TokenTree], Option<Delimiter>),
) {
    f(tts, delimiter);
    for tt in tts {
        if let TokenTree::Group(g) = tt {
            each_level(&tokens(g.stream()), Some(g.delimiter()), f);
        }
    }
}

/// File-wide checks at every level, then the handlers at every level.
fn walk(tts: &[TokenTree], out: &mut Vec<Finding>) {
    each_level(tts, None, &mut |level, _| {
        file_wide(level, out);
        handlers(level, out);
    });
}

fn file_wide(tts: &[TokenTree], out: &mut Vec<Finding>) {
    for (i, tt) in tts.iter().enumerate() {
        if let TokenTree::Ident(ident) = tt {
            let n = name(ident);
            let w = words(&n);
            let has = |set: &[&str]| w.iter().any(|w| set.contains(&w.as_str()));
            if has(&["embed", "embeds"]) {
                out.push((line(ident), UD1_EMBED.to_owned()));
            }
            if RESPONSE_TYPES.contains(&n.as_str()) {
                out.push((line(ident), UD3_TYPE.to_owned()));
            }
            if has(&["collector", "collectors"]) {
                out.push((line(ident), UD7_COLLECTOR.to_owned()));
            }
            if AWAIT_LOOPS.contains(&n.as_str()) {
                out.push((line(ident), UD7_AWAIT.to_owned()));
            }
            if has(&["listener", "listeners"]) {
                out.push((line(ident), UD7_LISTENER.to_owned()));
            }
        }
        if let Some((method, args)) = method_call(tts, i) {
            let n = name(method);
            let listens =
                n.starts_with("on_") || words(&n).iter().any(|w| w == "listen" || w == "listener");
            let closure = args.iter().any(|a| match a {
                TokenTree::Punct(p) => p.as_char() == '|',
                TokenTree::Ident(id) => id == "move",
                _ => false,
            });
            if listens && closure {
                out.push((line(method), UD7_LISTENER.to_owned()));
            }
        }
    }
}

/// The last path segment of an attribute's bracket contents.
fn attr_name(tts: &[TokenTree]) -> Option<String> {
    let mut last = None;
    for tt in tts {
        match tt {
            TokenTree::Ident(id) => last = Some(name(id)),
            TokenTree::Punct(p) if p.as_char() == ':' => {}
            _ => break,
        }
    }
    last
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Handler {
    Interactive,
    Event,
}

/// Finds attributed `fn` items at this level and checks the handlers.
fn handlers(tts: &[TokenTree], out: &mut Vec<Finding>) {
    let mut attrs: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tts.len() {
        if is_punct(tts.get(i), '#') {
            let (inner, at) = if is_punct(tts.get(i + 1), '!') {
                (true, i + 2)
            } else {
                (false, i + 1)
            };
            if let Some(TokenTree::Group(g)) = tts.get(at)
                && g.delimiter() == Delimiter::Bracket
            {
                if !inner && let Some(n) = attr_name(&tokens(g.stream())) {
                    attrs.push(n);
                }
                i = at + 1;
                continue;
            }
        }
        if attrs.is_empty() {
            i += 1;
            continue;
        }
        let kind = if attrs
            .iter()
            .any(|a| INTERACTIVE_ATTRS.contains(&a.as_str()))
        {
            Some(Handler::Interactive)
        } else if attrs.iter().any(|a| a == "event") {
            Some(Handler::Event)
        } else {
            None
        };
        attrs.clear();
        // Skip visibility and qualifiers up to `fn`.
        let mut j = i;
        while let Some(tt) = tts.get(j) {
            match tt {
                TokenTree::Ident(id)
                    if ["pub", "async", "const", "unsafe", "extern"]
                        .contains(&name(id).as_str()) => {}
                TokenTree::Group(g) if g.delimiter() == Delimiter::Parenthesis => {}
                TokenTree::Literal(_) => {}
                _ => break,
            }
            j += 1;
        }
        let is_fn = matches!(tts.get(j), Some(TokenTree::Ident(id)) if id == "fn");
        let (Some(kind), true) = (kind, is_fn) else {
            i = j.max(i + 1);
            continue;
        };
        let mut params = None;
        let mut body = None;
        let mut end = j + 1;
        while let Some(tt) = tts.get(end) {
            match tt {
                TokenTree::Group(g)
                    if g.delimiter() == Delimiter::Parenthesis && params.is_none() =>
                {
                    params = Some(tokens(g.stream()));
                }
                TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => {
                    body = Some(tokens(g.stream()));
                    break;
                }
                TokenTree::Punct(p) if p.as_char() == ';' => break,
                _ => {}
            }
            end += 1;
        }
        check_handler(
            kind,
            &tts[j..end.min(tts.len())],
            params.as_deref(),
            body.as_deref(),
            out,
        );
        i = end + 1;
    }
}

fn check_handler(
    kind: Handler,
    signature: &[TokenTree],
    params: Option<&[TokenTree]>,
    body: Option<&[TokenTree]>,
    out: &mut Vec<Finding>,
) {
    let interactive = kind == Handler::Interactive;
    let scan_idents = |tts: &[TokenTree], out: &mut Vec<Finding>, in_body: bool| {
        each_level(tts, None, &mut |level, _| {
            for tt in level {
                if let TokenTree::Ident(id) = tt {
                    let n = name(id);
                    if interactive && n == "LegacyMessage" {
                        out.push((line(id), UD1_LEGACY.to_owned()));
                    }
                    if in_body && (n == "spawn" || n == "spawn_local") {
                        out.push((line(id), UD7_SPAWN.to_owned()));
                    }
                }
            }
        });
    };
    scan_idents(signature, out, false);
    if let Some(body) = body {
        scan_idents(body, out, true);
    }
    if !interactive {
        return;
    }
    if let Some(params) = params {
        responder_params(params, out);
    }
    if let Some(body) = body {
        each_level(body, Some(Delimiter::Brace), &mut |level, delimiter| {
            for i in 0..level.len() {
                if let Some((method, _)) = method_call(level, i) {
                    let n = name(method);
                    if n == "content" {
                        out.push((line(method), UD1_CONTENT.to_owned()));
                    }
                    let on_ctx = i > 0
                        && matches!(level.get(i - 1), Some(TokenTree::Ident(id)) if name(id) == CTX_RECEIVER);
                    if RESPONDER_CALLS.contains(&n.as_str()) && !on_ctx {
                        out.push((line(method), UD3_CALL.to_owned()));
                    }
                }
                // `{ content: …` / `, content: …` in a struct literal.
                if delimiter == Some(Delimiter::Brace)
                    && let Some(TokenTree::Ident(id)) = level.get(i)
                    && id == "content"
                    && (i == 0 || is_punct(level.get(i - 1), ','))
                    && matches!(level.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Alone)
                {
                    out.push((line(id), UD1_CONTENT.to_owned()));
                }
            }
        });
    }
}

/// D10: Responder parameters that cannot only open a modal.
fn responder_params(params: &[TokenTree], out: &mut Vec<Finding>) {
    for (i, tt) in params.iter().enumerate() {
        let TokenTree::Ident(id) = tt else { continue };
        let n = name(id);
        if !RESPONDERS.contains(&n.as_str()) {
            continue;
        }
        // Not the last segment: `Responder::x`.
        if is_punct(params.get(i + 1), ':') {
            continue;
        }
        let mut manual = false;
        if is_punct(params.get(i + 1), '<') {
            let mut depth = 0_i32;
            let mut prev_minus = false;
            for tt in &params[i + 1..] {
                match tt {
                    TokenTree::Punct(p) if p.as_char() == '<' => depth += 1,
                    TokenTree::Punct(p) if p.as_char() == '>' && !prev_minus => depth -= 1,
                    TokenTree::Ident(arg) if arg == "Manual" => {
                        manual = true;
                    }
                    _ => {}
                }
                prev_minus = matches!(tt, TokenTree::Punct(p) if p.as_char() == '-');
                if depth == 0 {
                    break;
                }
            }
        }
        if n == "ModalResponder" || !manual {
            out.push((
                line(id),
                format!("[UD-3] take a Responder only to open a modal ({n}<Manual>)"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(src: &str) -> Vec<String> {
        lint(src)
            .unwrap()
            .into_iter()
            .map(|(line, msg)| format!("{line}: {msg}"))
            .collect()
    }

    fn clean(src: &str) {
        assert_eq!(findings(src), Vec::<String>::new(), "{src}");
    }

    fn flags(src: &str, message: &str) {
        let got = findings(src);
        assert!(got.iter().any(|f| f.ends_with(message)), "{src}\n{got:?}");
    }

    #[test]
    fn word_splitting() {
        assert_eq!(words("EmbedBuilder"), ["embed", "builder"]);
        assert_eq!(words("add_embed"), ["add", "embed"]);
        assert_eq!(words("r#ComponentCollector"), ["component", "collector"]);
        assert_eq!(words("HTTPListener2"), ["http", "listener2"]);
        assert_eq!(words("listen_addr"), ["listen", "addr"]);
        assert_eq!(words("v2Embed"), ["v2", "embed"]);
    }

    #[test]
    fn d2_d3_embeds() {
        for src in [
            "fn f() { let x = Embed::new(); }",
            "struct S { embeds: u8 }",
            "fn add_embed() {}",
            "type T = EmbedBuilder;",
        ] {
            flags(src, UD1_EMBED);
        }
        clean("fn f() { let embedded = \"embed\"; } // embed\n/// embeds\nfn g() {}");
    }

    #[test]
    fn d4_d5_legacy_message() {
        flags(
            "#[handler]\nasync fn h(ctx: Ctx) -> LegacyMessage { todo() }",
            UD1_LEGACY,
        );
        flags(
            "#[command(\"x\")]\nasync fn h() { let m: LegacyMessage = x(); }",
            UD1_LEGACY,
        );
        clean(
            "#[event]\nasync fn h() -> LegacyMessage { x() }\nfn plain() -> LegacyMessage { x() }",
        );
    }

    #[test]
    fn d6_d7_content() {
        flags(
            "#[handler]\nasync fn h() { reply().content(\"x\") }",
            UD1_CONTENT,
        );
        flags(
            "#[handler]\nasync fn h() { Msg { content: x } }",
            UD1_CONTENT,
        );
        flags(
            "#[handler]\nasync fn h() { Msg { a, content: x } }",
            UD1_CONTENT,
        );
        clean(
            "#[handler]\nasync fn h(content: &str) { let a = msg.content; content::x(); let content: String = f(a); }\n\
             fn plain() { Msg { content: x }.content(); }",
        );
    }

    #[test]
    fn d8_response_types() {
        flags("fn f() -> InteractionResponseType { x }", UD3_TYPE);
        flags("use x::CallbackType;", UD3_TYPE);
    }

    #[test]
    fn d9_responder_calls() {
        for call in [
            "reply",
            "update",
            "defer_reply",
            "defer_update",
            "edit_reply",
        ] {
            flags(
                &format!("#[handler]\nasync fn h() {{ r.{call}(x).await }}"),
                UD3_CALL,
            );
        }
        clean("fn plain() { r.update(x); }");
        // The handler context's methods pick the response type (FR-61).
        clean("#[handler]\nasync fn h(ctx: ComponentCtx) { ctx.update(render(x)).await }");
        clean(
            "#[command(\"x\")]\nasync fn h(ctx: CommandCtx) { ctx.respond(m).await; ctx.reply_text(k).await }",
        );
        flags(
            "#[handler]\nasync fn h() { my_ctx.update(x).await }",
            UD3_CALL,
        );
    }

    #[test]
    fn d10_d11_responders() {
        let msg = "[UD-3] take a Responder only to open a modal (ComponentResponder<Manual>)";
        flags("#[handler]\nasync fn h(r: ComponentResponder) {}", msg);
        flags(
            "#[handler]\nasync fn h(r: noctis::ComponentResponder<Plain>) {}",
            msg,
        );
        flags(
            "#[handler]\nasync fn h(r: ModalResponder<Manual>) {}",
            "[UD-3] take a Responder only to open a modal (ModalResponder<Manual>)",
        );
        clean(
            "impl EditBio {\n#[handler]\nasync fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> { Ok(r.show_modal(m).await?.done()) }\n}",
        );
        clean(
            "#[command(\"feedback\", auto_defer = false)]\nasync fn feedback(ctx: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> { Ok(r.show_modal(m).await?.done()) }",
        );
        // The withdrawn marker is not `Manual`.
        flags(
            "#[command(\"feedback\")]\nasync fn feedback(ctx: CommandCtx, r: CommandResponder<OpensModal>) {}",
            "[UD-3] take a Responder only to open a modal (CommandResponder<Manual>)",
        );
    }

    #[test]
    fn d12_d13_collectors() {
        flags(
            "fn f() { let c = ComponentCollector::new(); }",
            UD7_COLLECTOR,
        );
        flags("fn f() { create_collector(); }", UD7_COLLECTOR);
        clean("fn f() { let v = it.collect::<Vec<_>>(); it.collect(); }");
    }

    #[test]
    fn d14_await_loops() {
        flags("fn f() { ctx.wait_for_component().await; }", UD7_AWAIT);
    }

    #[test]
    fn d15_d16_listeners() {
        flags("fn f() { app.on_click(|x| y); }", UD7_LISTENER);
        flags("fn f() { app.on_click(move || y); }", UD7_LISTENER);
        flags("fn f() { app.add_listener(handler); }", UD7_LISTENER);
        flags("fn f() { app.listen(|e| e); }", UD7_LISTENER);
        clean(
            "fn f() { app.on_error(handler_fn); let s = Cfg { listen_addr: a }; s.listen_addr; }",
        );
    }

    #[test]
    fn d17_spawn() {
        flags(
            "#[handler]\nasync fn h() { tokio::spawn(async {}); }",
            UD7_SPAWN,
        );
        flags(
            "#[event]\nasync fn h() { tokio::task::spawn_local(f()); }",
            UD7_SPAWN,
        );
        clean("fn main() { tokio::spawn(run()); }");
    }

    #[test]
    fn d18_lex_error_and_nesting() {
        assert!(lint("fn f() { (").is_err());
        flags(
            "mod m {\n    #[handler]\n    async fn h() { r.defer_reply() }\n}",
            UD3_CALL,
        );
        flags(
            "impl X {\n    #[doc = \"x\"]\n    #[noctis::handler]\n    pub async fn h(&self) { r.defer_reply() }\n}",
            UD3_CALL,
        );
        // Line numbers come from spans.
        assert_eq!(
            findings("\n\n#[handler]\nasync fn h() {\n    r.defer_reply();\n}"),
            [format!("5: {UD3_CALL}")]
        );
    }
}
