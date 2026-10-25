use crate::html::escape_into;
use crate::{Html, InternalUrl, MAX_COMPONENT_BYTES, MAX_QUERY_PAIRS};
use std::fmt;

/// Maximum ASCII bytes in a declared form field name.
pub const MAX_FORM_NAME_BYTES: usize = 64;

/// A typed GET form construction error, without reflected field data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormError {
    /// GET replaces the action query; use named or hidden fields instead.
    ActionHasQuery,
    /// A field name is not a supported ASCII identifier.
    InvalidName,
    /// A field with this exact name already exists.
    DuplicateName,
    /// The form would exceed the query pair count budget.
    TooManyFields,
    /// A value, label, or button contains a control character.
    InvalidText,
    /// A visible label or submit button has no non-whitespace text.
    EmptyLabel,
    /// Text exceeds the decoded component byte budget.
    TooLarge,
}

impl fmt::Display for FormError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ActionHasQuery => "GET form action must not contain a query",
            Self::InvalidName => "invalid form field name",
            Self::DuplicateName => "duplicate form field name",
            Self::TooManyFields => "form field count limit exceeded",
            Self::InvalidText => "control character in form text",
            Self::EmptyLabel => "form label must not be empty",
            Self::TooLarge => "form text byte limit exceeded",
        })
    }
}

impl std::error::Error for FormError {}

#[derive(Debug)]
enum Field {
    Search {
        name: String,
        label: String,
        value: String,
    },
    Hidden {
        name: String,
        value: String,
    },
}

impl Field {
    fn name(&self) -> &str {
        match self {
            Self::Search { name, .. } | Self::Hidden { name, .. } => name,
        }
    }
}

fn valid_text(value: &str) -> Result<(), FormError> {
    if value.len() > MAX_COMPONENT_BYTES {
        return Err(FormError::TooLarge);
    }
    if value.chars().any(char::is_control) {
        return Err(FormError::InvalidText);
    }
    Ok(())
}

fn valid_label(value: &str) -> Result<(), FormError> {
    valid_text(value)?;
    if value.trim().is_empty() {
        return Err(FormError::EmptyLabel);
    }
    Ok(())
}

/// A native, read-only GET form with typed controls and an internal action.
///
/// Search controls have implicit wrapping labels; hidden controls and the
/// unnamed submit button cannot override method or action. No arbitrary HTML,
/// URL, event, or form-override attribute is accepted. Values are plain text,
/// escaped for HTML; the browser performs URL-encoded submission.
///
/// ```
/// use ruvane::{GetForm, InternalUrl};
/// let form = GetForm::new(InternalUrl::new("/articles").unwrap()).unwrap()
///     .search("search", "Search articles", "Rust + HTML").unwrap()
///     .hidden("page", "1").unwrap()
///     .submit_label("Search").unwrap();
/// assert!(form.render().as_str().contains("value=\"Rust + HTML\""));
/// ```
#[must_use]
#[derive(Debug)]
pub struct GetForm {
    action: InternalUrl,
    fields: Vec<Field>,
    submit: String,
}

impl GetForm {
    /// Creates an empty GET form with a "Submit" button.
    ///
    /// The action must have no query. Its route should be registered with
    /// `Router::get_form`. URL validity does not check that a route exists.
    pub fn new(action: InternalUrl) -> Result<Self, FormError> {
        if action.as_str().contains('?') {
            return Err(FormError::ActionHasQuery);
        }
        Ok(Self {
            action,
            fields: Vec::new(),
            submit: String::from("Submit"),
        })
    }

    fn validate_name(&self, name: &str) -> Result<(), FormError> {
        let mut bytes = name.bytes();
        if name.len() > MAX_FORM_NAME_BYTES
            || !bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(FormError::InvalidName);
        }
        if self.fields.iter().any(|field| field.name() == name) {
            return Err(FormError::DuplicateName);
        }
        if self.fields.len() == MAX_QUERY_PAIRS {
            return Err(FormError::TooManyFields);
        }
        Ok(())
    }

    /// Adds a search input with a nonempty visible label and escaped value.
    ///
    /// Names follow `[A-Za-z_][A-Za-z0-9_-]*`, up to `MAX_FORM_NAME_BYTES`.
    /// The HTML maxlength is client guidance in UTF-16 units, not a security
    /// check; the server independently enforces decoded UTF-8 byte budgets.
    pub fn search(
        mut self,
        name: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, FormError> {
        let (name, label, value) = (name.into(), label.into(), value.into());
        self.validate_name(&name)?;
        valid_label(&label)?;
        valid_text(&value)?;
        self.fields.push(Field::Search { name, label, value });
        Ok(self)
    }

    /// Adds a hidden field with escaped plain text. Hidden values remain
    /// untrusted client input and must be validated by the receiving handler.
    pub fn hidden(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, FormError> {
        let (name, value) = (name.into(), value.into());
        self.validate_name(&name)?;
        valid_text(&value)?;
        self.fields.push(Field::Hidden { name, value });
        Ok(self)
    }

    /// Sets the nonempty, escaped label of the unnamed submit button.
    pub fn submit_label(mut self, label: impl Into<String>) -> Result<Self, FormError> {
        let label = label.into();
        valid_label(&label)?;
        self.submit = label;
        Ok(self)
    }

    /// Renders the form with fixed GET/UTF-8 attributes and void inputs.
    ///
    /// Field order is retained. Wrapping labels avoid generated ID collisions.
    /// There is no POST, multipart, file control, or JavaScript behavior.
    pub fn render(self) -> Html {
        let mut markup = String::from("<form method=\"get\" action=\"");
        escape_into(&mut markup, self.action.as_str());
        markup.push_str("\" accept-charset=\"UTF-8\">");
        for field in self.fields {
            let (name, value) = match &field {
                Field::Search { name, label, value } => {
                    markup.push_str("<label><span>");
                    escape_into(&mut markup, label);
                    markup.push_str("</span><input type=\"search\"");
                    (name, value)
                }
                Field::Hidden { name, value } => {
                    markup.push_str("<input type=\"hidden\"");
                    (name, value)
                }
            };
            markup.push_str(" name=\"");
            escape_into(&mut markup, name);
            markup.push_str("\" value=\"");
            escape_into(&mut markup, value);
            markup.push('"');
            if matches!(field, Field::Search { .. }) {
                markup.push_str(" maxlength=\"");
                markup.push_str(&MAX_COMPONENT_BYTES.to_string());
                markup.push_str("\"></label>");
            } else {
                markup.push('>');
            }
        }
        markup.push_str("<button type=\"submit\">");
        escape_into(&mut markup, &self.submit);
        markup.push_str("</button></form>");
        Html::rendered(markup)
    }
}

impl From<GetForm> for Html {
    fn from(form: GetForm) -> Self {
        form.render()
    }
}
