use std::fmt;

/// An already rendered HTML fragment produced by Ruvane.
///
/// Construct text with [`text`] or compose markup with [`element`] and
/// [`fragment`]. Converting a string into `Html` always treats it as text and
/// escapes it. There is deliberately no raw HTML constructor.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct Html {
    markup: String,
}

impl Html {
    /// Borrows the serialized HTML, for example to write it to an HTTP body.
    pub fn as_str(&self) -> &str {
        &self.markup
    }

    /// Returns the serialized HTML without copying its string buffer.
    pub fn into_string(self) -> String {
        self.markup
    }
}

impl fmt::Display for Html {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.markup)
    }
}

impl AsRef<str> for Html {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for Html {
    fn from(value: &str) -> Self {
        text(value)
    }
}

impl From<String> for Html {
    fn from(value: String) -> Self {
        text(value)
    }
}

/// Renders a string as text, escaping markup and replacing null characters.
///
/// This is an HTML text encoder, not a sanitizer for existing HTML or an
/// encoder for JavaScript, CSS, or URLs.
///
/// ```
/// assert_eq!(ruvane::text("<Rust & HTML>").as_str(), "&lt;Rust &amp; HTML&gt;");
/// ```
pub fn text(value: impl AsRef<str>) -> Html {
    let mut markup = String::new();
    escape_into(&mut markup, value.as_ref());
    Html { markup }
}

/// Joins already rendered fragments in their original order.
///
/// Empty input produces empty HTML. Rendered children are not escaped again.
pub fn fragment(children: impl IntoIterator<Item = Html>) -> Html {
    let mut markup = String::new();
    for child in children {
        markup.push_str(child.as_str());
    }
    Html { markup }
}

/// The normal HTML elements supported by the initial renderer.
///
/// Element names are selected from this enum, never from untrusted strings.
/// Script, style, foreign content, and void elements are outside this initial
/// API. Ruvane does not validate the HTML content model of nested elements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Tag {
    /// A generic block container.
    Div,
    /// A generic inline container.
    Span,
    /// A paragraph.
    P,
    /// A first-level heading.
    H1,
    /// A second-level heading.
    H2,
    /// A third-level heading.
    H3,
    /// The main content of a document.
    Main,
    /// A section of a document.
    Section,
    /// A self-contained article.
    Article,
    /// Introductory content.
    Header,
    /// A footer for a document or section.
    Footer,
    /// A section containing navigation.
    Nav,
    /// An unordered list.
    Ul,
    /// An ordered list.
    Ol,
    /// An item in a list.
    Li,
    /// Strong importance.
    Strong,
    /// Emphasized text.
    Em,
    /// A fragment of computer code.
    Code,
    /// Preformatted text.
    Pre,
    /// A block quotation.
    Blockquote,
}

impl Tag {
    fn as_str(self) -> &'static str {
        match self {
            Self::Div => "div",
            Self::Span => "span",
            Self::P => "p",
            Self::H1 => "h1",
            Self::H2 => "h2",
            Self::H3 => "h3",
            Self::Main => "main",
            Self::Section => "section",
            Self::Article => "article",
            Self::Header => "header",
            Self::Footer => "footer",
            Self::Nav => "nav",
            Self::Ul => "ul",
            Self::Ol => "ol",
            Self::Li => "li",
            Self::Strong => "strong",
            Self::Em => "em",
            Self::Code => "code",
            Self::Pre => "pre",
            Self::Blockquote => "blockquote",
        }
    }
}

/// The plain, double-quoted attributes supported by the initial renderer.
///
/// Event handlers, style attributes, and URL attributes are not accepted by
/// this enum. Attribute values are always escaped when rendered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Attribute {
    /// The element identifier.
    Id,
    /// A space-separated list of CSS classes.
    Class,
    /// Advisory information about the element.
    Title,
}

impl Attribute {
    fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Class => "class",
            Self::Title => "title",
        }
    }
}

/// A builder for an HTML element with escaped attributes and children.
///
/// Children are serialized as they are added, rather than retained in a
/// recursively traversed tree. The caller remains responsible for input and
/// output size limits.
#[derive(Clone, Debug)]
#[must_use]
pub struct Element {
    tag: Tag,
    attributes: Vec<(Attribute, String)>,
    children: String,
}

/// Starts building an HTML element with a fixed tag name.
///
/// ```
/// use ruvane::{Attribute, Tag, element};
///
/// let heading = element(Tag::H1)
///     .attr(Attribute::Class, "title")
///     .child("Hello <world>")
///     .render();
///
/// assert_eq!(heading.as_str(), "<h1 class=\"title\">Hello &lt;world&gt;</h1>");
/// ```
pub fn element(tag: Tag) -> Element {
    Element {
        tag,
        attributes: Vec::new(),
        children: String::new(),
    }
}

impl Element {
    /// Sets an attribute, replacing its previous value if it was already set.
    ///
    /// Replacing an attribute preserves its position in the serialized output.
    pub fn attr(mut self, attribute: Attribute, value: impl Into<String>) -> Self {
        let value = value.into();
        if let Some((_, previous)) = self
            .attributes
            .iter_mut()
            .find(|(existing, _)| *existing == attribute)
        {
            *previous = value;
        } else {
            self.attributes.push((attribute, value));
        }
        self
    }

    /// Appends a child element, rendered fragment, or string.
    ///
    /// Strings are treated as text and escaped. Rendered children keep their
    /// markup without being escaped a second time.
    pub fn child(mut self, child: impl Into<Html>) -> Self {
        let child = child.into();
        self.children.push_str(child.as_str());
        self
    }

    /// Serializes this element into an HTML fragment.
    pub fn render(self) -> Html {
        let mut markup = String::new();
        markup.push('<');
        markup.push_str(self.tag.as_str());
        for (attribute, value) in self.attributes {
            markup.push(' ');
            markup.push_str(attribute.as_str());
            markup.push_str("=\"");
            escape_into(&mut markup, &value);
            markup.push('"');
        }
        markup.push('>');
        markup.push_str(&self.children);
        markup.push_str("</");
        markup.push_str(self.tag.as_str());
        markup.push('>');
        Html { markup }
    }
}

impl From<Element> for Html {
    fn from(value: Element) -> Self {
        value.render()
    }
}

/// A complete UTF-8 HTML document with a title, language, and body.
///
/// The title and language are escaped in their respective quoted/text
/// contexts. The language value is not validated as a BCP 47 language tag.
#[derive(Clone, Debug)]
#[must_use]
pub struct Document {
    title: String,
    language: String,
    body: Html,
}

impl Document {
    /// Creates a document whose default language is English (`en`).
    pub fn new(title: impl Into<String>, body: impl Into<Html>) -> Self {
        Self {
            title: title.into(),
            language: String::from("en"),
            body: body.into(),
        }
    }

    /// Sets the document language, for example `fr` or `en-GB`.
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// Serializes the document with a doctype and UTF-8 character declaration.
    pub fn render(self) -> Html {
        let mut markup = String::from("<!doctype html><html lang=\"");
        escape_into(&mut markup, &self.language);
        markup.push_str("\"><head><meta charset=\"utf-8\"><title>");
        escape_into(&mut markup, &self.title);
        markup.push_str("</title></head><body>");
        markup.push_str(self.body.as_str());
        markup.push_str("</body></html>");
        Html { markup }
    }
}

fn escape_into(output: &mut String, input: &str) {
    for character in input.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            '\0' => output.push('\u{fffd}'),
            other => output.push(other),
        }
    }
}
