//! A small, tolerant XML reader for machine-written files of a known shape (scheduled-task definitions). It builds a tree of
//! elements with their text; it does not validate, resolve namespaces (a prefix is dropped) or follow anything outside the text it
//! is given, so a hostile file can do nothing but be refused: nesting and the number of elements are bounded.

/// Elements nested deeper than this make the file refused (real task files are about 6 deep).
pub const MAX_DEPTH: usize = 64;
/// More elements than this make the file refused (real task files have a few dozen).
pub const MAX_NODES: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XmlNode {
    /// The element's name without any namespace prefix.
    pub name: String,
    /// The text directly inside the element (entities decoded, ends trimmed).
    pub text: String,
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    pub fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|child| child.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// The trimmed text of a child element, empty when there is none.
    pub fn child_text(&self, name: &str) -> &str {
        self.child(name).map_or("", |child| child.text.as_str())
    }
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn local_name(tag: &str) -> &str {
    tag.rsplit(':').next().unwrap_or(tag)
}

/// Reads one document and returns its root element, or `None` when it is not well formed enough to trust (crossed or unclosed
/// tags, nothing but text, too deep, too many elements).
pub fn parse_xml(text: &str) -> Option<XmlNode> {
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut nodes = 0usize;
    let mut rest = text.trim_start_matches('\u{feff}');

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[after.find("-->")? + 3..];
        } else if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let end = after.find("]]>")?;
            stack.last_mut()?.text.push_str(&after[..end]);
            rest = &after[end + 3..];
        } else if let Some(after) = rest.strip_prefix("<?").or_else(|| rest.strip_prefix("<!")) {
            rest = &after[after.find('>')? + 1..];
        } else if let Some(after) = rest.strip_prefix("</") {
            let end = after.find('>')?;
            let name = local_name(after[..end].trim());
            let mut node = stack.pop()?;
            if node.name != name {
                return None;
            }
            node.text = node.text.trim().to_owned();
            match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => {
                    if root.is_some() {
                        return None;
                    }
                    root = Some(node);
                }
            }
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix('<') {
            let end = after.find('>')?;
            let inside = &after[..end];
            let self_closing = inside.ends_with('/');
            let inside = inside.trim_end_matches('/');
            let name = local_name(inside.split_whitespace().next()?).to_owned();
            nodes += 1;
            if nodes > MAX_NODES || stack.len() >= MAX_DEPTH {
                return None;
            }
            let node = XmlNode { name, ..XmlNode::default() };
            if self_closing {
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None if root.is_none() => root = Some(node),
                    None => return None,
                }
            } else {
                stack.push(node);
            }
            rest = &after[end + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let text = decode_entities(&rest[..end]);
            if let Some(open) = stack.last_mut() {
                open.text.push_str(&text);
            }
            rest = &rest[end..];
        }
    }
    if stack.is_empty() {
        root
    } else {
        None
    }
}
