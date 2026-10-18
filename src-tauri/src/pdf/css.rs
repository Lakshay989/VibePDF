//! SPEC: P7-OCR-009 (P7.C1c) — the CSS an HTML page brings, for the HTML
//! reader: stylesheet rules and inline declarations, each selector's
//! specificity, and the property values the flow model can use.
//!
//! **Not a browser's CSS engine.** Selector *matching* is `dom_query`'s (Servo's
//! `selectors`); this module parses the text around it and turns values into
//! points and RGB, the units the flow model works in. At-rules (`@media`,
//! `@font-face`, `@import`, `@page`) are skipped whole, so screen-only layout
//! never reaches a printed page by accident.

/// `(ids, classes and attributes and pseudo-classes, types and pseudo-elements)`,
/// compared in that order.
pub type Specificity = (u32, u32, u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// Lower-case.
    pub name: String,
    pub value: String,
    pub important: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// One entry per selector of the rule's comma-separated list.
    pub selectors: Vec<String>,
    pub declarations: Vec<Declaration>,
}

// ---------------------------------------------------------------------------
// Rules

/// Comments replaced by a space; quoted strings kept whole, so a `/*` inside
/// one is text.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                out.extend(chars.next());
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut previous = '\0';
            for n in chars.by_ref() {
                if previous == '*' && n == '/' {
                    break;
                }
                previous = n;
            }
            out.push(' ');
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
        }
        out.push(c);
    }
    out
}

/// Byte offsets of each top-level occurrence of `wanted` in `s`: outside
/// quotes, parentheses and brackets. With `blocks`, `{}` nest too.
fn top_level(s: &str, wanted: char, blocks: bool) -> Vec<usize> {
    let mut found = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (at, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == wanted && depth == 0 {
            found.push(at);
            if wanted == '{' {
                // Only the first is wanted when looking for a block's start.
                break;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            '{' if blocks => depth += 1,
            '}' if blocks => depth = (depth - 1).max(0),
            _ => {}
        }
    }
    found
}

/// `s` split at top-level `separator`s.
#[must_use]
pub fn split_top(s: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for at in top_level(s, separator, true) {
        parts.push(&s[start..at]);
        start = at + separator.len_utf8();
    }
    parts.push(&s[start..]);
    parts
}

/// The offset of the `}` closing the block opened at `open`.
fn block_end(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (at, c) in s[open..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The rules of a stylesheet, in order. At-rules are skipped whole.
#[must_use]
pub fn parse_stylesheet(text: &str) -> Vec<Rule> {
    // Old pages hide <style> contents from ancient browsers in HTML comments.
    let text = strip_comments(text).replace("<!--", " ").replace("-->", " ");
    let mut rules = Vec::new();
    let mut rest = text.as_str();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let open = top_level(rest, '{', false).first().copied();
        if rest.starts_with('@') {
            // An at-rule ends at its `;`, or with its block.
            let semi = top_level(rest, ';', false).first().copied();
            match (semi, open) {
                (Some(s), o) if o.is_none_or(|o| s < o) => rest = &rest[s + 1..],
                (_, Some(o)) => rest = block_end(rest, o).map_or("", |end| &rest[end + 1..]),
                _ => break,
            }
            continue;
        }
        let Some(open) = open else { break };
        let end = block_end(rest, open);
        let body = &rest[open + 1..end.unwrap_or(rest.len())];
        let selectors: Vec<String> = split_top(&rest[..open], ',').into_iter().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
        let declarations = parse_declarations(body);
        if !selectors.is_empty() && !declarations.is_empty() {
            rules.push(Rule { selectors, declarations });
        }
        rest = end.map_or("", |end| &rest[end + 1..]);
    }
    rules
}

/// `name: value` pairs separated by `;`, as in a `style` attribute. Custom
/// properties and values that use them (`var()`) can't be resolved and are
/// left out.
#[must_use]
pub fn parse_declarations(text: &str) -> Vec<Declaration> {
    let text = strip_comments(text);
    let mut out = Vec::new();
    for part in split_top(&text, ';') {
        let Some(&colon) = top_level(part, ':', true).first() else { continue };
        let name = part[..colon].trim().to_ascii_lowercase();
        let mut value = part[colon + 1..].trim();
        let mut important = false;
        if let Some(bang) = value.rfind('!') {
            if value[bang + 1..].trim().eq_ignore_ascii_case("important") {
                important = true;
                value = value[..bang].trim_end();
            }
        }
        if name.is_empty() || value.is_empty() || name.starts_with("--") || value.contains("var(") {
            continue;
        }
        out.push(Declaration { name, value: value.to_owned(), important });
    }
    out
}

// ---------------------------------------------------------------------------
// Specificity

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || c == '\\' || !c.is_ascii()
}

/// The index after the name starting at `from`.
fn name_end(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i < chars.len() && is_name_char(chars[i]) {
        // An escape takes the next character with it.
        i += if chars[i] == '\\' { 2 } else { 1 };
    }
    i.min(chars.len())
}

/// A parenthesised argument at `from`: its text and the index after it.
fn argument(chars: &[char], from: usize) -> (Option<String>, usize) {
    if chars.get(from) != Some(&'(') {
        return (None, from);
    }
    let mut depth = 0;
    for (offset, &c) in chars[from..].iter().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let end = from + offset;
                    return (Some(chars[from + 1..end].iter().collect()), end + 1);
                }
            }
            _ => {}
        }
    }
    (Some(chars[from + 1..].iter().collect()), chars.len())
}

/// A selector's specificity, and whether it names a pseudo-element
/// (`::before`, `:first-line` …), whose generated content isn't drawn.
#[must_use]
pub fn specificity(selector: &str) -> (Specificity, bool) {
    let chars: Vec<char> = selector.chars().collect();
    let (mut a, mut b, mut c) = (0, 0, 0);
    let mut pseudo_element = false;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '#' => {
                a += 1;
                i = name_end(&chars, i + 1);
            }
            '.' => {
                b += 1;
                i = name_end(&chars, i + 1);
            }
            '[' => {
                b += 1;
                i = chars[i..].iter().position(|&ch| ch == ']').map_or(chars.len(), |p| i + p + 1);
            }
            ':' if chars.get(i + 1) == Some(&':') => {
                c += 1;
                pseudo_element = true;
                let end = name_end(&chars, i + 2);
                i = argument(&chars, end).1;
            }
            ':' => {
                let end = name_end(&chars, i + 1);
                let name: String = chars[i + 1..end].iter().collect::<String>().to_ascii_lowercase();
                let (inner, after) = argument(&chars, end);
                match name.as_str() {
                    "before" | "after" | "first-line" | "first-letter" => {
                        c += 1;
                        pseudo_element = true;
                    }
                    "where" => {}
                    // These take the most specific selector in their argument.
                    "not" | "is" | "has" | "matches" | "-webkit-any" | "-moz-any" => {
                        let inner = inner.unwrap_or_default();
                        let best = split_top(&inner, ',').into_iter().map(specificity).max_by_key(|(s, _)| *s);
                        if let Some(((ia, ib, ic), pe)) = best {
                            a += ia;
                            b += ib;
                            c += ic;
                            pseudo_element |= pe;
                        }
                    }
                    _ => b += 1,
                }
                i = after;
            }
            ch if is_name_char(ch) && !ch.is_ascii_digit() => {
                c += 1;
                i = name_end(&chars, i);
            }
            _ => i += 1,
        }
    }
    ((a, b, c), pseudo_element)
}

// ---------------------------------------------------------------------------
// Values

/// Points per CSS pixel: CSS fixes 96 px to the inch.
pub const PX: f32 = 0.75;

/// `value` as a number and the unit after it.
fn number_and_unit(value: &str) -> Option<(f32, &str)> {
    let value = value.trim();
    let end = value
        .char_indices()
        .find(|&(at, c)| !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && at == 0)))
        .map_or(value.len(), |(at, _)| at);
    let number: f32 = value[..end].parse().ok()?;
    number.is_finite().then_some((number, &value[end..]))
}

/// A length in points. `em` is the font size it's relative to (the
/// element's own, or its parent's for `font-size`), `rem` the root's; a
/// percentage needs what it is a percentage of.
#[must_use]
pub fn length(value: &str, em: f32, rem: f32, percent_of: Option<f32>) -> Option<f32> {
    let (n, unit) = number_and_unit(value)?;
    let points = match unit.to_ascii_lowercase().as_str() {
        "px" => n * PX,
        "pt" => n,
        "pc" => n * 12.0,
        "in" => n * 72.0,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "q" => n * 72.0 / 101.6,
        "em" => n * em,
        "rem" => n * rem,
        // Without the font's own x-height, half an em is the usual stand-in.
        "ex" | "ch" => n * em * 0.5,
        "%" => n * percent_of? / 100.0,
        "" if n.abs() < f32::EPSILON => 0.0,
        _ => return None,
    };
    Some(points)
}

/// A bare number, as `line-height: 1.5` takes.
#[must_use]
pub fn plain_number(value: &str) -> Option<f32> {
    number_and_unit(value).filter(|(_, unit)| unit.is_empty()).map(|(n, _)| n)
}

/// `font-size`: keywords, `larger`/`smaller`, or a length where `em` and `%`
/// are the parent's size.
#[must_use]
pub fn font_size(value: &str, parent: f32, root: f32) -> Option<f32> {
    let px = |p: f32| Some(p * PX);
    match value.trim().to_ascii_lowercase().as_str() {
        "xx-small" => px(9.0),
        "x-small" => px(10.0),
        "small" => px(13.0),
        "medium" => px(16.0),
        "large" => px(18.0),
        "x-large" => px(24.0),
        "xx-large" => px(32.0),
        "xxx-large" => px(48.0),
        "larger" => Some(parent * 1.2),
        "smaller" => Some(parent / 1.2),
        other => length(other, parent, root, Some(parent)).filter(|s| *s > 0.0),
    }
}

/// A colour value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    Rgb([u8; 3]),
    Transparent,
    /// `currentColor`: the element's own text colour.
    Current,
}

fn hex_digit(c: char) -> Option<u8> {
    c.to_digit(16).and_then(|d| u8::try_from(d).ok())
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn channel(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// `rgb()` / `rgba()` arguments: three numbers or percentages, and an
/// optional alpha, separated by commas or spaces and a `/`.
fn rgb_function(arguments: &str) -> Option<Paint> {
    let parts: Vec<&str> = arguments.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    if parts.len() < 3 {
        return None;
    }
    let component = |p: &str| match p.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f32>().ok().map(|v| v * 2.55),
        None => p.parse::<f32>().ok(),
    };
    let rgb = [channel(component(parts[0])?), channel(component(parts[1])?), channel(component(parts[2])?)];
    let alpha = parts.get(3).and_then(|a| match a.strip_suffix('%') {
        Some(percent) => percent.parse::<f32>().ok().map(|v| v / 100.0),
        None => a.parse::<f32>().ok(),
    });
    Some(if alpha.is_some_and(|a| a <= 0.0) { Paint::Transparent } else { Paint::Rgb(rgb) })
}

/// `hsl()` / `hsla()` arguments.
fn hsl_function(arguments: &str) -> Option<Paint> {
    let parts: Vec<&str> = arguments.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    if parts.len() < 3 {
        return None;
    }
    let hue = parts[0].trim_end_matches("deg").parse::<f32>().ok()?.rem_euclid(360.0) / 360.0;
    let saturation = parts[1].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
    let lightness = parts[2].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
    if parts.get(3).and_then(|a| a.trim_end_matches('%').parse::<f32>().ok()).is_some_and(|a| a <= 0.0) {
        return Some(Paint::Transparent);
    }
    // CSS Color 4's hsl-to-rgb.
    let k = |n: f32| (n + hue * 12.0).rem_euclid(12.0);
    let a = saturation * lightness.min(1.0 - lightness);
    let f = |n: f32| lightness - a * (k(n) - 3.0).min(9.0 - k(n)).clamp(-1.0, 1.0);
    Some(Paint::Rgb([channel(f(0.0) * 255.0), channel(f(8.0) * 255.0), channel(f(4.0) * 255.0)]))
}

/// A colour: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `hsl()`, a
/// named colour, `transparent` or `currentColor`.
#[must_use]
pub fn colour(value: &str) -> Option<Paint> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        let digits: Vec<u8> = hex.chars().map(hex_digit).collect::<Option<_>>()?;
        let (rgb, alpha) = match digits.len() {
            3 | 4 => ([digits[0] * 17, digits[1] * 17, digits[2] * 17], digits.get(3).map(|a| a * 17)),
            6 | 8 => (
                [digits[0] * 16 + digits[1], digits[2] * 16 + digits[3], digits[4] * 16 + digits[5]],
                (digits.len() == 8).then(|| digits[6] * 16 + digits[7]),
            ),
            _ => return None,
        };
        return Some(if alpha == Some(0) { Paint::Transparent } else { Paint::Rgb(rgb) });
    }
    if let Some(arguments) = value.strip_prefix("rgba(").or_else(|| value.strip_prefix("rgb(")) {
        return rgb_function(arguments.trim_end_matches(')'));
    }
    if let Some(arguments) = value.strip_prefix("hsla(").or_else(|| value.strip_prefix("hsl(")) {
        return hsl_function(arguments.trim_end_matches(')'));
    }
    match value.as_str() {
        "transparent" => Some(Paint::Transparent),
        "currentcolor" => Some(Paint::Current),
        name => NAMED
            .binary_search_by_key(&name, |(n, _)| n)
            .ok()
            .map(|at| NAMED[at].1)
            .map(|rgb| {
                let [_, r, g, b] = rgb.to_be_bytes();
                Paint::Rgb([r, g, b])
            }),
    }
}

/// A `font-family` list, quotes removed.
#[must_use]
pub fn families(value: &str) -> Vec<String> {
    split_top(value, ',')
        .into_iter()
        .map(|f| {
            let f = f.trim();
            let unquoted = f.strip_prefix('"').and_then(|f| f.strip_suffix('"')).or_else(|| f.strip_prefix('\'').and_then(|f| f.strip_suffix('\'')));
            unquoted.map_or_else(|| f.split_whitespace().collect::<Vec<_>>().join(" "), str::to_owned)
        })
        .filter(|f| !f.is_empty())
        .collect()
}

/// The `font` shorthand, split into its parts. Values stay text: their units
/// depend on the element.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Font {
    pub italic: bool,
    pub bold: bool,
    pub size: String,
    pub line_height: Option<String>,
    pub families: Vec<String>,
}

/// `font: [style] [variant] [weight] [stretch] size[/line-height] family, …`.
/// `None` for system fonts (`font: menu`) and anything without a size.
#[must_use]
pub fn font_shorthand(value: &str) -> Option<Font> {
    let mut font = Font::default();
    let mut rest = value.trim();
    loop {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..end];
        if token.is_empty() {
            return None;
        }
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "italic" | "oblique" => font.italic = true,
            "bold" | "bolder" => font.bold = true,
            "normal" | "small-caps" | "lighter" | "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed"
            | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded" => {}
            weight if weight.parse::<u16>().is_ok_and(|w| (1..=1000).contains(&w)) => font.bold = weight.parse::<u16>().is_ok_and(|w| w >= 600),
            _ => {
                let (size, line_height) = match token.split_once('/') {
                    Some((s, lh)) => (s, Some(lh.to_owned())),
                    None => (token, None),
                };
                font_size(size, 12.0, 12.0)?;
                size.clone_into(&mut font.size);
                font.line_height = line_height;
                let mut after = rest[end..].trim_start();
                // `12px / 1.5 Georgia`: the line height may stand apart.
                if font.line_height.is_none() {
                    if let Some(lh) = after.strip_prefix('/') {
                        let lh = lh.trim_start();
                        let lh_end = lh.find(char::is_whitespace).unwrap_or(lh.len());
                        font.line_height = Some(lh[..lh_end].to_owned());
                        after = lh[lh_end..].trim_start();
                    }
                }
                font.families = families(after);
                return (!font.families.is_empty()).then_some(font);
            }
        }
        rest = rest[end..].trim_start();
    }
}

/// One to four values for top, right, bottom and left, as `margin` takes.
#[must_use]
pub fn sides(value: &str) -> Option<[String; 4]> {
    let parts: Vec<&str> = split_top(value.trim(), ' ').into_iter().map(str::trim).filter(|p| !p.is_empty()).collect();
    let [t, r, b, l] = match parts.as_slice() {
        [all] => [all, all, all, all],
        [v, h] => [v, h, v, h],
        [t, h, b] => [t, h, b, h],
        [t, r, b, l] => [t, r, b, l],
        _ => return None,
    };
    Some([(*t).to_owned(), (*r).to_owned(), (*b).to_owned(), (*l).to_owned()])
}

/// The parts of a `border` shorthand; any may be missing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BorderValue {
    /// Points.
    pub width: Option<f32>,
    pub style: Option<String>,
    pub colour: Option<Paint>,
}

/// A border width: a length or `thin` / `medium` / `thick`.
#[must_use]
pub fn border_width(value: &str, em: f32, rem: f32) -> Option<f32> {
    match value.trim().to_ascii_lowercase().as_str() {
        "thin" => Some(1.0 * PX),
        "medium" => Some(3.0 * PX),
        "thick" => Some(5.0 * PX),
        other => length(other, em, rem, None).filter(|w| *w >= 0.0),
    }
}

/// Whether `value` is a border style keyword.
#[must_use]
pub fn is_border_style(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "none" | "hidden" | "dotted" | "dashed" | "solid" | "double" | "groove" | "ridge" | "inset" | "outset"
    )
}

/// `border: 1px solid red`, in any order.
#[must_use]
pub fn border(value: &str, em: f32, rem: f32) -> BorderValue {
    let mut out = BorderValue::default();
    for token in split_top(value.trim(), ' ').into_iter().map(str::trim).filter(|t| !t.is_empty()) {
        if is_border_style(token) {
            out.style = Some(token.to_ascii_lowercase());
        } else if let Some(width) = border_width(token, em, rem) {
            out.width = Some(width);
        } else if let Some(paint) = colour(token) {
            out.colour = Some(paint);
        }
    }
    out
}

/// CSS's named colours (CSS Color 4), sorted for binary search. Written as
/// the spec writes them (`#F0F8FF`), which separators would obscure.
#[allow(clippy::unreadable_literal)]
const NAMED: [(&str, u32); 148] = [
    ("aliceblue", 0xF0F8FF), ("antiquewhite", 0xFAEBD7), ("aqua", 0x00FFFF), ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF), ("beige", 0xF5F5DC), ("bisque", 0xFFE4C4), ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD), ("blue", 0x0000FF), ("blueviolet", 0x8A2BE2), ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887), ("cadetblue", 0x5F9EA0), ("chartreuse", 0x7FFF00), ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50), ("cornflowerblue", 0x6495ED), ("cornsilk", 0xFFF8DC), ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF), ("darkblue", 0x00008B), ("darkcyan", 0x008B8B), ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9), ("darkgreen", 0x006400), ("darkgrey", 0xA9A9A9), ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B), ("darkolivegreen", 0x556B2F), ("darkorange", 0xFF8C00), ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000), ("darksalmon", 0xE9967A), ("darkseagreen", 0x8FBC8F), ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F), ("darkslategrey", 0x2F4F4F), ("darkturquoise", 0x00CED1), ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493), ("deepskyblue", 0x00BFFF), ("dimgray", 0x696969), ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF), ("firebrick", 0xB22222), ("floralwhite", 0xFFFAF0), ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF), ("gainsboro", 0xDCDCDC), ("ghostwhite", 0xF8F8FF), ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520), ("gray", 0x808080), ("green", 0x008000), ("greenyellow", 0xADFF2F),
    ("grey", 0x808080), ("honeydew", 0xF0FFF0), ("hotpink", 0xFF69B4), ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082), ("ivory", 0xFFFFF0), ("khaki", 0xF0E68C), ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5), ("lawngreen", 0x7CFC00), ("lemonchiffon", 0xFFFACD), ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080), ("lightcyan", 0xE0FFFF), ("lightgoldenrodyellow", 0xFAFAD2), ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90), ("lightgrey", 0xD3D3D3), ("lightpink", 0xFFB6C1), ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA), ("lightskyblue", 0x87CEFA), ("lightslategray", 0x778899), ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE), ("lightyellow", 0xFFFFE0), ("lime", 0x00FF00), ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6), ("magenta", 0xFF00FF), ("maroon", 0x800000), ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD), ("mediumorchid", 0xBA55D3), ("mediumpurple", 0x9370DB), ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE), ("mediumspringgreen", 0x00FA9A), ("mediumturquoise", 0x48D1CC), ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970), ("mintcream", 0xF5FFFA), ("mistyrose", 0xFFE4E1), ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD), ("navy", 0x000080), ("oldlace", 0xFDF5E6), ("olive", 0x808000),
    ("olivedrab", 0x6B8E23), ("orange", 0xFFA500), ("orangered", 0xFF4500), ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA), ("palegreen", 0x98FB98), ("paleturquoise", 0xAFEEEE), ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5), ("peachpuff", 0xFFDAB9), ("peru", 0xCD853F), ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD), ("powderblue", 0xB0E0E6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xFF0000), ("rosybrown", 0xBC8F8F), ("royalblue", 0x4169E1), ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072), ("sandybrown", 0xF4A460), ("seagreen", 0x2E8B57), ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D), ("silver", 0xC0C0C0), ("skyblue", 0x87CEEB), ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xFFFAFA), ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4), ("tan", 0xD2B48C), ("teal", 0x008080), ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347), ("turquoise", 0x40E0D0), ("violet", 0xEE82EE), ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF), ("whitesmoke", 0xF5F5F5), ("yellow", 0xFFFF00), ("yellowgreen", 0x9ACD32),
];

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rules_are_read_in_order_and_at_rules_skipped() {
        let sheet = r#"
            /* a comment with { braces } */
            @import url("x.css");
            @charset "utf-8";
            p, h1.title { color: red; margin: 0 }
            @media screen { p { color: blue } div { float: left } }
            @font-face { font-family: X; src: url(x.woff) }
            a[href="{odd}"] { text-decoration: underline !important ; }
            <!-- td { padding: 2px } -->
        "#;
        let rules = parse_stylesheet(sheet);
        assert_eq!(rules.len(), 3, "{rules:#?}");
        assert_eq!(rules[0].selectors, vec!["p", "h1.title"]);
        assert_eq!(rules[0].declarations[1], Declaration { name: "margin".into(), value: "0".into(), important: false });
        assert_eq!(rules[1].selectors, vec![r#"a[href="{odd}"]"#]);
        assert!(rules[1].declarations[0].important);
        assert_eq!(rules[1].declarations[0].value, "underline");
        assert_eq!(rules[2].selectors, vec!["td"]);
    }

    #[test]
    fn declarations_skip_custom_properties_and_keep_urls_whole() {
        let d = parse_declarations("color: var(--x); --x: red; background: url(a;b.png) ; font-weight :BOLD;; bad");
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[0].value, "url(a;b.png)");
        assert_eq!((d[1].name.as_str(), d[1].value.as_str()), ("font-weight", "BOLD"));
    }

    #[test]
    fn specificity_counts_like_the_spec() {
        // Examples from Selectors Level 4, §17.
        assert_eq!(specificity("*").0, (0, 0, 0));
        assert_eq!(specificity("li").0, (0, 0, 1));
        assert_eq!(specificity("ul li").0, (0, 0, 2));
        assert_eq!(specificity("ul ol+li").0, (0, 0, 3));
        assert_eq!(specificity("h1 + *[rel=up]").0, (0, 1, 1));
        assert_eq!(specificity("ul ol li.red").0, (0, 1, 3));
        assert_eq!(specificity("li.red.level").0, (0, 2, 1));
        assert_eq!(specificity("#x34y").0, (1, 0, 0));
        assert_eq!(specificity("#s12:not(FOO)").0, (1, 0, 1));
        assert_eq!(specificity(".foo :is(.bar, #baz)").0, (1, 1, 0));
        assert_eq!(specificity(":where(#a) p").0, (0, 0, 1));
        assert_eq!(specificity("tr:nth-child(2n+1) td").0, (0, 1, 2));
        assert!(!specificity("a:hover").1);
        assert_eq!(specificity("p::first-line"), ((0, 0, 2), true));
        assert_eq!(specificity("p:before"), ((0, 0, 2), true));
    }

    fn close(actual: Option<f32>, expected: f32) -> bool {
        actual.is_some_and(|a| (a - expected).abs() < 1e-3)
    }

    #[test]
    fn lengths_become_points() {
        assert!(close(length("16px", 12.0, 12.0, None), 12.0));
        assert!(close(length("1in", 12.0, 12.0, None), 72.0));
        assert!(close(length("2.54cm", 12.0, 12.0, None), 72.0));
        assert!(close(length("25.4mm", 12.0, 12.0, None), 72.0));
        assert!(close(length("6pc", 12.0, 12.0, None), 72.0));
        assert!(close(length("1.5em", 10.0, 12.0, None), 15.0));
        assert!(close(length(".5em", 10.0, 12.0, None), 5.0));
        assert!(close(length("2rem", 10.0, 12.0, None), 24.0));
        assert!(close(length("50%", 10.0, 12.0, Some(300.0)), 150.0));
        assert_eq!(length("50%", 10.0, 12.0, None), None);
        assert!(close(length("0", 10.0, 12.0, None), 0.0));
        assert!(close(length("-0.5em", 10.0, 12.0, None), -5.0));
        assert_eq!(length("12", 10.0, 12.0, None), None);
        assert_eq!(length("auto", 10.0, 12.0, None), None);
        assert!(close(plain_number("1.5"), 1.5));
        assert_eq!(plain_number("1.5em"), None);
    }

    #[test]
    fn font_sizes_follow_the_parent() {
        assert!(close(font_size("medium", 30.0, 12.0), 12.0));
        assert!(close(font_size("2em", 10.0, 12.0), 20.0));
        assert!(close(font_size("150%", 10.0, 12.0), 15.0));
        assert!(close(font_size("larger", 10.0, 12.0), 12.0));
        assert!(close(font_size("x-large", 10.0, 12.0), 18.0));
        assert_eq!(font_size("-3px", 10.0, 12.0), None);
    }

    #[test]
    fn colours_in_every_notation() {
        let rgb = |v: &str| match colour(v) {
            Some(Paint::Rgb(c)) => c,
            other => panic!("{v}: {other:?}"),
        };
        assert_eq!(rgb("#f80"), [255, 136, 0]);
        assert_eq!(rgb("#FF8800"), [255, 136, 0]);
        assert_eq!(rgb("#ff8800cc"), [255, 136, 0]);
        assert_eq!(rgb("rgb(255, 136, 0)"), [255, 136, 0]);
        assert_eq!(rgb("rgb(100% 0% 0% / 50%)"), [255, 0, 0]);
        assert_eq!(rgb("hsl(120, 100%, 25%)"), [0, 128, 0]);
        assert_eq!(rgb("hsl(0deg 100% 50%)"), [255, 0, 0]);
        assert_eq!(rgb("RebeccaPurple"), [0x66, 0x33, 0x99]);
        assert_eq!(rgb("navy"), [0, 0, 128]);
        assert_eq!(colour("transparent"), Some(Paint::Transparent));
        assert_eq!(colour("rgba(0,0,0,0)"), Some(Paint::Transparent));
        assert_eq!(colour("#0000"), Some(Paint::Transparent));
        assert_eq!(colour("currentColor"), Some(Paint::Current));
        assert_eq!(colour("#12"), None);
        assert_eq!(colour("not-a-colour"), None);
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0), "the table must stay sorted");
    }

    #[test]
    fn the_font_shorthand_splits_into_parts() {
        // What macOS's HTML writer emits.
        let f = font_shorthand("12.0px Times").expect("parsed");
        assert_eq!((f.size.as_str(), f.families.clone()), ("12.0px", vec!["Times".to_owned()]));
        let f = font_shorthand(r#"italic bold 14px/1.5 "Helvetica Neue", Arial, sans-serif"#).expect("parsed");
        assert!(f.italic && f.bold);
        assert_eq!(f.line_height.as_deref(), Some("1.5"));
        assert_eq!(f.families, vec!["Helvetica Neue", "Arial", "sans-serif"]);
        let f = font_shorthand("700 1.2em / 2 Georgia").expect("parsed");
        assert!(f.bold);
        assert_eq!((f.size.as_str(), f.line_height.as_deref()), ("1.2em", Some("2")));
        assert_eq!(font_shorthand("menu"), None);
        assert_eq!(font_shorthand("12px"), None, "a family is required");
    }

    #[test]
    fn box_sides_and_borders() {
        assert_eq!(sides("1px 2px").expect("2"), ["1px", "2px", "1px", "2px"].map(String::from));
        assert_eq!(sides("1px 2px 3px").expect("3"), ["1px", "2px", "3px", "2px"].map(String::from));
        assert_eq!(sides("0 auto").expect("2"), ["0", "auto", "0", "auto"].map(String::from));
        assert_eq!(sides("1px 2px 3px 4px 5px"), None);
        let b = border("solid 2px rgb(255, 0, 0)", 12.0, 12.0);
        assert_eq!(b, BorderValue { width: Some(1.5), style: Some("solid".into()), colour: Some(Paint::Rgb([255, 0, 0])) });
        assert_eq!(border("thin dotted", 12.0, 12.0).width, Some(0.75));
        assert_eq!(families(" 'Times New Roman' ,  Courier   New,serif"), vec!["Times New Roman", "Courier New", "serif"]);
    }
}
