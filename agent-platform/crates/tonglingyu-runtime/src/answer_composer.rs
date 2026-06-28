pub(crate) fn public_quote_text(text: &str) -> String {
    let without_refs = strip_tagged_sections(text, "ref");
    let stripped = strip_angle_tags(&strip_wiki_templates(&without_refs))
        .replace("'''", "")
        .replace("<br />", " ")
        .replace("<br/>", " ")
        .replace("<br>", " ");
    collapse_whitespace(&stripped)
}

fn strip_tagged_sections(text: &str, tag: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    while let Some(start) = rest.to_ascii_lowercase().find(&open) {
        output.push_str(&rest[..start]);
        let after_open = &rest[start..];
        let lower_after_open = after_open.to_ascii_lowercase();
        let Some(close_start) = lower_after_open.find(&close) else {
            rest = "";
            break;
        };
        rest = &after_open[close_start + close.len()..];
    }
    output.push_str(rest);
    output
}

fn strip_wiki_templates(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let template = &rest[start + 2..];
        let Some(end) = template.find("}}") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let inner = template[..end].trim();
        if let Some((head, body)) = inner.split_once('|') {
            if !structural_template_name(head.trim()) {
                output.push_str(body);
            }
        } else if !structural_template_name(inner) {
            output.push_str(inner);
        }
        rest = &template[end + 2..];
    }
    output.push_str(rest);
    output
}

fn structural_template_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized.starts_with("block ")
        || normalized.starts_with("center")
        || normalized.starts_with("/center")
}

fn strip_angle_tags(text: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

pub(crate) fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
